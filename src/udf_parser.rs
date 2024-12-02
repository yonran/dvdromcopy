use log::{debug, error};
use std::{
    arch::aarch64::__crc32b,
    collections::BTreeMap,
    io::{self, Read, Seek, SeekFrom},
    mem::offset_of,
    ptr::addr_of,
    vec,
};
use thiserror::Error;

use crate::{
    cache::Cache,
    crc::cksum,
    dvdcss_sys::DVDCSS_BLOCK_SIZE,
    logical_block_reader::{read_exact_from_partition, short_ad_to_pos_in_partition},
    udf::{
        AnchorVolumeDescriptorPointer, DescriptorTag, FileEntry, FileIdentifierDescriptor,
        FileSetDescriptor, ICBTag, IndirectEntry, LbAddr, LogicalVolumeDescriptor, LongAd,
        PartitionDescriptor, PartitionMap, PrimaryVolumeDescriptor, ShortAllocationDescriptor,
        TerminalEntry, TerminatingDescriptor, Type1PartitionMap,
    },
};

#[derive(Error, Debug)]
pub enum UdfError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("Invalid descriptor tag")]
    InvalidDescriptorTag,
    #[error("Invalid partition map")]
    InvalidPartitionMap,
    #[error("Buffer too small")]
    BufferTooSmall,
    #[error("Invalid offset")]
    InvalidOffset,
    #[error("Invalid partition number")]
    InvalidPartitionNumber,
}

pub type Result<T> = std::result::Result<T, UdfError>;

/// UDF Parser that handles reading UDF structures from a source
pub struct UdfParser<R: Read + Seek> {
    pub reader: R,
    pub sector_size: u32,
    data_offset: u32,
}

/// Standard logical sector size for UDF
const LOGICAL_SECTOR_SIZE: u32 = 2048;
/// Raw CD-ROM mode 1/2 sector size
const RAW_CD_SECTOR_SIZE: u32 = 2352;
/// Offset to user data in raw CD-ROM mode 1/2 sectors
const RAW_CD_DATA_OFFSET: u32 = 16;

impl<R: Read + Seek> UdfParser<R> {
    /// common-case new for dvds
    pub fn new(reader: R) -> Self {
        Self::new_with_sector_size(reader, LOGICAL_SECTOR_SIZE, 0)
    }
    /// Create a new parser for raw CD-ROM sectors (2352 bytes)
    pub fn new_raw_cd(reader: R) -> Self {
        Self::new_with_sector_size(reader, RAW_CD_SECTOR_SIZE, RAW_CD_DATA_OFFSET)
    }

    /// Create a new parser with custom sector size and data offset
    pub fn new_with_sector_size(reader: R, sector_size: u32, data_offset: u32) -> Self {
        Self {
            reader,
            sector_size,
            data_offset,
        }
    }

    /// Read an Anchor Volume Descriptor Pointer from one of its standard locations
    pub fn read_anchor(&mut self) -> Result<AnchorVolumeDescriptorPointer> {
        debug!("read_anchor");
        // Try standard locations: sector 256, N-256, and N
        let mut buf = vec![0u8; LOGICAL_SECTOR_SIZE as usize];

        // Try sector 256 first
        if let Ok(anchor) = self.read_anchor_at_sector(256, &mut buf) {
            return Ok(anchor);
        }

        debug!("read_anchor: trying N-256");
        // Get total size to try N-256 and N
        let total_sectors = self.get_total_sectors()?;

        // Try N-256
        if let Ok(anchor) = self.read_anchor_at_sector(total_sectors - 256, &mut buf) {
            return Ok(anchor);
        }

        debug!("read_anchor: trying N");
        // Try N
        self.read_anchor_at_sector(total_sectors - 1, &mut buf)
    }

    pub fn seek_to_sector(&mut self, sector: u32) -> Result<()> {
        let position = sector as u64 * self.sector_size as u64 + self.data_offset as u64;
        self.reader.seek(SeekFrom::Start(position))?;
        Ok(())
    }
    fn read_anchor_at_sector(
        &mut self,
        sector: u32,
        buf: &mut [u8],
    ) -> Result<AnchorVolumeDescriptorPointer> {
        debug!(
            "read_anchor_at_sector: buf={} length, sector={}",
            buf.len(),
            sector
        );
        self.seek_to_sector(sector);
        self.reader.read_exact(buf)?;
        let anchor = AnchorVolumeDescriptorPointer::read(buf);

        // Validate descriptor tag
        if !validate_descriptor_tag(&anchor.tag, buf) {
            return Err(UdfError::InvalidDescriptorTag);
        }

        Ok(anchor)
    }

    /// Read the Primary Volume Descriptor from the specified location
    pub fn read_primary_volume_descriptor(
        &mut self,
        location: u32,
    ) -> Result<PrimaryVolumeDescriptor> {
        let mut buf: Vec<u8> =
            vec![0u8; PrimaryVolumeDescriptor::size().max(LOGICAL_SECTOR_SIZE as usize)];
        debug!("read_primary_volume_descriptor");
        self.seek_to_sector(location)?;
        self.reader.read_exact(&mut buf)?;

        let pvd = PrimaryVolumeDescriptor::read(&buf);

        // Validate descriptor tag
        if !validate_descriptor_tag(&pvd.tag, &buf) {
            return Err(UdfError::InvalidDescriptorTag);
        }

        Ok(pvd)
    }

    /// Read the Logical Volume Descriptor
    pub fn read_logical_volume_descriptor(
        &mut self,
        location: u32,
    ) -> Result<(LogicalVolumeDescriptor, Vec<PartitionMap>)> {
        // Read the fixed portion first
        let mut buf = vec![0u8; LogicalVolumeDescriptor::size().max(LOGICAL_SECTOR_SIZE as usize)];
        debug!("read_logical_volume_descriptor");
        self.seek_to_sector(location)?;
        self.reader.read_exact(&mut buf)?;

        let lvd = LogicalVolumeDescriptor::read(&buf[..LogicalVolumeDescriptor::size()]);

        // Read partition maps
        let mut partition_maps = Vec::new();
        let map_table_length = lvd.map_table_length as usize;

        if map_table_length == 0 {
            return Ok((lvd, partition_maps));
        }

        // Read the entire partition map table
        let mut partition_map_extra_buf = Vec::<u8>::new();
        partition_map_extra_buf.resize(
            (1 + map_table_length).div_ceil(LOGICAL_SECTOR_SIZE as usize)
                * LOGICAL_SECTOR_SIZE as usize,
            0,
        );
        partition_map_extra_buf[..LOGICAL_SECTOR_SIZE as usize]
            .copy_from_slice(&buf[..LOGICAL_SECTOR_SIZE as usize]);
        debug!(
            "reading extra partition map {}",
            partition_map_extra_buf[LOGICAL_SECTOR_SIZE as usize..].len()
        );
        self.reader
            .read_exact(&mut partition_map_extra_buf[LOGICAL_SECTOR_SIZE as usize..])?;

        // Validate descriptor tag
        if !validate_descriptor_tag(&lvd.tag, &partition_map_extra_buf) {
            return Err(UdfError::InvalidDescriptorTag);
        }

        let partition_map_buf = &partition_map_extra_buf[LogicalVolumeDescriptor::size()..];

        let mut offset = 0;
        let mut maps_read = 0;

        while maps_read < lvd.number_of_partition_maps {
            // Ensure we have at least enough bytes for the header
            if offset + 2 > partition_map_buf.len() {
                return Err(UdfError::BufferTooSmall);
            }

            // Peek at the header to get the map length
            let map_type = partition_map_buf[offset];
            let map_length = partition_map_buf[offset + 1];
            debug!(
                "Partition map entry at offset {}: type={} length={}",
                offset, map_type, map_length
            );

            // Validate we have enough bytes for the full map
            if offset + map_length as usize > partition_map_buf.len() {
                return Err(UdfError::BufferTooSmall);
            }

            // Read the appropriate partition map type
            match PartitionMap::read(&partition_map_buf[offset..]) {
                Ok(map) => {
                    offset += map.get_length() as usize;
                    partition_maps.push(map);
                }
                Err(e) => {
                    debug!("Error reading partition map: {:?}", e);
                    return Err(UdfError::InvalidPartitionMap);
                }
            }

            maps_read += 1;
        }

        // Verify we read exactly the right amount of data
        if offset != map_table_length {
            debug!(
                "Partition map table length mismatch: read {} bytes but expected {}",
                offset, map_table_length
            );
            return Err(UdfError::InvalidPartitionMap);
        }

        Ok((lvd, partition_maps))
    }

    fn get_total_sectors(&mut self) -> Result<u32> {
        debug!("get_total_sectors");
        let current = self.reader.stream_position()?;
        let size = self.reader.seek(SeekFrom::End(0))?;
        self.reader.seek(SeekFrom::Start(current))?;
        Ok(((size - self.data_offset as u64) / self.sector_size as u64) as u32)
    }

    pub fn read_fileset_descriptors(
        &mut self,
        partition_descriptor: &crate::udf::PartitionDescriptor,
        _partition_map: &Type1PartitionMap,
    ) -> Result<Vec<FileSetDescriptor>> {
        // for type 1, file set descriptor is always at partition starting location
        // ECMA-167 4/8.3.1 File Set Descriptor Sequence
        // https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=76
        // 6.9.2.4 Step 4. File Set Descriptor:
        // “The File Set Descriptor is located at logical sector numbers:
        // Partition_Location + FSD_Location through
        // Partition_Location + FSD_Location + (FSD_Length - 1) / BlockSize”
        // http://www.osta.org/specs/pdf/udf260.pdf#page=136
        self.reader.seek(SeekFrom::Start(
            partition_descriptor.partition_starting_location as u64 * self.sector_size as u64
                + self.data_offset as u64,
        ))?;

        let mut buf = vec![0u8; LOGICAL_SECTOR_SIZE as usize];
        let mut read_block_count = 0;
        let mut fsds: Vec<FileSetDescriptor> = Vec::new();
        'outer: while read_block_count < partition_descriptor.partition_length {
            self.reader.read_exact(&mut buf)?;
            read_block_count += 1;
            for chunk in buf.chunks_exact(512) {
                let tag = DescriptorTag::read(&buf);
                if !validate_descriptor_tag(&tag, &buf) {
                    return Err(UdfError::InvalidDescriptorTag);
                }
                if tag.tag_identifier == TerminatingDescriptor::TAG_IDENTIFIER {
                    debug!("read_fileset_descriptor: found terminating descriptor");
                    break 'outer;
                } else if tag.tag_identifier == FileSetDescriptor::TAG_IDENTIFIER {
                    let fsd = crate::udf::FileSetDescriptor::read(&buf);
                    debug!("read_fileset_descriptor: {:?}", fsd);
                    fsds.push(fsd);
                }
            }
        }
        Ok(fsds)
    }
}
pub fn read_short_allocation_descriptors(descriptors: &[u8]) -> Vec<ShortAllocationDescriptor> {
    descriptors
        .chunks_exact(ShortAllocationDescriptor::size())
        .map(ShortAllocationDescriptor::read)
        .collect()
}

fn validate_descriptor_tag(tag: &DescriptorTag, full_descriptor: &[u8]) -> bool {
    // sum modulo 256 of bytes 0-3 and 5-15 of the tag
    let tag_checksum = full_descriptor[0..4]
        .iter()
        .chain(&full_descriptor[5..16])
        .fold(0u8, |acc, &b| acc.wrapping_add(b));
    if tag.tag_checksum != tag_checksum {
        error!(
            "Descriptor checksum mismatch: expected {:X} but got {:X}",
            tag.tag_checksum, tag_checksum
        );
        return false;
    }

    let start = DescriptorTag::size();
    // let size = offset_of!(DescriptorTag, descriptor_crc) + size_of::<u16>();

    let end = start + tag.descriptor_crc_length as usize;
    debug!(
        "checking descriptor crc: start={} end={} length={} crc={:x}",
        start, end, tag.descriptor_crc_length, tag.descriptor_crc
    );
    let checked_bytes = &full_descriptor[start..end.min(full_descriptor.len())];

    // debug!("checking descriptor crc: start={} end={} length={} crc={:x} of {:?}", start, end, tag.descriptor_crc_length, tag.descriptor_crc, checked_bytes);
    if tag.descriptor_crc_length > 0 && cksum(checked_bytes) != tag.descriptor_crc {
        error!(
            "Descriptor CRC mismatch: expected {:X} but got {:X}",
            tag.descriptor_crc,
            cksum(checked_bytes)
        );
        return false;
    }
    // TODO:
    // - Check descriptor version
    true
}

pub struct DirectoryFileIdentifierDescriptor(pub FileIdentifierDescriptor);
pub struct FileFileIdentifierDescriptor(pub FileIdentifierDescriptor);
pub enum FileIdentifierEnum {
    Directory(DirectoryFileIdentifierDescriptor),
    File(FileFileIdentifierDescriptor),
}
impl FileIdentifierEnum {
    pub fn new(file_identifier_descriptor: FileIdentifierDescriptor) -> Self {
        if file_identifier_descriptor.file_characteristics
            & FileIdentifierDescriptor::FILE_CHARACTERISTIC_DIRECTORY
            != 0
        {
            FileIdentifierEnum::Directory(DirectoryFileIdentifierDescriptor(
                file_identifier_descriptor,
            ))
        } else {
            FileIdentifierEnum::File(FileFileIdentifierDescriptor(file_identifier_descriptor))
        }
    }
}

/// Typically there should be just one FileEntry in a file's ICB
/// but there can be mulitple ones to handle overflow
/// see 8.10 Information Control Block (ICB) https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=81
pub fn read_file_entries<R: Read + Seek>(
    reader: &mut Cache<&mut R, DVDCSS_BLOCK_SIZE>,
    logical_volume_descriptor: &LogicalVolumeDescriptor,
    partition_descriptor: &PartitionDescriptor,
    short_ad: &ShortAllocationDescriptor,
) -> Result<Vec<FileEntry>> {
    let mut bytes = vec![0u8; short_ad.extent_length_bytes() as usize];
    read_exact_from_partition(
        reader,
        partition_descriptor,
        short_ad.extent_location as usize
            * logical_volume_descriptor.logical_block_size as usize,
        &mut bytes,
    )?;

    debug!(
        "Found matching partition descriptor: {:?} -> starting location: {} sector",
        partition_descriptor, partition_descriptor.partition_starting_location
    );

    let mut entries = vec![];
    let mut pos_in_icb: u32 = 0;
    let address = short_ad.extent_location;
    while bytes.len() - pos_in_icb as usize >= DescriptorTag::size() {
        let buf = &bytes[pos_in_icb as usize..];
        let tag = DescriptorTag::read(&buf[..DescriptorTag::size()]);
        if tag.tag_identifier == 0 {
            // “an unrecorded logical block, indicating that there are no more entries recorded after this entry”
            // https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=81
            debug!(
                "read_file_entries at {:?} {}: found unrecorded logical block; breaking",
                address, pos_in_icb
            );
            break;
        }
        if !validate_descriptor_tag(&tag, &buf) {
            debug!(
                "read_file_entries at {:?} {}: invalid descriptor tag {:?}",
                address, pos_in_icb, tag
            );
            return Err(UdfError::InvalidDescriptorTag);
        }
        if tag.tag_identifier == FileEntry::TAG_IDENTIFIER {
            // file entry is variable length but
            // “The total length of a File Entry shall not exceed the size of one logical block.”
            // http://www.osta.org/specs/pdf/udf260.pdf#page=75
            let file_entry = FileEntry::read(&buf);
            debug!(
                "read_file_entries at {:?} {}: FileEntry {:?}",
                address, pos_in_icb, file_entry
            );
            pos_in_icb += file_entry.get_length() as u32;
            entries.push(file_entry);
        } else if tag.tag_identifier == TerminalEntry::TAG_IDENTIFIER {
            debug!("read_file_entries at {:?}: found terminal entry", address);
            break;
        } else if tag.tag_identifier == IndirectEntry::TAG_IDENTIFIER {
            let entry = IndirectEntry::read(&buf[..IndirectEntry::size()]);
            debug!("read_file_entries at {:?}: {:?}", address, entry);
            pos_in_icb += IndirectEntry::size() as u32;
            // TODO
            panic!("IndirectEntry not implemented");
        } else {
            error!(
                "read_file_entries at {:?} unknown tag identifier in information control block (ICB): {}",
                address,
                tag.tag_identifier
            );
            panic!("unknown tag identifier");
        }
    }
    debug!("read_file_entries: done");
    Ok(entries)
}

/// Given a FileEntry which is assumed to be from a directory,
/// reads the content of the file and parses the FileIdentifierDescriptors.
pub fn read_directory_contents<R: Read + Seek>(
    reader: &mut Cache<&mut R, DVDCSS_BLOCK_SIZE>,
    logical_volume_descriptor: &LogicalVolumeDescriptor,
    partition_descriptor: &PartitionDescriptor,
    file_entries: &[FileEntry],
) -> Result<Vec<FileIdentifierDescriptor>> {
    let mut file_identifiers = vec![];
    for file_entry in file_entries {
        let allocation_descriptors =
        read_short_allocation_descriptors(&*file_entry.allocation_descriptors);
        
        for ad in &allocation_descriptors {
            let pos_in_partition = short_ad_to_pos_in_partition(logical_volume_descriptor, ad);
            let mut buf: Vec<u8> = vec![0u8; ad.extent_length_bytes() as usize];
            if ad.extent_length_bytes() > 0 {
                read_exact_from_partition(
                    reader,
                    partition_descriptor,
                    pos_in_partition,
                    &mut buf,
                )?;
                file_identifiers.extend(parse_file_identifiers(&buf)?);
            }
        }
    }
    Ok(file_identifiers)
}

/// ECMA-167 4/8.6 Directories
/// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=77
pub fn parse_file_identifiers(mut buf: &[u8]) -> Result<Vec<FileIdentifierDescriptor>> {
    let mut entries = Vec::new();
    debug!("read_in_range: reading buf of size {}", buf.len());
    while buf.len() > DescriptorTag::size() {
        let tag = DescriptorTag::read(&buf);
        if tag.tag_identifier == 0 {
            debug!("read_in_range: found unrecorded logical block; breaking");
            break;
        }
        match tag.tag_identifier {
            FileIdentifierDescriptor::TAG_IDENTIFIER => {
                let file_identifier = FileIdentifierDescriptor::read(&buf);
                debug!("read_in_range: {:?}", file_identifier);
                // claude.ai says FileIdentifierDescriptor is aligned to 4 bytes
                // but can't find a citation from the spec.
                let size_aligned_4_byte = file_identifier.size() + 3 & !3;
                buf = &buf[size_aligned_4_byte..];
                entries.push(file_identifier);
            }
            TerminalEntry::TAG_IDENTIFIER => {
                debug!("read_in_range: found terminal entry");
                break;
            }
            _ => {
                error!(
                    "read_in_range: unknown tag identifier: {}",
                    tag.tag_identifier
                );
                return Err(UdfError::InvalidDescriptorTag);
            }
        }
    }
    debug!("read_in_range: remaining bytes: {:?}", buf);
    Ok(entries)
}

// Helper functions for working with OSTA compressed Unicode
pub mod osta {
    use clap::error;
    use log::error;

    /// Helper functions for working with OSTA compressed Unicode
    /// aka dstring
    /// see UncompressUnicode http://www.osta.org/specs/pdf/udf260.pdf#page=116
    pub fn decode(bytes: &[u8]) -> String {
        if bytes.is_empty() {
            return String::new();
        }

        let mut result = String::new();
        let compression_id = bytes[0];
        let mut i = 1; // Skip compression ID byte

        match compression_id {
            8 => {
                // 8-bit compression
                while i < bytes.len() {
                    if bytes[i] == 0 {
                        break;
                    }
                    result.push(bytes[i] as char);
                    i += 1;
                }
            }
            16 => {
                // 16-bit compression
                while i + 1 < bytes.len() {
                    let unicode = ((bytes[i] as u16) << 8) | (bytes[i + 1] as u16);
                    if unicode == 0 {
                        break;
                    }
                    if let Some(c) = char::from_u32(unicode as u32) {
                        result.push(c);
                    }
                    i += 2;
                }
            }
            _ => {
                error!(
                    "could not decode dstring: Unknown compression ID: {}",
                    compression_id
                );
            } // Unknown compression, return empty string
        }

        result
    }

    /// see CompressUnicode http://www.osta.org/specs/pdf/udf260.pdf#page=117
    pub fn encode(s: &str) -> Vec<u8> {
        let mut result = Vec::new();

        // Determine if we can use 8-bit compression
        let needs_16bit = s.chars().any(|c| c as u32 > 0xFF);
        let compression_id = if needs_16bit { 16 } else { 8 };

        // Place compression ID in first byte
        result.push(compression_id);

        match compression_id {
            8 => {
                // 8-bit compression
                for c in s.chars() {
                    result.push(c as u8);
                }
            }
            16 => {
                // 16-bit compression
                for c in s.chars() {
                    let unicode = c as u16;
                    result.push((unicode >> 8) as u8);
                    result.push((unicode & 0xFF) as u8);
                }
            }
            _ => unreachable!(),
        }

        // Add null terminator
        if compression_id == 8 {
            result.push(0);
        } else {
            result.push(0);
            result.push(0);
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use io::BufReader;

    use super::*;

    use std::io::Cursor;

    #[test]
    fn test_read_anchor() {
        // Create test data
        let mut data = vec![0u8; 512 * 257]; // Enough space for sector 256

        // Put an anchor volume descriptor at sector 256
        let mut anchor = AnchorVolumeDescriptorPointer::default();
        anchor.tag.tag_identifier = 2; // Anchor Volume Descriptor Pointer
        anchor.tag.descriptor_version = 2;
        anchor.write(&mut data[512 * 256..512 * 257]);

        let cursor = Cursor::new(data);
        let mut parser = UdfParser::new_with_sector_size(cursor, 512, 0);

        let result = parser.read_anchor();
        assert!(result.is_ok());

        let read_anchor = result.unwrap();
        assert_eq!(read_anchor.tag.tag_identifier, 2);
    }

    #[test]
    fn test_osta_unicode() {
        let input = "Hello, 世界!";
        let encoded = osta::encode(input);
        let decoded = osta::decode(&encoded);
        assert_eq!(input, decoded);
    }

    #[test]
    fn test_osta_ascii() {
        let input = "Hello, World!";
        let encoded = osta::encode(input);
        assert_eq!(encoded[0], 8); // Should use 8-bit compression
        let decoded = osta::decode(&encoded);
        assert_eq!(input, decoded);
    }

    #[test]
    fn test_osta_unicode_empty() {
        let input = "";
        let encoded = osta::encode(input);
        let decoded = osta::decode(&encoded);
        assert_eq!(input, decoded);
    }

    #[test]
    fn test_parse_file_identifiers() {
        let _ = env_logger::try_init();
        // copied from a DVD
        let bytes: Vec<u8> = vec![
            1, 1, 2, 0, 200, 0, 0, 0, 71, 98, 24, 0, 3, 0, 0, 0, 1, 0, 10, 0, 0, 8, 0, 0, 2, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 2, 0, 251, 0, 0, 0, 96, 116, 32, 0, 3, 0,
            0, 0, 1, 0, 2, 9, 0, 8, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 65, 85, 68,
            73, 79, 95, 84, 83, 0, 1, 1, 2, 0, 217, 0, 0, 0, 211, 223, 32, 0, 3, 0, 0, 0, 1, 0, 2,
            9, 0, 8, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 86, 73, 68, 69, 79, 95, 84,
            83, 0,
        ];
        // let parser = UdfPars÷÷÷÷÷er::new(Cursor::new(bytes));
        let result = parse_file_identifiers(&bytes).unwrap();
        assert_eq!(
            result
                .iter()
                .map(|entry| entry.file_identifier.to_string())
                .collect::<Vec<String>>(),
            vec!["", "AUDIO_TS", "VIDEO_TS"]
        );
    }
}

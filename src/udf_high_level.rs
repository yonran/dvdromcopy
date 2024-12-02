use log::debug;
use std::{
    collections::BTreeMap, fs::File, io::{Read, Seek}
};

use crate::{
    dvdcss_sys::DVDCSS_BLOCK_SIZE,
    udf::{
        DescriptorTag, LogicalVolumeDescriptor, PartitionDescriptor, PartitionMap, PrimaryVolumeDescriptor, TerminatingDescriptor, Timestamp
    },
    udf_parser::{Result, UdfError, UdfParser},
};

// ... [Previous error definitions remain the same]

#[derive(Debug)]
pub struct VolumeStructures {
    pub primary_volume: PrimaryVolumeDescriptor,
    pub logical_volume: LogicalVolumeDescriptor,
    pub partition_maps: Vec<PartitionMap>,
    /// mapping from partition number to partition descriptor
    pub partition_descriptors: BTreeMap<u16, PartitionDescriptor>,
}

// ... [Previous UdfParser implementation remains the same until read_logical_volume_descriptor]

impl<R: Read + Seek> UdfParser<R> {
    // ... [Previous methods remain the same]

    /// Read all volume structures starting from the anchor
    /// See UDF 2.6.0 6.9 Requirements for DVD-ROM http://www.osta.org/specs/pdf/udf260.pdf#page=136
    pub fn read_volume_structures(&mut self) -> Result<VolumeStructures> {
        debug!("read_volume_structures");
        // First, locate and read the anchor
        let anchor = self.read_anchor()?;
        debug!("read_volume_structures: anchor={:?}", anchor);

        // Read the main Volume Descriptor Sequence
        let structures = self.read_volume_descriptor_sequence(
            anchor
                .main_volume_descriptor_sequence_location
                .location_sector,
            anchor.main_volume_descriptor_sequence_location.length_bytes,
        )?;

        // If main sequence failed or was incomplete, try reserve sequence
        if structures.is_none() {
            return self
                .read_volume_descriptor_sequence(
                    anchor
                        .reserve_volume_descriptor_sequence_location
                        .location_sector,
                    anchor
                        .reserve_volume_descriptor_sequence_location
                        .length_bytes,
                )?
                .ok_or(UdfError::InvalidDescriptorTag);
        }

        structures.ok_or(UdfError::InvalidDescriptorTag)
    }

    /** Read a Volume Descriptor Sequence.
     * The Anchor Volume Descriptor points to one
     * Main Volume Descriptor Sequence (MVDS)
     * and a copy at Secondary Volume Descriptor Sequence (SVDS).
     *
     * A VDS contains a Partition Descriptor, Logical Volume Descriptor, etc.
     * see 8.4.1 Contents of a Volume Descriptor Sequence
     * https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=46
     */
    fn read_volume_descriptor_sequence(
        &mut self,
        start_location: u32,
        length: u32,
    ) -> Result<Option<VolumeStructures>> {
        let mut current_location = start_location;
        let end_location = start_location + length.div_ceil(self.sector_size);
        debug!(
            "read_volume_descriptor_sequence(start_location={:?} sector, length={:?} bytes)",
            start_location, length
        );

        let mut primary_volume = None;
        let mut logical_volume = None;
        let mut partition_maps = None;
        let mut partition_descriptors: BTreeMap<u16, PartitionDescriptor> = BTreeMap::new();

        // Read descriptors until we find a terminator or reach the end
        while current_location < end_location {
            // Read the tag to determine the descriptor type
            let mut tag_buf: Vec<u8> = vec![0u8; DescriptorTag::size().max(DVDCSS_BLOCK_SIZE)];
            self.seek_to_sector(current_location)?;
            self.reader.read_exact(&mut tag_buf)?;

            let tag: DescriptorTag = DescriptorTag::read(&tag_buf);

            match tag.tag_identifier {
                PrimaryVolumeDescriptor::TAG_IDENTIFIER => {
                    // Primary Volume Descriptor
                    primary_volume = Some(self.read_primary_volume_descriptor(current_location)?);
                }
                PartitionDescriptor::TAG_IDENTIFIER => {
                    // Partition Descriptor
                    let partition_descriptor = PartitionDescriptor::read(&tag_buf);
                    partition_descriptors.insert(partition_descriptor.partition_number, partition_descriptor);
                }
                LogicalVolumeDescriptor::TAG_IDENTIFIER => {
                    // Logical Volume Descriptor
                    let (lvd, maps) = self.read_logical_volume_descriptor(current_location)?;
                    if logical_volume.iter().all(|old_volume: &LogicalVolumeDescriptor| old_volume.volume_descriptor_sequence_number < lvd.volume_descriptor_sequence_number) {
                        logical_volume = Some(lvd);
                        partition_maps = Some(maps);
                    }
                }
                TerminatingDescriptor::TAG_IDENTIFIER => {
                    // Terminating Descriptor
                    break;
                }
                _ => { // Skip unknown descriptors
                     // No action needed
                }
            }

            current_location += 1;
        }

        // Return the structures only if we found all required descriptors
        if let (Some(pvd), Some(lvd), Some(maps)) = (primary_volume, logical_volume, partition_maps)
        {
            Ok(Some(VolumeStructures {
                primary_volume: pvd,
                logical_volume: lvd,
                partition_maps: maps,
                partition_descriptors,
            }))
        } else {
            Ok(None)
        }
    }
}

// Add a convenience method to get volume information
impl VolumeStructures {
    pub fn volume_info(&self) -> VolumeInfo {
        VolumeInfo {
            identifier: self.primary_volume.volume_identifier.to_string(),
            set_identifier: self.primary_volume.volume_set_identifier.to_string(),
            logical_volume_identifier: self.logical_volume.logical_volume_identifier.to_string(),
            logical_block_size: self.logical_volume.logical_block_size,
            recording_timestamp: self.primary_volume.recording_date_and_time.clone(),
            application_id: std::str::from_utf8(
                &self.primary_volume.application_identifier.identifier,
            )
            .unwrap_or("Unknown")
            .trim_end_matches('\0')
            .to_string(),
        }
    }
}

#[derive(Debug)]
pub struct VolumeInfo {
    pub identifier: String,
    pub set_identifier: String,
    pub logical_volume_identifier: String,
    pub logical_block_size: u32,
    pub recording_timestamp: Timestamp,
    pub application_id: String,
}

// Example usage in tests
#[cfg(test)]
mod tests {
    use crate::{udf::{AnchorVolumeDescriptorPointer, DescriptorTag, Type1PartitionMap}, udf_parser::osta};

    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_read_volume_structures() {
        // Create test data with minimal valid structures
        let mut data = vec![0u8; 512 * 1024]; // Enough space for all structures

        // Create an anchor at sector 256
        let mut anchor = AnchorVolumeDescriptorPointer::default();
        anchor.tag.tag_identifier = 2; // Anchor Volume Descriptor Pointer
        anchor.tag.descriptor_version = 2;
        anchor
            .main_volume_descriptor_sequence_location
            .location_sector = 512;
        anchor.main_volume_descriptor_sequence_location.length_bytes = 512 * 64;
        anchor.write(&mut data[512 * 256..512 * 257]);

        // Create a Primary Volume Descriptor
        let mut pvd = PrimaryVolumeDescriptor::default();
        pvd.tag.tag_identifier = 1;
        pvd.tag.descriptor_version = 2;
        osta::encode("TEST_VOLUME")
            .iter()
            .enumerate()
            .for_each(|(i, &b)| pvd.volume_identifier.0[i] = b);
        pvd.write(&mut data[512 * 512..]);

        // Create a Logical Volume Descriptor
        let mut lvd = LogicalVolumeDescriptor::default();
        lvd.tag.tag_identifier = 6;
        lvd.tag.descriptor_version = 2;
        lvd.logical_block_size = 2048;
        // Add a partition map
        lvd.number_of_partition_maps = 1;
        lvd.map_table_length = 6;
        lvd.write(&mut data[512 * 513..]);

        // Create a Type 1 Partition Map
        let mut pm = Type1PartitionMap::default();
        pm.header.partition_map_type = 1;
        pm.header.partition_map_length = 6;
        PartitionMap::Type1(pm)
            .write(&mut data[512 * 513 + 440..])
            .unwrap();

        // Create a terminating descriptor
        let mut term = DescriptorTag::default();
        term.tag_identifier = 8;
        term.write(&mut data[512 * 514..]);

        // Create parser and read structures
        let cursor = Cursor::new(data);
        let mut parser = UdfParser::new_with_sector_size(cursor, 512, 0);

        let result = parser.read_volume_structures();
        assert!(result.is_ok());

        let structures = result.unwrap();
        let info = structures.volume_info();

        assert_eq!(info.identifier, "TEST_VOLUME");
        assert_eq!(info.logical_block_size, 2048);
    }
}

// Example usage
fn main() -> Result<()> {
    let file = File::open("disk.iso")?;
    let mut parser = UdfParser::new_with_sector_size(file, 2048, 0);

    // This single call will read all necessary volume structures
    let volume_structures = parser.read_volume_structures()?;

    // Get human-readable volume information
    let volume_info = volume_structures.volume_info();

    println!("Volume Information:");
    println!("  Identifier: {}", volume_info.identifier);
    println!("  Set Identifier: {}", volume_info.set_identifier);
    println!(
        "  Logical Volume: {}",
        volume_info.logical_volume_identifier
    );
    println!("  Block Size: {}", volume_info.logical_block_size);
    println!("  Application: {}", volume_info.application_id);

    Ok(())
}

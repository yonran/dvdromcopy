use std::borrow::Cow;
use std::ffi::CStr;
use std::fmt::Debug;
use std::io::{self, Read, Seek};
use std::mem::offset_of;
use std::str;

use crc::Algorithm;
use static_assertions::assert_eq_size;

use crate::udf_parser::osta;

#[macro_export]
macro_rules! offsets_of {
    ($type:ty, $field:ident) => {{
        let start = std::mem::offset_of!($type, $field);
        let end = start + std::mem::size_of_val(unsafe { &std::mem::zeroed::<$type>().$field });
        start..end
    }};
}

#[derive(Clone)]
pub struct Dstring<const N: usize>(pub [u8; N]);
impl<const n: usize> Dstring<n> {
    pub fn from_str(s: &str) -> Self {
        let v = osta::encode(s);
        let mut x = [0; n];
        let min = v.len().min(n);
        x[..min].copy_from_slice(&v[..min]);
        Self(x)
    }
    pub fn to_string(&self) -> String {
        osta::decode(&self.0)
    }
}
impl<const N: usize> Default for Dstring<N> {
    fn default() -> Self {
        Self([0; N])
    }
}
impl<const N: usize> Debug for Dstring<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_string())
    }
}
#[derive(Clone, PartialEq)]
pub struct DynamicDstring(pub Vec<u8>);
impl DynamicDstring {
    pub fn from_str(s: &str) -> Self {
        let v = osta::encode(s);
        Self(v)
    }
    pub fn to_string(&self) -> String {
        osta::decode(&self.0)
    }
}
impl Default for DynamicDstring {
    fn default() -> Self {
        Self(Vec::new())
    }
}
impl Debug for DynamicDstring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_string())
    }
}

/// UDF 1.5.0 2.1.2 OSTA CS0 Charspec
#[derive(Clone, PartialEq)]
#[repr(C)]
pub struct CharSpec {
    /// should always be 0 in UDF
    pub character_set_type: u8,
    /// should always be “OSTA Compressed Unicode” in UDF padded with 0
    pub character_set_info: [u8; 63],
}
impl CharSpec {
    pub fn new() -> Self {
        Self {
            character_set_type: 0,
            // OSTA Compressed Unicode
            character_set_info: [
                0x4F, 0x53, 0x54, 0x41, 0x20, 0x43, 0x6F, 0x6D, 0x70, 0x72, 0x65, 0x73, 0x73, 0x65,
                0x64, 0x20, 0x55, 0x6E, 0x69, 0x63, 0x6F, 0x64, 0x65, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0,
            ],
        }
    }
    pub fn size() -> usize {
        std::mem::size_of::<CharSpec>()
    }
    pub fn read(bytes: &[u8]) -> Self {
        let mut r = Self::default();
        r.character_set_type = bytes[0];
        r.character_set_info.copy_from_slice(&bytes[1..64]);
        r
    }

    pub fn write(&self, bytes: &mut [u8]) {
        bytes[0] = self.character_set_type;
        bytes[1..64].copy_from_slice(&self.character_set_info);
    }
    pub fn is_osta_compressed_unicode(&self) -> bool {
        self.character_set_type == 0
            && &self.character_set_info[0..23] == b"OSTA Compressed Unicode"
    }
}
impl Default for CharSpec {
    fn default() -> Self {
        Self {
            character_set_type: 0,
            character_set_info: [0; 63],
        }
    }
}
impl Debug for CharSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_osta_compressed_unicode() {
            f.write_str("OSTA Compressed Unicode")
        } else {
            f.debug_struct("CharSpec")
                .field("character_set_type", &self.character_set_type)
                .field(
                    "character_set_info",
                    &CStr::from_bytes_with_nul(&self.character_set_info)
                        .map(|x| x.to_string_lossy())
                        .unwrap_or(Cow::Borrowed("")),
                )
                .finish()
        }
    }
}

/// UDF 2.1.4 Timestamp aka ISO 13346 1/7.3
#[derive(Default, Clone, PartialEq)]
#[repr(C)]
pub struct Timestamp {
    pub type_and_timezone: u16,
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub centiseconds: u8,
    pub hundreds_of_microseconds: u8,
    pub microseconds: u8,
}
assert_eq_size!(Timestamp, [u8; 12]);
impl Debug for Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("Timestamp {:04}-{:02}-{:02}", self.year, self.month, self.day))
    }
}
impl Timestamp {
    pub fn size() -> usize {
        std::mem::size_of::<Timestamp>()
    }
    pub fn read(bytes: &[u8]) -> Self {
        let mut r = Self::default();
        r.type_and_timezone = u16::from_le_bytes([bytes[0], bytes[1]]);
        r.year = u16::from_le_bytes([bytes[2], bytes[3]]);
        r.month = bytes[4];
        r.day = bytes[5];
        r.hour = bytes[6];
        r.minute = bytes[7];
        r.second = bytes[8];
        r.centiseconds = bytes[9];
        r.hundreds_of_microseconds = bytes[10];
        r.microseconds = bytes[11];
        r
    }

    pub fn write(&self, bytes: &mut [u8]) {
        bytes[0..2].copy_from_slice(&self.type_and_timezone.to_le_bytes());
        bytes[2..4].copy_from_slice(&self.year.to_le_bytes());
        bytes[4] = self.month;
        bytes[5] = self.day;
        bytes[6] = self.hour;
        bytes[7] = self.minute;
        bytes[8] = self.second;
        bytes[9] = self.centiseconds;
        bytes[10] = self.hundreds_of_microseconds;
        bytes[11] = self.microseconds;
    }
}

/// 2.1.5 Entity Identifier aka ISO 13346 1/7.4
/// http://www.osta.org/specs/pdf/udf150.pdf#page=17
#[derive(Clone, PartialEq)]
#[repr(C)]
pub struct EntityID {
    /// UDF 1.50: flags “Shall be set to ZERO.”
    pub flags: u8,
    pub identifier: [u8; 23],
    /// UDF parses this as Domain IdentifierSuffix
    pub identifier_suffix: [u8; 8],
}
impl Default for EntityID {
    fn default() -> Self {
        Self {
            flags: 0,
            identifier: [0; 23],
            identifier_suffix: [0; 8],
        }
    }
}
impl Debug for EntityID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EntityID")
            .field("flags", &self.flags)
            // I think this is an ascii cstr rather than a Dstring
            .field(
                "identifier",
                &CStr::from_bytes_until_nul(&self.identifier)
                    .map(|x| x.to_string_lossy())
                    .unwrap_or(Cow::Borrowed("")),
            )
            .field("identifier_suffix", &self.identifier_suffix)
            .finish()
    }
}
impl EntityID {
    pub fn size() -> usize {
        std::mem::size_of::<EntityID>()
    }
    pub fn read(bytes: &[u8]) -> Self {
        let mut r = Self::default();
        r.flags = bytes[0];
        r.identifier.copy_from_slice(&bytes[1..24]);
        r.identifier_suffix.copy_from_slice(&bytes[24..32]);
        r
    }
    pub fn write(&self, bytes: &mut [u8]) {
        bytes[0] = self.flags;
        bytes[1..24].copy_from_slice(&self.identifier);
        bytes[24..32].copy_from_slice(&self.identifier_suffix);
    }
}

pub struct IdentifierSuffix {
    // 0150 for UDF 1.50,
    udf_revision: u16,
}

/// DescriptorTag is the header of all UDF descriptors.
// http://www.osta.org/specs/pdf/udf150.pdf#page=22
// UDF Descriptor Tag aka ISO 13346 3/7.2
#[derive(Default, Debug, Clone, PartialEq)]
#[repr(C)]
pub struct DescriptorTag {
    pub tag_identifier: u16,
    pub descriptor_version: u16,
    /// “This field shall specify the sum modulo 256 of bytes 0-3 and 5-15 of the tag”
    /// ECMA-167 7.2.3 Tag Checksum
    /// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=43
    pub tag_checksum: u8,
    pub _reserved: u8,
    /// UDF 1.5.0: “Ignored. Intended for disaster recovery”
    pub tag_serial_number: u16,
    pub descriptor_crc: u16,
    /// UDF 1.5.0 2.2.1.2: “(Size of the Descriptor) - (Length of Descriptor Tag)”
    pub descriptor_crc_length: u16,
    pub tag_location: u32,
}
assert_eq_size!(DescriptorTag, [u8; 16]);
impl DescriptorTag {
    pub fn size() -> usize {
        std::mem::size_of::<DescriptorTag>()
    }
    pub fn read(bytes: &[u8]) -> Self {
        let mut r = Self::default();
        r.tag_identifier = u16::from_le_bytes([bytes[0], bytes[1]]);
        r.descriptor_version = u16::from_le_bytes([bytes[2], bytes[3]]);
        r.tag_checksum = bytes[4];
        r._reserved = bytes[5];
        r.tag_serial_number = u16::from_le_bytes([bytes[6], bytes[7]]);
        r.descriptor_crc = u16::from_le_bytes([bytes[8], bytes[9]]);
        r.descriptor_crc_length = u16::from_le_bytes([bytes[10], bytes[11]]);
        r.tag_location = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        r
    }

    pub fn write(&self, bytes: &mut [u8]) {
        bytes[0..2].copy_from_slice(&self.tag_identifier.to_le_bytes());
        bytes[2..4].copy_from_slice(&self.descriptor_version.to_le_bytes());
        bytes[4] = self.tag_checksum;
        bytes[5] = self._reserved;
        bytes[6..8].copy_from_slice(&self.tag_serial_number.to_le_bytes());
        bytes[8..10].copy_from_slice(&self.descriptor_crc.to_le_bytes());
        bytes[10..12].copy_from_slice(&self.descriptor_crc_length.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.tag_location.to_le_bytes());
    }
}

/// There is exactly one of these per volume.
/// The Anchor Volume Descriptor contains the
/// Main Volume Descriptor Sequence (MVDS) contains one or more Primary Volume Descriptors.
/// (ECMA-167 8.4.1 https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=46)
/// Reserve Volume Descriptor Sequence Extent
/// In ISO 9660, there is one of these at sector 16,
/// but UDF does not require it to exist there at all.
// UDF 1.5.0 2.2.2 http://www.osta.org/specs/pdf/udf150.pdf#page=22
#[derive(Clone, Debug)]
#[repr(C)]
pub struct PrimaryVolumeDescriptor {
    pub tag: DescriptorTag,
    pub volume_descriptor_sequence_number: u32,
    pub primary_volume_descriptor_number: u32,
    pub volume_identifier: Dstring<32>,
    pub volume_sequence_number: u16,
    pub maximum_volume_sequence_number: u16,
    pub interchange_level: u16,
    pub maximum_interchange_level: u16,
    pub character_set_list: u32,
    pub maximum_character_set_list: u32,
    pub volume_set_identifier: Dstring<128>,
    pub descriptor_character_set: CharSpec,
    pub explanatory_character_set: CharSpec,
    pub volume_abstract: ExtentAd,
    pub volume_copyright_notice: ExtentAd,
    pub application_identifier: EntityID,
    pub recording_date_and_time: Timestamp,
    pub implementation_identifier: EntityID,
    pub implementation_use: [u8; 64],
    pub predecessor_volume_descriptor_sequence_location: u32,
    pub flags: u16,
    pub reserved: [u8; 22],
}
impl Default for PrimaryVolumeDescriptor {
    fn default() -> Self {
        Self {
            tag: Default::default(),
            volume_descriptor_sequence_number: 0,
            primary_volume_descriptor_number: 0,
            volume_identifier: Dstring::default(),
            volume_sequence_number: 0,
            maximum_volume_sequence_number: 0,
            interchange_level: 0,
            maximum_interchange_level: 0,
            character_set_list: 0,
            maximum_character_set_list: 0,
            volume_set_identifier: Dstring::default(),
            descriptor_character_set: Default::default(),
            explanatory_character_set: Default::default(),
            volume_abstract: Default::default(),
            volume_copyright_notice: Default::default(),
            application_identifier: Default::default(),
            recording_date_and_time: Default::default(),
            implementation_identifier: Default::default(),
            implementation_use: [0; 64],
            predecessor_volume_descriptor_sequence_location: 0,
            flags: 0,
            reserved: [0; 22],
        }
    }
}

impl PrimaryVolumeDescriptor {
    pub const TAG_IDENTIFIER: u16 = 1;
    pub fn size() -> usize {
        std::mem::size_of::<PrimaryVolumeDescriptor>()
    }

    pub fn read(bytes: &[u8]) -> Self {
        let mut r = Self::default();
        r.tag = DescriptorTag::read(&bytes[0..16]);
        r.volume_descriptor_sequence_number =
            u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        r.primary_volume_descriptor_number =
            u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        r.volume_identifier.0.copy_from_slice(&bytes[24..56]);
        r.volume_sequence_number = u16::from_le_bytes([bytes[56], bytes[57]]);
        r.maximum_volume_sequence_number = u16::from_le_bytes([bytes[58], bytes[59]]);
        r.interchange_level = u16::from_le_bytes([bytes[60], bytes[61]]);
        r.maximum_interchange_level = u16::from_le_bytes([bytes[62], bytes[63]]);
        r.character_set_list = u32::from_le_bytes([bytes[64], bytes[65], bytes[66], bytes[67]]);
        r.maximum_character_set_list =
            u32::from_le_bytes([bytes[68], bytes[69], bytes[70], bytes[71]]);
        r.volume_set_identifier.0.copy_from_slice(&bytes[72..200]);
        r.descriptor_character_set = CharSpec::read(&bytes[200..264]);
        r.explanatory_character_set = CharSpec::read(&bytes[264..328]);
        r.volume_abstract = ExtentAd::read(&bytes[328..336]);
        r.volume_copyright_notice = ExtentAd::read(&bytes[336..344]);
        r.application_identifier = EntityID::read(&bytes[344..376]);
        r.recording_date_and_time = Timestamp::read(&bytes[376..388]);
        r.implementation_identifier = EntityID::read(&bytes[388..420]);
        r.implementation_use.copy_from_slice(&bytes[420..484]);
        r.predecessor_volume_descriptor_sequence_location =
            u32::from_le_bytes([bytes[484], bytes[485], bytes[486], bytes[487]]);
        r.flags = u16::from_le_bytes([bytes[488], bytes[489]]);
        r.reserved.copy_from_slice(&bytes[490..512]);

        r
    }

    pub fn write(&self, bytes: &mut [u8]) {
        self.tag.write(&mut bytes[0..16]);
        assert!(512 == std::mem::size_of::<PrimaryVolumeDescriptor>());
        bytes[16..20].copy_from_slice(&self.volume_descriptor_sequence_number.to_le_bytes());
        bytes[20..24].copy_from_slice(&self.primary_volume_descriptor_number.to_le_bytes());
        bytes[24..56].copy_from_slice(&self.volume_identifier.0);
        bytes[56..58].copy_from_slice(&self.volume_sequence_number.to_le_bytes());
        bytes[58..60].copy_from_slice(&self.maximum_volume_sequence_number.to_le_bytes());
        bytes[60..62].copy_from_slice(&self.interchange_level.to_le_bytes());
        bytes[62..64].copy_from_slice(&self.maximum_interchange_level.to_le_bytes());
        bytes[64..68].copy_from_slice(&self.character_set_list.to_le_bytes());
        bytes[68..72].copy_from_slice(&self.maximum_character_set_list.to_le_bytes());
        bytes[72..200].copy_from_slice(&self.volume_set_identifier.0);
        self.descriptor_character_set.write(&mut bytes[200..264]);
        self.explanatory_character_set.write(&mut bytes[264..328]);
        self.volume_abstract.write(&mut bytes[328..336]);
        self.volume_copyright_notice.write(&mut bytes[336..344]);
        self.application_identifier.write(&mut bytes[344..376]);
        self.recording_date_and_time.write(&mut bytes[376..388]);
        self.implementation_identifier.write(&mut bytes[388..420]);
        bytes[420..484].copy_from_slice(&self.implementation_use);
        bytes[484..488].copy_from_slice(
            &self
                .predecessor_volume_descriptor_sequence_location
                .to_le_bytes(),
        );
        bytes[488..490].copy_from_slice(&self.flags.to_le_bytes());
        bytes[490..512].copy_from_slice(&self.reserved);
    }
}

/// ECMA-167 7.1 Extent Descriptor aka extent_ad
/// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=42
#[derive(Default, Debug, Clone)]
#[repr(C)]
pub struct ExtentAd {
    /// length in bytes
    pub length_bytes: u32,
    /// location in logical sector number, or 0 if length is 0
    pub location_sector: u32,
}
impl ExtentAd {
    pub fn size() -> usize {
        std::mem::size_of::<ExtentAd>()
    }
    pub fn read(bytes: &[u8]) -> Self {
        let mut r = Self::default();
        r.length_bytes = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        r.location_sector = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        r
    }
    pub fn write(&self, bytes: &mut [u8]) {
        bytes[0..4].copy_from_slice(&self.length_bytes.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.location_sector.to_le_bytes());
    }
}

/// UDF Anchor Volume Descriptor Pointer aka ISO 13346 3/10.2
/// This shall be recorded in at least 2 of:
/// 1) Logical Sector 256, 2) Logical Sector (N - 256), 3) N
#[derive(Debug, Clone)]
#[repr(C)]
pub struct AnchorVolumeDescriptorPointer {
    pub tag: DescriptorTag,
    /// Location of the Main Volume Descriptor Sequence (MVDS)
    /// main_volume_descriptor_sequence_location.extent_length >= 16
    pub main_volume_descriptor_sequence_location: ExtentAd,
    /// reserve_volume_descriptor_sequence_location.extent_length >= 16
    pub reserve_volume_descriptor_sequence_location: ExtentAd,
    pub reserved: [u8; 480],
}
impl Default for AnchorVolumeDescriptorPointer {
    fn default() -> Self {
        Self {
            tag: Default::default(),
            main_volume_descriptor_sequence_location: Default::default(),
            reserve_volume_descriptor_sequence_location: Default::default(),
            reserved: [0; 480],
        }
    }
}
impl AnchorVolumeDescriptorPointer {
    pub fn size() -> usize {
        std::mem::size_of::<AnchorVolumeDescriptorPointer>()
    }
    pub fn read(bytes: &[u8]) -> Self {
        let mut r = Self::default();
        r.tag = DescriptorTag::read(&bytes[0..16]);
        r.main_volume_descriptor_sequence_location = ExtentAd::read(&bytes[16..24]);
        r.reserve_volume_descriptor_sequence_location = ExtentAd::read(&bytes[24..32]);
        r.reserved.copy_from_slice(&bytes[32..512]);
        r
    }
    pub fn write(&self, bytes: &mut [u8]) {
        self.tag.write(&mut bytes[0..16]);
        self.main_volume_descriptor_sequence_location
            .write(&mut bytes[16..24]);
        self.reserve_volume_descriptor_sequence_location
            .write(&mut bytes[24..32]);
        bytes[32..512].copy_from_slice(&self.reserved);
    }
}

// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=60
#[derive(Default, Debug, Clone)]
#[repr(C)]
pub struct GenericPartitionMapHeader {
    pub partition_map_type: u8,
    pub partition_map_length: u8,
}

#[derive(Debug, Clone)]
pub enum PartitionMap {
    Type1(Type1PartitionMap),
    Type2(Type2PartitionMap),
    Other {
        header: GenericPartitionMapHeader,
        data: Vec<u8>,
    },
}

/// see ECMA-167 10.7.2 Type 1 Partition Map
/// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=60
#[derive(Default, Debug, Clone)]
pub struct Type1PartitionMap {
    pub header: GenericPartitionMapHeader,
    /// volume upon which the VAT and Partition is recorded
    /// UDF 2.6.0 2.2.8 http://www.osta.org/specs/pdf/udf260.pdf
    /// typically just 1 for single-volume DVD
    pub volume_seq_number: u16,
    pub partition_number: u16,
}

#[derive(Debug, Clone)]
pub struct Type2PartitionMap {
    pub header: GenericPartitionMapHeader,
    pub reserved1: [u8; 2],
    pub partition_type_identifier: [u8; 32],
    // TODO: complete
}
impl Default for Type2PartitionMap {
    fn default() -> Self {
        Self {
            header: GenericPartitionMapHeader::default(),
            reserved1: [0; 2],
            partition_type_identifier: [0; 32],
        }
    }
}

impl PartitionMap {
    pub fn read(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() < 2 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Buffer too small",
            ));
        }

        let map_type = bytes[0];
        let map_length = bytes[1];

        if bytes.len() < map_length as usize {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Buffer too small for partition map",
            ));
        }

        let header = GenericPartitionMapHeader {
            partition_map_type: map_type,
            partition_map_length: map_length,
        };

        match map_type {
            1 => {
                if map_length != 6 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Invalid Type 1 partition map length",
                    ));
                }
                Ok(PartitionMap::Type1(Type1PartitionMap {
                    header,
                    volume_seq_number: u16::from_le_bytes([bytes[2], bytes[3]]),
                    partition_number: u16::from_le_bytes([bytes[4], bytes[5]]),
                }))
            }
            2 => {
                if map_length != 64 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Invalid Type 2 partition map length",
                    ));
                }
                let mut reserved1 = [0u8; 2];
                reserved1.copy_from_slice(&bytes[2..4]);
                let mut partition_type_identifier = [0u8; 32];
                partition_type_identifier.copy_from_slice(&bytes[4..36]);

                Ok(PartitionMap::Type2(Type2PartitionMap {
                    header,
                    reserved1,
                    partition_type_identifier,
                }))
            }
            // Handle other partition map types by storing their raw data
            _ => {
                let mut data = vec![0u8; map_length as usize];
                data.copy_from_slice(&bytes[..map_length as usize]);
                Ok(PartitionMap::Other { header, data })
            }
        }
    }

    pub fn write(&self, bytes: &mut [u8]) -> io::Result<()> {
        match self {
            PartitionMap::Type1(map) => {
                if bytes.len() < 6 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Buffer too small",
                    ));
                }
                bytes[0] = map.header.partition_map_type;
                bytes[1] = map.header.partition_map_length;
                bytes[2..4].copy_from_slice(&map.volume_seq_number.to_le_bytes());
                bytes[4..6].copy_from_slice(&map.partition_number.to_le_bytes());
            }
            PartitionMap::Type2(map) => {
                if bytes.len() < 64 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Buffer too small",
                    ));
                }
                bytes[0] = map.header.partition_map_type;
                bytes[1] = map.header.partition_map_length;
                bytes[2..64].copy_from_slice(&map.partition_type_identifier);
            }
            PartitionMap::Other { header: _, data } => {
                if bytes.len() < data.len() {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Buffer too small",
                    ));
                }
                bytes[..data.len()].copy_from_slice(data);
            }
        }
        Ok(())
    }

    pub fn get_length(&self) -> u8 {
        match self {
            PartitionMap::Type1(_) => 6,
            PartitionMap::Type2(_) => 64,
            PartitionMap::Other { header, .. } => header.partition_map_length,
        }
    }
}

/// UDF Logical Volume Descriptor aka ISO 13346 3/10.6
///
#[derive(Clone, Debug)]
#[repr(C)]
pub struct LogicalVolumeDescriptor {
    pub tag: DescriptorTag,
    pub volume_descriptor_sequence_number: u32,
    pub descriptor_character_set: CharSpec,
    pub logical_volume_identifier: Dstring<128>,
    pub logical_block_size: u32,
    pub domain_identifier: EntityID,
    /// this field is a Logical Volume Header Descriptor in UDF 2.6.0
    /// http://www.osta.org/specs/pdf/udf260.pdf#page=70
    pub logical_volume_contents_use: [u8; 16],
    pub map_table_length: u32,
    pub number_of_partition_maps: u32,
    pub implementation_identifier: EntityID,
    pub implementation_use: [u8; 128],
    /// points to Logical Volume Integrity Descriptor
    pub integrity_sequence_extent: ExtentAd,
    pub partition_maps: [u8; 0],
}
assert_eq_size!(LogicalVolumeDescriptor, [u8; 440]);
impl Default for LogicalVolumeDescriptor {
    fn default() -> Self {
        Self {
            tag: Default::default(),
            volume_descriptor_sequence_number: 0,
            descriptor_character_set: Default::default(),
            logical_volume_identifier: Dstring::default(),
            logical_block_size: 0,
            domain_identifier: Default::default(),
            logical_volume_contents_use: [0; 16],
            map_table_length: 0,
            number_of_partition_maps: 0,
            implementation_identifier: Default::default(),
            implementation_use: [0; 128],
            integrity_sequence_extent: Default::default(),
            partition_maps: [0; 0],
        }
    }
}
impl LogicalVolumeDescriptor {
    pub const TAG_IDENTIFIER: u16 = 6;
    pub fn size() -> usize {
        std::mem::size_of::<LogicalVolumeDescriptor>()
    }
    pub fn read(bytes: &[u8]) -> Self {
        assert_eq!(440, std::mem::size_of::<LogicalVolumeDescriptor>());
        assert_eq!(bytes.len(), 440);
        let mut r = Self::default();
        r.tag = DescriptorTag::read(&bytes[0..16]);
        r.volume_descriptor_sequence_number =
            u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        r.descriptor_character_set = CharSpec::read(&bytes[20..84]);
        r.logical_volume_identifier
            .0
            .copy_from_slice(&bytes[84..212]);
        r.logical_block_size = u32::from_le_bytes([bytes[212], bytes[213], bytes[214], bytes[215]]);
        r.domain_identifier = EntityID::read(&bytes[216..248]);
        r.logical_volume_contents_use
            .copy_from_slice(&bytes[248..264]);
        r.map_table_length = u32::from_le_bytes([bytes[264], bytes[265], bytes[266], bytes[267]]);
        r.number_of_partition_maps =
            u32::from_le_bytes([bytes[268], bytes[269], bytes[270], bytes[271]]);
        r.implementation_identifier = EntityID::read(&bytes[272..304]);
        r.implementation_use.copy_from_slice(&bytes[304..432]);
        r.integrity_sequence_extent = ExtentAd::read(&bytes[432..440]);
        // r.partition_maps.copy_from_slice(&bytes[440..]);
        r
    }
    pub fn write(&self, bytes: &mut [u8]) {
        assert_eq!(440, std::mem::size_of::<LogicalVolumeDescriptor>());
        self.tag.write(&mut bytes[0..16]);
        bytes[16..20].copy_from_slice(&self.volume_descriptor_sequence_number.to_le_bytes());
        self.descriptor_character_set.write(&mut bytes[20..84]);
        bytes[84..212].copy_from_slice(&self.logical_volume_identifier.0);
        bytes[212..216].copy_from_slice(&self.logical_block_size.to_le_bytes());
        self.domain_identifier.write(&mut bytes[216..248]);
        bytes[248..264].copy_from_slice(&self.logical_volume_contents_use);
        bytes[264..268].copy_from_slice(&self.map_table_length.to_le_bytes());
        bytes[268..272].copy_from_slice(&self.number_of_partition_maps.to_le_bytes());
        self.implementation_identifier.write(&mut bytes[272..304]);
        bytes[304..432].copy_from_slice(&self.implementation_use);
        self.integrity_sequence_extent.write(&mut bytes[432..440]);
        // bytes[440..].copy_from_slice(&self.partition_maps);
    }
    pub fn read_partition_maps<R: Read + Seek>(
        &self,
        reader: &mut R,
    ) -> io::Result<Vec<PartitionMap>> {
        let mut maps = Vec::new();
        let remaining_bytes = self.map_table_length;
        let mut partition_map_buf = vec![0u8; remaining_bytes as usize];
        reader.read_exact(&mut partition_map_buf)?;

        let mut offset = 0;
        while offset < remaining_bytes as usize {
            let map = PartitionMap::read(&partition_map_buf[offset..])?;
            offset += map.get_length() as usize;
            maps.push(map);
        }

        Ok(maps)
    }
}

/// UDF 2.6.0 2.2.14 http://www.osta.org/specs/pdf/udf260.pdf#page=51
/// aka ECMA 167 3/10.5
#[derive(Debug, Clone)]
#[repr(C)]
pub struct PartitionDescriptor {
    pub tag: DescriptorTag,
    pub volume_descriptor_sequence_number: u32,
    pub partition_flags: u16,
    pub partition_number: u16,
    pub partition_contents: EntityID,
    pub partition_contents_use: [u8; 128],
    pub access_type: u32,
    /// position of the partition in sector (2048 byte for DVD)
    pub partition_starting_location: u32,
    /// length in blocks
    pub partition_length: u32,
    pub implementation_identifier: EntityID,
    pub implementation_use: [u8; 128],
    pub reserved: [u8; 156],
}
assert_eq_size!(PartitionDescriptor, [u8; 512]);
impl Default for PartitionDescriptor {
    fn default() -> Self {
        Self {
            tag: Default::default(),
            volume_descriptor_sequence_number: Default::default(),
            partition_flags: Default::default(),
            partition_number: Default::default(),
            partition_contents: Default::default(),
            partition_contents_use: [0; 128],
            access_type: Default::default(),
            partition_starting_location: Default::default(),
            partition_length: Default::default(),
            implementation_identifier: Default::default(),
            implementation_use: [0; 128],
            reserved: [0; 156],
        }
    }
}

impl PartitionDescriptor {
    pub const TAG_IDENTIFIER: u16 = 5;
    pub fn size() -> usize {
        std::mem::size_of::<PartitionDescriptor>()
    }

    pub fn read(bytes: &[u8]) -> Self {
        let mut r = Self::default();
        r.tag = DescriptorTag::read(&bytes[0..16]);
        r.volume_descriptor_sequence_number =
            u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        r.partition_flags = u16::from_le_bytes([bytes[20], bytes[21]]);
        r.partition_number = u16::from_le_bytes([bytes[22], bytes[23]]);
        r.partition_contents = EntityID::read(&bytes[24..56]);
        r.partition_contents_use.copy_from_slice(&bytes[56..184]);
        r.access_type = u32::from_le_bytes([bytes[184], bytes[185], bytes[186], bytes[187]]);
        r.partition_starting_location =
            u32::from_le_bytes([bytes[188], bytes[189], bytes[190], bytes[191]]);
        r.partition_length = u32::from_le_bytes([bytes[192], bytes[193], bytes[194], bytes[195]]);
        r.implementation_identifier = EntityID::read(&bytes[196..228]);
        r.implementation_use.copy_from_slice(&bytes[228..356]);
        r.reserved.copy_from_slice(&bytes[356..512]);
        r
    }

    pub fn write(&self, bytes: &mut [u8]) {
        self.tag.write(&mut bytes[0..16]);
        bytes[16..20].copy_from_slice(&self.volume_descriptor_sequence_number.to_le_bytes());
        bytes[20..22].copy_from_slice(&self.partition_flags.to_le_bytes());
        bytes[22..24].copy_from_slice(&self.partition_number.to_le_bytes());
        self.partition_contents.write(&mut bytes[24..56]);
        bytes[56..184].copy_from_slice(&self.partition_contents_use);
        bytes[184..188].copy_from_slice(&self.access_type.to_le_bytes());
        bytes[188..192].copy_from_slice(&self.partition_starting_location.to_le_bytes());
        bytes[192..196].copy_from_slice(&self.partition_length.to_le_bytes());
        self.implementation_identifier.write(&mut bytes[196..228]);
        bytes[228..356].copy_from_slice(&self.implementation_use);
        bytes[356..512].copy_from_slice(&self.reserved);
    }
}


/// ECMA-167 7.1 Recorded address aka lb_addr
/// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=73
#[derive(Default, Debug, Clone, PartialEq, Copy)]
#[repr(C, packed)]
pub struct LbAddr {
    pub logical_block_number: u32,
    pub partition_reference_number: u16,
}
assert_eq_size!(LbAddr, [u8; 6]);

impl LbAddr {
    pub fn size() -> usize {
        std::mem::size_of::<LbAddr>()
    }

    pub fn read(bytes: &[u8]) -> Self {
        let mut r = Self::default();
        r.logical_block_number = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        r.partition_reference_number = u16::from_le_bytes([bytes[4], bytes[5]]);
        r
    }

    pub fn write(&self, bytes: &mut [u8]) {
        bytes[0..4].copy_from_slice(&self.logical_block_number.to_le_bytes());
        bytes[4..6].copy_from_slice(&self.partition_reference_number.to_le_bytes());
    }
}

/// UDF 2.3.2 File Set Descriptor aka ECMA 167 4/14.1
/// http://www.osta.org/specs/pdf/udf260.pdf#page=54
#[derive(Debug, Clone)]
#[repr(C)]
pub struct FileSetDescriptor {
    pub tag: DescriptorTag,
    pub recording_date_and_time: Timestamp,
    pub interchange_level: u16,
    pub maximum_interchange_level: u16,
    pub character_set_list: u32,
    pub maximum_character_set_list: u32,
    pub file_set_number: u32,
    pub file_set_descriptor_number: u32,
    pub logical_volume_identifier_character_set: CharSpec,
    pub logical_volume_identifier: Dstring<128>,
    pub file_set_character_set: CharSpec,
    pub file_set_identifier: Dstring<32>,
    pub copyright_file_identifier: Dstring<32>,
    pub abstract_file_identifier: Dstring<32>,
    pub root_directory_icb: LongAd,
    pub domain_identifier: EntityID,
    pub next_extent: LongAd,
    pub system_stream_directory_icb: LongAd,
    pub reserved: [u8; 32],
}

impl Default for FileSetDescriptor {
    fn default() -> Self {
        Self {
            tag: Default::default(),
            recording_date_and_time: Default::default(),
            interchange_level: 0,
            maximum_interchange_level: 0,
            character_set_list: 0,
            maximum_character_set_list: 0,
            file_set_number: 0,
            file_set_descriptor_number: 0,
            logical_volume_identifier_character_set: Default::default(),
            logical_volume_identifier: Dstring::default(),
            file_set_character_set: Default::default(),
            file_set_identifier: Dstring::default(),
            copyright_file_identifier: Dstring::default(),
            abstract_file_identifier: Dstring::default(),
            root_directory_icb: Default::default(),
            domain_identifier: Default::default(),
            next_extent: Default::default(),
            system_stream_directory_icb: Default::default(),
            reserved: [0; 32],
        }
    }
}

impl FileSetDescriptor {
    pub const TAG_IDENTIFIER: u16 = 256;
    pub fn size() -> usize {
        std::mem::size_of::<FileSetDescriptor>()
    }

    pub fn read(bytes: &[u8]) -> Self {
        let mut r = Self::default();
        r.tag = DescriptorTag::read(&bytes[0..16]);
        r.recording_date_and_time = Timestamp::read(&bytes[16..28]);
        r.interchange_level = u16::from_le_bytes([bytes[28], bytes[29]]);
        r.maximum_interchange_level = u16::from_le_bytes([bytes[30], bytes[31]]);
        r.character_set_list = u32::from_le_bytes([bytes[32], bytes[33], bytes[34], bytes[35]]);
        r.maximum_character_set_list =
            u32::from_le_bytes([bytes[36], bytes[37], bytes[38], bytes[39]]);
        r.file_set_number = u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]);
        r.file_set_descriptor_number =
            u32::from_le_bytes([bytes[44], bytes[45], bytes[46], bytes[47]]);
        r.logical_volume_identifier_character_set = CharSpec::read(&bytes[48..112]);
        r.logical_volume_identifier
            .0
            .copy_from_slice(&bytes[112..240]);
        r.file_set_character_set = CharSpec::read(&bytes[240..304]);
        r.file_set_identifier.0.copy_from_slice(&bytes[304..336]);
        r.copyright_file_identifier
            .0
            .copy_from_slice(&bytes[336..368]);
        r.abstract_file_identifier
            .0
            .copy_from_slice(&bytes[368..400]);
        r.root_directory_icb = LongAd::read(&bytes[400..416]);
        r.domain_identifier = EntityID::read(&bytes[416..448]);
        r.next_extent = LongAd::read(&bytes[448..464]);
        r.system_stream_directory_icb = LongAd::read(&bytes[464..480]);
        r.reserved.copy_from_slice(&bytes[480..512]);
        r
    }

    pub fn write(&self, bytes: &mut [u8]) {
        self.tag.write(&mut bytes[0..16]);
        self.recording_date_and_time.write(&mut bytes[16..28]);
        bytes[28..30].copy_from_slice(&self.interchange_level.to_le_bytes());
        bytes[30..32].copy_from_slice(&self.maximum_interchange_level.to_le_bytes());
        bytes[32..36].copy_from_slice(&self.character_set_list.to_le_bytes());
        bytes[36..40].copy_from_slice(&self.maximum_character_set_list.to_le_bytes());
        bytes[40..44].copy_from_slice(&self.file_set_number.to_le_bytes());
        bytes[44..48].copy_from_slice(&self.file_set_descriptor_number.to_le_bytes());
        self.logical_volume_identifier_character_set
            .write(&mut bytes[48..112]);
        bytes[112..240].copy_from_slice(&self.logical_volume_identifier.0);
        self.file_set_character_set.write(&mut bytes[240..304]);
        bytes[304..336].copy_from_slice(&self.file_set_identifier.0);
        bytes[336..368].copy_from_slice(&self.copyright_file_identifier.0);
        bytes[368..400].copy_from_slice(&self.abstract_file_identifier.0);
        self.root_directory_icb.write(&mut bytes[400..416]);
        self.domain_identifier.write(&mut bytes[416..448]);
        self.next_extent.write(&mut bytes[448..464]);
        self.system_stream_directory_icb.write(&mut bytes[464..480]);
        bytes[480..512].copy_from_slice(&self.reserved);
    }
}

#[derive(Debug, Clone)]
pub struct TerminatingDescriptor {
    /// tag identifier must be 8
    pub tag: DescriptorTag,
    pub reserved: [u8; 496],
}
assert_eq_size!(TerminatingDescriptor, [u8; 512]);
impl Default for TerminatingDescriptor {
    fn default() -> Self {
        Self {
            tag: DescriptorTag::default(),
            reserved: [0; 496],
        }
    }
}
impl TerminatingDescriptor {
    pub const TAG_IDENTIFIER: u16 = 8;
    pub fn size() -> usize {
        std::mem::size_of::<TerminatingDescriptor>()
    }
    pub fn read(bytes: &[u8]) -> Self {
        assert_eq!(bytes.len(), 512);
        let mut r = Self::default();
        r.tag = DescriptorTag::read(&bytes[0..16]);
        r.reserved.copy_from_slice(&bytes[16..512]);
        r
    }
    pub fn write(&self, bytes: &mut [u8]) {
        self.tag.write(&mut bytes[0..16]);
        bytes[16..512].copy_from_slice(&self.reserved);
    }
}

/// File Entry is like an inode in Unix; it has permissions, timestamps,
/// and pointers to data blocks.
/// If it is a directory (icb_tag.flags & ), then 
/// ECMA-167 4/14.9 File Entry
/// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=98
#[derive(Debug, Clone)]
#[repr(C)]
pub struct FileEntry {
    pub tag: DescriptorTag,
    pub icb_tag: ICBTag,
    pub uid: u32,
    pub gid: u32,
    pub permissions: u32,
    pub file_link_count: u16,
    pub record_format: u8,
    pub record_display_attributes: u8,
    pub record_length: u32,
    pub information_length: u64,
    pub logical_blocks_recorded: u64,
    pub access_time: Timestamp,
    pub modification_time: Timestamp,
    pub attribute_time: Timestamp,
    pub checkpoint: u32,
    pub extended_attribute_icb: LongAd,
    pub implementation_identifier: EntityID,
    pub unique_id: u64,
    pub length_of_extended_attributes: u32,
    pub length_of_allocation_descriptors: u32,
    pub extended_attributes: Vec<u8>,
    /// “This field shall be a sequence of allocation descriptors
    /// recorded as specified in 4/12.1.
    /// Any such allocation descriptor which is specified as unrecorded and
    /// unallocated (see 4/14.14.1.1) shall have its Extent Location field set to 0.”
    /// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=102
    /// UDF constraint: “Only Short Allocation Descriptors shall be used.”
    /// http://www.osta.org/specs/pdf/udf260.pdf#page=64
    pub allocation_descriptors: Vec<u8>,
}

impl Default for FileEntry {
    fn default() -> Self {
        Self {
            tag: Default::default(),
            icb_tag: Default::default(),
            uid: 0,
            gid: 0,
            permissions: 0,
            file_link_count: 0,
            record_format: 0,
            record_display_attributes: 0,
            record_length: 0,
            information_length: 0,
            logical_blocks_recorded: 0,
            access_time: Default::default(),
            modification_time: Default::default(),
            attribute_time: Default::default(),
            checkpoint: 0,
            extended_attribute_icb: Default::default(),
            implementation_identifier: Default::default(),
            unique_id: 0,
            length_of_extended_attributes: 0,
            length_of_allocation_descriptors: 0,
            extended_attributes: Vec::new(),
            allocation_descriptors: Vec::new(),
        }
    }
}
/// ECMA 167 4/14.9
/// UDF 2.60 2.3.6 http://www.osta.org/specs/pdf/udf260.pdf#page=62
impl FileEntry {
    /// ECMA-167 4/7.2.1 Tag Identifier (RBP 0)
    /// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=74
    pub const TAG_IDENTIFIER: u16 = 261;
    pub fn get_length(&self) -> usize {
        176 + self.length_of_extended_attributes as usize + self.length_of_allocation_descriptors as usize
    }
    pub fn read(bytes: &[u8]) -> Self {
        let mut r = Self::default();
        r.tag = DescriptorTag::read(&bytes[0..16]);
        r.icb_tag = ICBTag::read(&bytes[16..36]);
        r.uid = u32::from_le_bytes([bytes[36], bytes[37], bytes[38], bytes[39]]);
        r.gid = u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]);
        r.permissions = u32::from_le_bytes([bytes[44], bytes[45], bytes[46], bytes[47]]);
        r.file_link_count = u16::from_le_bytes([bytes[48], bytes[49]]);
        r.record_format = bytes[50];
        r.record_display_attributes = bytes[51];
        r.record_length = u32::from_le_bytes([bytes[52], bytes[53], bytes[54], bytes[55]]);
        r.information_length = u64::from_le_bytes([
            bytes[56], bytes[57], bytes[58], bytes[59], bytes[60], bytes[61], bytes[62], bytes[63],
        ]);
        r.logical_blocks_recorded = u64::from_le_bytes([
            bytes[64], bytes[65], bytes[66], bytes[67], bytes[68], bytes[69], bytes[70], bytes[71],
        ]);
        r.access_time = Timestamp::read(&bytes[72..84]);
        r.modification_time = Timestamp::read(&bytes[84..96]);
        r.attribute_time = Timestamp::read(&bytes[96..108]);
        r.checkpoint = u32::from_le_bytes([bytes[108], bytes[109], bytes[110], bytes[111]]);
        r.extended_attribute_icb = LongAd::read(&bytes[112..128]);
        r.implementation_identifier = EntityID::read(&bytes[128..160]);
        r.unique_id = u64::from_le_bytes([
            bytes[160], bytes[161], bytes[162], bytes[163], bytes[164], bytes[165], bytes[166],
            bytes[167],
        ]);
        r.length_of_extended_attributes =
            u32::from_le_bytes([bytes[168], bytes[169], bytes[170], bytes[171]]);
        r.length_of_allocation_descriptors =
            u32::from_le_bytes([bytes[172], bytes[173], bytes[174], bytes[175]]);
        r.extended_attributes =
            bytes[176..(176 + r.length_of_extended_attributes as usize)].to_vec();
        r.allocation_descriptors = bytes[(176 + r.length_of_extended_attributes as usize)
            ..176
                + (r.length_of_extended_attributes as usize)
                + (r.length_of_allocation_descriptors as usize)]
            .to_vec();
        r
    }

    pub fn write(&self, bytes: &mut [u8]) {
        self.tag.write(&mut bytes[0..16]);
        self.icb_tag.write(&mut bytes[16..36]);
        bytes[36..40].copy_from_slice(&self.uid.to_le_bytes());
        bytes[40..44].copy_from_slice(&self.gid.to_le_bytes());
        bytes[44..48].copy_from_slice(&self.permissions.to_le_bytes());
        bytes[48..50].copy_from_slice(&self.file_link_count.to_le_bytes());
        bytes[50] = self.record_format;
        bytes[51] = self.record_display_attributes;
        bytes[52..56].copy_from_slice(&self.record_length.to_le_bytes());
        bytes[56..64].copy_from_slice(&self.information_length.to_le_bytes());
        bytes[64..72].copy_from_slice(&self.logical_blocks_recorded.to_le_bytes());
        self.access_time.write(&mut bytes[72..84]);
        self.modification_time.write(&mut bytes[84..96]);
        self.attribute_time.write(&mut bytes[96..108]);
        bytes[108..112].copy_from_slice(&self.checkpoint.to_le_bytes());
        self.extended_attribute_icb.write(&mut bytes[112..128]);
        self.implementation_identifier.write(&mut bytes[128..160]);
        bytes[160..168].copy_from_slice(&self.unique_id.to_le_bytes());
        bytes[168..172].copy_from_slice(&self.length_of_extended_attributes.to_le_bytes());
        bytes[172..176].copy_from_slice(&self.length_of_allocation_descriptors.to_le_bytes());
        bytes[176..(176 + self.length_of_extended_attributes as usize)]
            .copy_from_slice(&self.extended_attributes);
        bytes[(176 + self.length_of_extended_attributes as usize)..]
            .copy_from_slice(&self.allocation_descriptors);
    }
}

pub struct TerminalEntry {
    tag: DescriptorTag,
    icb_tag: ICBTag,
}
assert_eq_size!(TerminalEntry, [u8; 36]);
impl Default for TerminalEntry {
    fn default() -> Self {
        Self {
            tag: Default::default(),
            icb_tag: Default::default(),
        }
    }
}
impl TerminalEntry {
    /// ECMA-167 4/7.2.1 Tag Identifier (RBP 0)
    /// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=74
    pub const TAG_IDENTIFIER: u16 = 260;
    pub fn size() -> usize {
        std::mem::size_of::<TerminalEntry>()
    }
    pub fn read(bytes: &[u8]) -> Self {
        let mut r = Self::default();
        r.tag = DescriptorTag::read(&bytes[0..16]);
        r.icb_tag = ICBTag::read(&bytes[16..36]);
        r
    }
    pub fn write(&self, bytes: &mut [u8]) {
        self.tag.write(&mut bytes[0..16]);
        self.icb_tag.write(&mut bytes[16..36]);
    }
}

/// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=96
#[repr(u8)]
pub enum AllocationDescriptorType {
    SHORT = 0,
    LONG = 1,
    EXTENDED = 2,
    ONE = 3,
}
impl From<u8> for AllocationDescriptorType {
    fn from(v: u8) -> AllocationDescriptorType {
        match v {
            0 => AllocationDescriptorType::SHORT,
            1 => AllocationDescriptorType::LONG,
            2 => AllocationDescriptorType::EXTENDED,
            3 => AllocationDescriptorType::ONE,
            _ => panic!("Invalid AllocationDescriptorType"),
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub enum FileType {
    /// 0 Shall mean that the interpretation of the file is not specified by this field
    Unspecified = 0,
    /// 1 Shall mean that this is an Unallocated Space Entry (see 4/14.11)
    Unallocated = 1,
    /// 2 Shall mean that this is a Partition Integrity Entry (see 4/14.13)
    PartitionIntegrity = 2,
    /// 3 Shall mean that this is an Indirect Entry (see 4/14.7)
    Indirect = 3,
    /// 4 Shall mean that the file is a directory (see 4/8.6)
    Directory = 4,
    /// 5 Shall mean that the file shall be interpreted as a sequence of bytes, each of which may be randomly accessed
    SequenceOfBytes = 5,
    /// 6 Shall mean that the file is a block special device file as specified by ISO/IEC 9945-1
    BlockSpecialDevice = 6,
    /// 7 Shall mean that the file is a character special device file as specified by ISO/IEC 9945-1
    CharacterSpecialDevice = 7,
    /// 8 Shall mean that the file is for recording Extended Attributes as described in 4/9.1
    ExtendedAttributes = 8,
    /// 9 Shall mean that the file is a FIFO file as specified by ISO/IEC 9945-1
    Fifo = 9,
    /// 10 Shall mean that the file shall be interpreted according to the C_ISSOCK file type identified by ISO/IEC 9945-1
    Socket = 10,
    /// 11 Shall mean that this is a Terminal Entry (see 4/14.8)
    TerminalEntry = 11,
    /// 12 Shall mean that the file is a symbolic link and that its content is a pathname (see 4/8.7) for a file or directory
    SymbolicLink = 12,
    /// 13 Shall mean that the file is a Stream Directory (see 4/9.2)
    StreamDirectory = 13,
    /// 14-247 Reserved for future standardisation
    Reserved = 14,
    /// 248-255 Shall be subject to agreement between the originator and recipient of the medium
    Agreement = 248,
}
impl From<u8> for FileType {
    fn from(v: u8) -> FileType {
        match v {
            0 => FileType::Unspecified,
            1 => FileType::Unallocated,
            2 => FileType::PartitionIntegrity,
            3 => FileType::Indirect,
            4 => FileType::Directory,
            5 => FileType::SequenceOfBytes,
            6 => FileType::BlockSpecialDevice,
            7 => FileType::CharacterSpecialDevice,
            8 => FileType::ExtendedAttributes,
            9 => FileType::Fifo,
            10 => FileType::Socket,
            11 => FileType::TerminalEntry,
            12 => FileType::SymbolicLink,
            13 => FileType::StreamDirectory,
            14..=247 => FileType::Reserved,
            248..=255 => FileType::Agreement,
            _ => panic!("Invalid FileType"),
        }
    }
}

/// ECMA 167 4/14.6
/// UDF 2.3.5 http://www.osta.org/specs/pdf/udf260.pdf#page=60
#[derive(Default, Debug, Clone)]
#[repr(C)]
pub struct ICBTag {
    pub prior_recorded_number_of_direct_entries: u32,
    pub strategy_type: u16,
    pub strategy_parameter: [u8; 2],
    pub maximum_number_of_entries: u16,
    pub reserved: u8,
    pub file_type: u8,
    pub parent_icb_location: LbAddr,
    /// http://www.osta.org/specs/pdf/udf260.pdf#page=61
    pub flags: u16,
}
assert_eq_size!(ICBTag, [u8; 20]);
impl ICBTag {
    pub fn read(bytes: &[u8]) -> Self {
        assert_eq!(bytes.len(), 20);
        let mut r = Self::default();
        r.prior_recorded_number_of_direct_entries =
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        r.strategy_type = u16::from_le_bytes([bytes[4], bytes[5]]);
        r.strategy_parameter.copy_from_slice(&bytes[6..8]);
        r.maximum_number_of_entries = u16::from_le_bytes([bytes[8], bytes[9]]);
        r.reserved = bytes[10];
        r.file_type = bytes[11];
        r.parent_icb_location = LbAddr::read(&bytes[12..18]);
        r.flags = u16::from_le_bytes([bytes[18], bytes[19]]);
        r
    }
    
    pub fn write(&self, bytes: &mut [u8]) {
        bytes[0..4].copy_from_slice(&self.prior_recorded_number_of_direct_entries.to_le_bytes());
        bytes[4..6].copy_from_slice(&self.strategy_type.to_le_bytes());
        bytes[6..8].copy_from_slice(&self.strategy_parameter);
        bytes[8..10].copy_from_slice(&self.maximum_number_of_entries.to_le_bytes());
        bytes[10] = self.reserved;
        bytes[11] = self.file_type;
        self.parent_icb_location.write(&mut bytes[12..18]);
        bytes[18..20].copy_from_slice(&self.flags.to_le_bytes());
    }
    pub fn allocation_descriptor_type(&self) -> AllocationDescriptorType {
        AllocationDescriptorType::from(self.flags as u8 & 0b11)
    }
    pub fn file_type(&self) -> FileType {
        FileType::from(self.file_type)
    }
}

/// UDF 2.60 2.3.4 File Identifier Descriptor 
/// http://www.osta.org/specs/pdf/udf260.pdf#page=57
/// ECMA 167 4/14.4
#[derive(Debug, Clone, PartialEq)]
#[repr(C)]
pub struct FileIdentifierDescriptor {
    pub tag: DescriptorTag,
    pub file_version_number: u16,
    /// see ECMA-167 14.4.3 File Characteristics (RBP 18)
    /// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=92
    /// and also correction in http://www.osta.org/specs/pdf/udf260.pdf#page=58
    pub file_characteristics: u8,
    pub length_of_file_identifier: u8,
    pub icb: LongAd,
    pub length_of_implementation_use: u16,

    pub implementation_use: Vec<u8>,
    /// length 0 for parent directory, otherwise length 1-255
    ///
    /// 2.3.4.6 char FileIdentifier[]
    /// http://www.osta.org/specs/pdf/udf260.pdf#page=59
    pub file_identifier: DynamicDstring,
}

impl Default for FileIdentifierDescriptor {
    fn default() -> Self {
        Self {
            tag: Default::default(),
            file_version_number: 0,
            file_characteristics: 0,
            length_of_file_identifier: 0,
            icb: Default::default(),
            length_of_implementation_use: 0,
            implementation_use: Vec::new(),
            file_identifier: DynamicDstring::default(),
        }
    }
}

impl FileIdentifierDescriptor {
    /// ECMA-167 4/7.2.1 Tag Identifier (RBP 0)
    /// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=74
    pub const TAG_IDENTIFIER: u16 = 257;

    pub const FILE_CHARACTERISTIC_EXISTENCE: u8 = 0b0000_0001;
    pub const FILE_CHARACTERISTIC_DIRECTORY: u8 = 0b0000_0010;
    pub const FILE_CHARACTERISTIC_DELETED: u8 = 0b0000_0100;
    pub const FILE_CHARACTERISTIC_PARENT: u8 = 0b0000_1000;
    pub const FILE_CHARACTERISTIC_METADATA: u8 = 0b0001_0000;

    pub fn size(&self) -> usize {
        38 + self.length_of_implementation_use as usize + self.length_of_file_identifier as usize
    }

    pub fn read(bytes: &[u8]) -> Self {
        let mut r = Self::default();
        r.tag = DescriptorTag::read(&bytes[0..16]);
        r.file_version_number = u16::from_le_bytes([bytes[16], bytes[17]]);
        r.file_characteristics = bytes[18];
        r.length_of_file_identifier = bytes[19];
        r.icb = LongAd::read(&bytes[20..36]);
        r.length_of_implementation_use = u16::from_le_bytes([bytes[36], bytes[37]]);
        let impl_use_len = r.length_of_implementation_use as usize;
        let file_id_len = r.length_of_file_identifier as usize;
        r.implementation_use = bytes[38..38 + impl_use_len].to_vec();
        r.file_identifier = DynamicDstring(bytes[38 + impl_use_len..38 + impl_use_len + file_id_len].to_vec());
        r
    }

    pub fn write(&self, bytes: &mut [u8]) {
        self.tag.write(&mut bytes[0..16]);
        bytes[16..18].copy_from_slice(&self.file_version_number.to_le_bytes());
        bytes[18] = self.file_characteristics;
        bytes[19] = self.length_of_file_identifier;
        self.icb.write(&mut bytes[20..36]);
        bytes[36..38].copy_from_slice(&self.length_of_implementation_use.to_le_bytes());
        bytes[38..38 + self.length_of_implementation_use as usize].copy_from_slice(&self.implementation_use);
        bytes[38 + self.length_of_implementation_use as usize..38 + self.length_of_implementation_use as usize + self.length_of_file_identifier as usize].copy_from_slice(&self.file_identifier.0);
    }
}

/// ECMA-167 4/14.7 https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=97
#[derive(Debug, Clone)]
pub struct IndirectEntry {
    pub tag: DescriptorTag,
    pub icb_tag: ICBTag,
    pub indirect_icb: LongAd,
}
assert_eq_size!(IndirectEntry, [u8; 52]);
impl Default for IndirectEntry {
    fn default() -> Self {
        Self {
            tag: Default::default(),
            icb_tag: Default::default(),
            indirect_icb: Default::default(),
        }
    }
}
impl IndirectEntry {
    /// ECMA-167 4/7.2.1 Tag Identifier (RBP 0)
    /// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=74
    pub const TAG_IDENTIFIER: u16 = 259;
    pub fn size() -> usize {
        std::mem::size_of::<IndirectEntry>()
    }
    pub fn read(bytes: &[u8]) -> Self {
        let mut r = Self::default();
        r.tag = DescriptorTag::read(&bytes[0..16]);
        r.icb_tag = ICBTag::read(&bytes[16..36]);
        r.indirect_icb = LongAd::read(&bytes[36..52]);
        r
    }
    pub fn write(&self, bytes: &mut [u8]) {
        self.tag.write(&mut bytes[0..16]);
        self.icb_tag.write(&mut bytes[16..36]);
        self.indirect_icb.write(&mut bytes[36..52]);
    }
}


/// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=116
#[derive(Debug, Clone, PartialEq, Copy)]
#[repr(u8)]
pub enum ExtentType {
    /// 0 Extent recorded and allocated
    /// but “If the 30 least significant bits are set to ZERO, the two most significant bits shall also be set to ZERO”
    RecordedAllocated = 0,
    /// 1 Extent not recorded but allocated
    NotRecordedAllocated = 1,
    /// 2 Extent not recorded and not allocated
    NotRecordedNotAllocated = 2,
    /// 3 The extent is the next extent of allocation descriptors (see 4/12)
    NextExtent = 3,
}
impl ExtentType {
    pub fn from_u8(v: u8) -> ExtentType {
        match v {
            0 => ExtentType::RecordedAllocated,
            1 => ExtentType::NotRecordedAllocated,
            2 => ExtentType::NotRecordedNotAllocated,
            3 => ExtentType::NextExtent,
            _ => panic!("Invalid ExtentType"),
        }
    }
}

/// ECMA-167 4/14.14.1 Short Allocation Descriptor aka struct short_ad
/// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=116
#[derive(Debug, Clone)]
pub struct ShortAllocationDescriptor {
    pub extent_length_and_type: u32,
    /// “the logical block number, within the partition the descriptor is recorded on, of the extent.”
    /// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=116
    pub extent_location: u32,
}
assert_eq_size!(ShortAllocationDescriptor, [u8; 8]);
impl ShortAllocationDescriptor {
    pub fn size() -> usize {
        std::mem::size_of::<ShortAllocationDescriptor>()
    }
    /// Unless otherwise specified, the length shall be an integral multiple of the logical block size.
    /// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=116
    pub fn extent_length_bytes(&self)-> u32 {
        self.extent_length_and_type & 0x3FFFFFFF
    }
    pub fn extent_type(&self)-> ExtentType{
        ExtentType::from_u8((self.extent_length_and_type >> 30) as u8)
    }

    pub fn read(bytes: &[u8]) -> Self {
        let mut r = Self {
            extent_length_and_type: 0,
            extent_location: 0,
        };
        r.extent_length_and_type = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        r.extent_location = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        r
    }

    pub fn write(&self, bytes: &mut [u8]) {
        bytes[0..4].copy_from_slice(&self.extent_length_and_type.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.extent_location.to_le_bytes());
    }
}

/// 2.3.10.1 Long Allocation Descriptor aka ECMA 167 4/14.14.2 aka struct long_ad
/// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=117
/// http://www.osta.org/specs/pdf/udf260.pdf#page=66
#[derive(Default, Debug, Clone, PartialEq)]
#[repr(C)]
pub struct LongAd {
    /// length in bytes, with most significant 2 bits used for flags
    pub extent_length_and_type: u32,
    /// “This field shall specify the logical block number of the extent.
    /// If the extent's length is 0, no extent is specified
    /// and this field shall contain 0.”
    /// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=117
    pub extent_location: LbAddr,
    pub implementation_use: [u8; 6],
}
assert_eq_size!(LongAd, [u8; 16]);
impl LongAd {
    pub const fn size() -> usize {
        std::mem::size_of::<LongAd>()
    }

    pub fn read(bytes: &[u8]) -> Self {
        assert_eq!(bytes.len(), 16);
        let mut r = Self::default();
        r.extent_length_and_type = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        r.extent_location = LbAddr::read(&bytes[4..10]);
        r.implementation_use.copy_from_slice(&bytes[10..16]);
        r
    }

    pub fn write(&self, bytes: &mut [u8]) {
        bytes[0..4].copy_from_slice(&self.extent_length_and_type.to_le_bytes());
        self.extent_location.write(&mut bytes[4..10]);
        bytes[10..16].copy_from_slice(&self.implementation_use);
    }

    /// Unless otherwise specified, the length shall be an integral multiple of the logical block size.
    /// https://ecma-international.org/wp-content/uploads/ECMA-167_3rd_edition_june_1997.pdf#page=116
    pub fn extent_length_bytes(&self)-> u32 {
        self.extent_length_and_type & 0x3FFFFFFF
    }
    pub fn extent_type(&self)-> ExtentType{
        ExtentType::from_u8((self.extent_length_and_type >> 30) as u8)
    }
}

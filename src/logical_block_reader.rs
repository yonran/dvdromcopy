use std::{
    collections::BTreeMap,
    io::{Read, Seek},
};

use log::debug;

use crate::{
    cache::Cache,
    dvdcss_sys::DVDCSS_BLOCK_SIZE,
    udf::{LogicalVolumeDescriptor, LongAd, PartitionDescriptor, ShortAllocationDescriptor},
    udf_parser::{Result, UdfError::InvalidPartitionNumber},
};

pub fn long_ad_to_sector_number(
    logical_volume_descriptor: &LogicalVolumeDescriptor,
    partition_descriptors: &BTreeMap<u16, PartitionDescriptor>,
    long_ad: &LongAd,
) -> Option<u32> {
    let partition_reference_number = long_ad.extent_location.partition_reference_number;
    let partition_descriptor = partition_descriptors.get(&partition_reference_number);
    if let Some(partition_descriptor) = partition_descriptor {
        let pos: u32 = partition_descriptor.partition_starting_location
            + (long_ad.extent_location.logical_block_number as u32)
                * (logical_volume_descriptor.logical_block_size as u32) / DVDCSS_BLOCK_SIZE as u32;
        Some(pos)
    } else {
        None
    }
}
// this is wrong
// pub fn long_ad_to_pos_in_partition(
//     logical_volume_descriptor: &LogicalVolumeDescriptor,
//     partition_descriptors: &BTreeMap<u16, PartitionDescriptor>,
//     long_ad: &LongAd,
// ) -> Option<usize> {
//     long_ad_to_sector_number(logical_volume_descriptor, partition_descriptors, long_ad)
//         .map(|sector| sector as usize * DVDCSS_BLOCK_SIZE as usize)
// }
pub fn short_ad_to_pos_in_partition(
    logical_volume_descriptor: &LogicalVolumeDescriptor,
    short_ad: &ShortAllocationDescriptor,
) -> usize {
    let pos: usize = short_ad.extent_location as usize
        * logical_volume_descriptor.logical_block_size as usize;
    pos
}

pub fn read_exact_from_partition<R: Read + Seek, const BYTE_SIZE: usize>(
    cache: &mut Cache<R, BYTE_SIZE>,
    partition_descriptor: &PartitionDescriptor,
    pos_in_partition: usize,
    buf: &mut [u8],
) -> Result<()> {
    debug!(
        "read_exact_from_partition: partition_starting_location={}, pos_in_partition={}, len={}",
        partition_descriptor.partition_starting_location, pos_in_partition, buf.len()
    );
    let pos = partition_descriptor.partition_starting_location as usize * DVDCSS_BLOCK_SIZE + pos_in_partition;
    cache.read_exact(pos, buf)
}

use std::collections::BTreeMap;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

use clap::Parser;
use dvdromcopy::cache::Cache;
use dvdromcopy::dvdcss_sys::{css_to_io_error, DvdCss, DVDCSS_BLOCK_SIZE};
use dvdromcopy::logical_block_reader::{read_exact_from_partition, short_ad_to_pos_in_partition};
use dvdromcopy::udf::{
    Dstring, FileIdentifierDescriptor, LogicalVolumeDescriptor, LongAd, PartitionDescriptor,
    PartitionMap, ShortAllocationDescriptor, Type1PartitionMap,
};
use dvdromcopy::udf_parser::{
    read_directory_contents, read_file_entries, read_short_allocation_descriptors, Result, UdfError, UdfParser
};
use log::{self, debug, error, warn};
use std::fs::{create_dir, create_dir_all};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The DVD device or file to open
    #[arg(short, long)]
    device: String,

    /// The output directory to write the DVD to
    #[arg(short, long)]
    output: PathBuf,

    /// Name of the DVD; if not specified then it will read from DVD
    /// primary_volume.volume_identifier
    #[arg(long)]
    name: Option<String>,

    /// Include only the specified files and directories
    #[arg(long)]
    include: Option<Vec<String>>,
}


fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();
    run(&args)?;
    println!("Hello, world!");
    Ok(())
}

fn titlecase_name(name: &str) -> String {
    let mut result = String::new();
    let mut capitalize = true;
    for c in name.chars() {
        if c == ' ' || c == '_' {
            capitalize = true;
            result.push(' ');
        } else if capitalize {
            result.push_str(&c.to_uppercase().to_string());
            capitalize = false;
        } else {
            result.push_str(&c.to_lowercase().to_string());
        }
    }
    result.trim().to_string()
}

struct RunOnDirectoryOptions<'a> {
    dvd_dir: &'a Path,
    
}

fn run_on_directory<R: Read + Seek>(
    reader: &mut Cache<&mut R, DVDCSS_BLOCK_SIZE>,
    logical_volume_descriptor: &LogicalVolumeDescriptor,
    partition_descriptors: &BTreeMap<u16, PartitionDescriptor>,
    icb_address: &LongAd,
    dvd_dir: &Path,
    path: &mut Vec<String>,
) -> Result<()> {
    let partition_descriptor = partition_descriptors
        .get(&(icb_address.extent_location.partition_reference_number | 0))
        .ok_or_else(|| {
            error!(
                "Could not find partition descriptor for directory ICB: {}",
                &(icb_address.extent_location.partition_reference_number | 0)
            );
            UdfError::InvalidPartitionNumber
        })?;
    let file_entries = read_file_entries(
        reader,
        logical_volume_descriptor,
        partition_descriptor,
        &ShortAllocationDescriptor {
            extent_length_and_type: icb_address.extent_length_and_type,
            extent_location: icb_address.extent_location.logical_block_number,
        },
    )?;
    let file_identifier_descriptors = read_directory_contents(
        reader,
        logical_volume_descriptor,
        partition_descriptor,
        &*file_entries,
    )?;
    for file_identifier_descriptor in file_identifier_descriptors.iter() {
        let path_string =
            path.join("/") + "/" + &file_identifier_descriptor.file_identifier.to_string();
        if file_identifier_descriptor.file_characteristics
            & FileIdentifierDescriptor::FILE_CHARACTERISTIC_PARENT
            != 0
        {
            // don't infinite loop up to parent directory
            continue;
        }
        if file_identifier_descriptor.file_characteristics
            & FileIdentifierDescriptor::FILE_CHARACTERISTIC_DIRECTORY
            != 0
        {
            path.push(file_identifier_descriptor.file_identifier.to_string());
            debug!(
                "run_on_directory: descending into subdirectory {:?}",
                path_string
            );
            let result = run_on_directory(
                reader,
                logical_volume_descriptor,
                partition_descriptors,
                &file_identifier_descriptor.icb,
                dvd_dir,
                path,
            );
            path.pop();
            result?;
        } else {
            path.push(file_identifier_descriptor.file_identifier.to_string());
            debug!("run_on_directory: file {:?}", path_string);
            path.pop();
            // read file
            read_file(
                reader,
                logical_volume_descriptor,
                partition_descriptors,
                dvd_dir,
                path_string,
                &file_identifier_descriptor.icb,
            )?;
        }
        // debug!(
        //     "Found file identifier descriptor: {:?} {}",
        //     file_identifier_descriptor, path_string
        // );
    }

    Ok(())
}

fn read_file<R: Read + Seek>(
    reader: &mut Cache<&mut R, 2048>,
    logical_volume_descriptor: &LogicalVolumeDescriptor,
    partition_descriptors: &BTreeMap<u16, PartitionDescriptor>,
    dvd_dir: &Path,
    path: String,
    icb_address: &LongAd,
) -> Result<()> {
    let output_path = dvd_dir.join(&path);
    if let Some(parent) = output_path.parent() {
        create_dir_all(parent)?;
    }
    let partition_descriptor = partition_descriptors
        .get(&(icb_address.extent_location.partition_reference_number | 0))
        .ok_or_else(|| {
            error!(
                "Could not find partition descriptor for directory ICB: {}",
                &(icb_address.extent_location.partition_reference_number | 0)
            );
            UdfError::InvalidPartitionNumber
        })?;
    let file_entries = read_file_entries(reader, logical_volume_descriptor, partition_descriptor,         &ShortAllocationDescriptor {
        extent_length_and_type: icb_address.extent_length_and_type,
        extent_location: icb_address.extent_location.logical_block_number,
    })?;
    if file_entries.len() != 1 {
        warn!(
            "Expected exactly one file entry for file {:?}, but got {}",
            path,
            file_entries.len()
        );
    }
    // let mut output_file = std::fs::File::open(&output_path).map_err(|err| {
    //     error!("Could not open output file {:?}: {}", output_path, err);
    //     err
    // })?;
    let mut output_file = std::fs::File::create_new(&output_path).map_err(|err| {
        error!("Could not open output file {:?}: {}", output_path, err);
        err
    })?;
    let mut partition_count_match: u32 = 0;
    let mut partition_count_fix_zero: u32 = 0;
    let mut partition_count_mismatch: u32 = 0;
    for file_entry in file_entries.iter() {
        let allocation_descriptors =
        read_short_allocation_descriptors(&*file_entry.allocation_descriptors);
        for ad in &allocation_descriptors {
            debug!("path {}: reading part {:?}", path, ad);
            let pos_in_partition = short_ad_to_pos_in_partition(logical_volume_descriptor, ad);
            let mut buf: Vec<u8> = vec![0u8; 1024*1024];
            let mut output_buf: Vec<u8> = vec![0u8; 1024*1024];
            let mut offset: usize = 0;
            while offset < ad.extent_length_bytes() as usize {
                let pos_this_iteration = pos_in_partition + offset as usize;
                let len_this_iteration = (ad.extent_length_bytes() as usize - offset).min(buf.len());
                let slice = &mut buf[..len_this_iteration];
                read_exact_from_partition(
                    reader,
                    partition_descriptor,
                    pos_this_iteration,
                    slice,
                )?;

                output_file.write_all(slice)?;
                // let output_slice = &mut output_buf[..len_this_iteration];
                // output_file.read_exact(output_slice)?;

                // for i in 0..((slice.len() as u32).div_ceil(logical_volume_descriptor.logical_block_size)) { 
                //     let logical_block_in_slice_start = i as usize * logical_volume_descriptor.logical_block_size as usize;
                //     let logical_block_in_slice_end = ((i + 1) as usize * logical_volume_descriptor.logical_block_size as usize).min(slice.len());
                //     if slice[logical_block_in_slice_start..logical_block_in_slice_end] == output_slice[logical_block_in_slice_start..logical_block_in_slice_end] {
                //         partition_count_match += 1;
                //     } else if output_slice[logical_block_in_slice_start..logical_block_in_slice_end].iter().all(|&x| x == 0) {
                //         partition_count_fix_zero += 1;
                //     } else {
                //         partition_count_mismatch += 1;
                //     }
                // }
                offset += len_this_iteration;
            }
        }
    }
    output_file.sync_all()?;
    debug!(
        "read_file: {:?}: partitions match: {}, fix_zero: {}, mismatch: {}",
        output_path, partition_count_match, partition_count_fix_zero, partition_count_mismatch
    );

    Ok(())
}

fn run(args: &Args) -> Result<()> {
    println!("run");
    let css = DvdCss::open(&args.device).map_err(css_to_io_error)?;
    let mut parser = UdfParser::new(css);
    let structures = parser.read_volume_structures()?;
    debug!("volume structures {:?}", structures);
    let name_from_dvd = titlecase_name(&structures.primary_volume.volume_identifier.to_string());
    debug!("name from dvd: {}", name_from_dvd);
    let name = args.name.as_ref().unwrap_or(&name_from_dvd);
    let dvd_dir = args.output.join(name);
    if let Err(e) = create_dir(&dvd_dir) {
        if e.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(e.into());
        }
    }
    // parser.read_filesystem(&structures, output, name)?;
    // See UDF 2.6.0 6.9 Requirements for DVD-ROM http://www.osta.org/specs/pdf/udf260.pdf#page=136
    for partition_map in structures.partition_maps.iter() {
        match partition_map {
            PartitionMap::Type1(partition_map) => {
                let partition_descriptor = structures
                    .partition_descriptors
                    .get(&partition_map.partition_number);
                if let Some(partition_descriptor) = partition_descriptor {
                    debug!("Found matching partition descriptor: {:?} -> starting location: {} sector, length: {} sectors",
                        partition_descriptor, partition_descriptor.partition_starting_location, partition_descriptor.partition_length);
                    let fsds =
                        parser.read_fileset_descriptors(partition_descriptor, partition_map)?;
                    let mut reader =
                        Cache::<&mut DvdCss, DVDCSS_BLOCK_SIZE>::new(&mut parser.reader);

                    for fsd in &fsds[..1] {
                        run_on_directory(
                            &mut reader,
                            &structures.logical_volume,
                            &structures.partition_descriptors,
                            &fsd.root_directory_icb,
                            &dvd_dir,
                            &mut vec![],
                        )?;
                    }
                } else {
                    warn!(
                        "Could not find matching partition descriptor for partition map: {:?}",
                        partition_map
                    );
                }
            }
            _ => {
                log::warn!("Ignoring other partition type");
            }
        }
    }
    // structures.partition_maps
    Ok(())
}

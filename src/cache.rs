use std::{collections::BTreeMap, io::{Read, Seek}, mem, num::NonZero};

use log::debug;
use lru::LruCache;

use crate::{dvdcss_sys::DVDCSS_BLOCK_SIZE, udf::{LogicalVolumeDescriptor, LongAd, PartitionDescriptor}, udf_parser::Result};


pub struct Cache<R: Read + Seek, const BYTE_SIZE: usize> {
    /// The cache data.
    data: [u8; BYTE_SIZE],
    lru_cache: lru::LruCache<u32, u32>,
    empty_blocks: Vec<u32>,
    reader: R,
}
impl<R: Read + Seek, const BYTE_SIZE: usize> Cache<R, BYTE_SIZE>   {
    /// Create a new cache.
    pub fn new(reader:R) -> Cache<R, BYTE_SIZE> {
        let mut empty_blocks = Vec::with_capacity(BYTE_SIZE / DVDCSS_BLOCK_SIZE);
        for i in 0..BYTE_SIZE / DVDCSS_BLOCK_SIZE {
            empty_blocks.push(i as u32);
        }
        Cache {
            data: [0; BYTE_SIZE],
            lru_cache: LruCache::new(NonZero::new(empty_blocks.len()).unwrap()),
            empty_blocks,
            reader
        }
    }
    fn ensure_empty_block(&mut self) -> u32 {
        if let Some(index) = self.empty_blocks.pop() {
            index
        } else {
            let (_old_block, index) = self.lru_cache.pop_lru().unwrap();
            index
        }
    }
    pub fn read_exact(&mut self, pos: usize, buf: &mut [u8]) -> Result<()> {
        let end_pos = pos + buf.len();
        let mut read = 0;
        while read < buf.len() {
            let pos_this_read = pos + read;
            let block = pos_this_read / DVDCSS_BLOCK_SIZE;
            let offset = pos_this_read % DVDCSS_BLOCK_SIZE;
            let end_pos_this_read = end_pos.min((block + 1) * DVDCSS_BLOCK_SIZE);
            let len = end_pos_this_read - pos_this_read;
            let data = self.read_block(block as u32)?;
            buf[read..read + len].copy_from_slice(&data[offset..offset + len]);
            read += len;
        }
        // debug!("read_exact: pos={}, len={} read {:?}", pos, buf.len(), buf);
        Ok(())
    }
    pub fn read_block(&mut self, block: u32) -> Result<&[u8]> {
        let existing = self.lru_cache.get(&block);
        if let Some(&index) = existing {
            let start = index as usize * DVDCSS_BLOCK_SIZE as usize;
            Ok(&self.data[start..start + DVDCSS_BLOCK_SIZE as usize])
        } else {
            let index = self.ensure_empty_block();
            let buf = &mut self.data[index as usize * DVDCSS_BLOCK_SIZE..
                (index + 1) as usize * DVDCSS_BLOCK_SIZE];
            buf.fill(0);
            match (|| -> Result<()> {
                self.reader.seek(std::io::SeekFrom::Start(block as u64 * DVDCSS_BLOCK_SIZE as u64))?;
                self.reader.read_exact(buf)?;
                Ok(())
            })() {
                Ok(()) => {
                    self.lru_cache.put(block, index);
                    Ok(buf)
                }
                Err(e) => {
                    self.empty_blocks.push(index);
                    Err(e)
                }
            }
        }
    }
}


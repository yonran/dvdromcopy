use std::{ffi::{c_char, c_int, c_void, CStr}, io::{Read, Seek}};

use log::debug;

#[repr(C)]
pub struct DvdCssS {
    _unused: [u8; 0],
}
/// Library instance handle, to be used for each library call.
pub type DvdCssT = *mut DvdCssS;

/// Set of callbacks to access DVDs in custom ways.
#[repr(C)]
pub struct DvdCssStreamCb {
    /// custom seek callback
    pub pf_seek: Option<extern "C" fn(p_stream: *mut std::ffi::c_void, i_pos: u64) -> i32>,
    /// custom read callback
    pub pf_read: Option<
        extern "C" fn(
            p_stream: *mut std::ffi::c_void,
            buffer: *mut std::ffi::c_void,
            i_read: i32,
        ) -> i32,
    >,
    /// custom vectored read callback
    pub pf_readv: Option<
        extern "C" fn(
            p_stream: *mut std::ffi::c_void,
            p_iovec: *const std::ffi::c_void,
            i_blocks: i32,
        ) -> i32,
    >,
}

/// The block size of a DVD.
pub const DVDCSS_BLOCK_SIZE: usize = 2048;

/// The default flag to be used by libdvdcss functions.
pub const DVDCSS_NOFLAGS: i32 = 0;

/// Flag to ask dvdcss_read() to decrypt the data it reads.
pub const DVDCSS_READ_DECRYPT: i32 = 1 << 0;

/// Flag to tell dvdcss_seek() it is seeking in MPEG data.
pub const DVDCSS_SEEK_MPEG: i32 = 1 << 0;

/// Flag to ask dvdcss_seek() to check the current title key.
pub const DVDCSS_SEEK_KEY: i32 = 1 << 1;

#[link(name = "dvdcss")]
extern "C" {
    /// Opens a DVD device or file.
    pub fn dvdcss_open(psz_target: *const c_char) -> DvdCssT;

    /// Opens a DVD device or file using custom stream callbacks.
    pub fn dvdcss_open_stream(p_stream: *mut c_void, p_stream_cb: *mut DvdCssStreamCb) -> DvdCssT;

    /// Closes a DVD device or file.
    pub fn dvdcss_close(dvdcss: DvdCssT) -> c_int;

    /// Seeks to a specific block on the DVD.
    pub fn dvdcss_seek(dvdcss: DvdCssT, i_blocks: c_int, i_flags: c_int) -> c_int;

    /// Reads data from the DVD.
    pub fn dvdcss_read(
        dvdcss: DvdCssT,
        p_buffer: *mut c_void,
        i_blocks: c_int,
        i_flags: c_int,
    ) -> c_int;

    /// Reads data from the DVD using vectored I/O.
    pub fn dvdcss_readv(
        dvdcss: DvdCssT,
        p_iovec: *mut c_void,
        i_blocks: c_int,
        i_flags: c_int,
    ) -> c_int;

    /// Returns the last error message.
    pub fn dvdcss_error(dvdcss: DvdCssT) -> *const c_char;

    /// Checks if the DVD is scrambled.
    pub fn dvdcss_is_scrambled(dvdcss: DvdCssT) -> c_int;
}
pub struct DvdCss {
    handle: DvdCssT,
}
pub fn css_to_io_error(css_error: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, css_error)
}

impl Read for DvdCss {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.css_read(buf, buf.len().div_ceil(DVDCSS_BLOCK_SIZE as usize) as i32, DVDCSS_READ_DECRYPT) {
            Ok(size) => Ok(size as usize * DVDCSS_BLOCK_SIZE),
            Err(e) => Err(css_to_io_error(e)),
        }
    }
}
impl Seek for DvdCss {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        let blocks = match pos {
            std::io::SeekFrom::Start(offset) => (offset / DVDCSS_BLOCK_SIZE as u64) as i32,
            std::io::SeekFrom::End(_offset) => panic!("SeekFrom::End is not supported"),
            std::io::SeekFrom::Current(_offset) => panic!("SeekFrom::Current is not supported"),
        };
        self.css_seek(blocks, DVDCSS_NOFLAGS).map(|x| x as u64).map_err(css_to_io_error)
    }
}

impl DvdCss {
    /// Opens a DVD device or file.
    pub fn open(target: &str) -> Result<Self, String> {
        let c_target = std::ffi::CString::new(target).map_err(|e| e.to_string())?;
        debug!("dvdcss_open({})",target);
        let handle = unsafe { dvdcss_open(c_target.as_ptr()) };
        if handle.is_null() {
            Err("Failed to open DVD device or file".to_string())
        } else {
            Ok(DvdCss { handle })
        }
    }

    /// Opens a DVD device or file using custom stream callbacks.
    pub fn open_stream(
        stream: *mut std::ffi::c_void,
        stream_cb: &mut DvdCssStreamCb,
    ) -> Result<Self, String> {
        debug!("dvdcss_open_stream()");
        let handle = unsafe { dvdcss_open_stream(stream, stream_cb) };
        if handle.is_null() {
            Err("Failed to open DVD device or file with custom stream".to_string())
        } else {
            Ok(DvdCss { handle })
        }
    }

    /// Seeks to a specific block on the DVD.
    pub fn css_seek(&self, blocks: i32, flags: i32) -> Result<i32, String> {
        // debug!("dvdcss_seek({}, {})", blocks, flags);
        let result = unsafe { dvdcss_seek(self.handle, blocks, flags) };
        if result < 0 {
            Err(self.error())
        } else {
            Ok(result)
        }
    }

    /// Reads data from the DVD.
    pub fn css_read(&self, buffer: &mut [u8], blocks: i32, flags: i32) -> Result<i32, String> {
        // debug!("dvdcss_read(buf with length {}, {}, {})", buffer.len(), blocks, flags);
        assert!(buffer.len() >= (blocks as usize) * DVDCSS_BLOCK_SIZE as usize);
        let result = unsafe {
            dvdcss_read(
                self.handle,
                buffer.as_mut_ptr() as *mut c_void,
                blocks,
                flags,
            )
        };
        if result < 0 {
            Err(self.error())
        } else {
            Ok(result)
        }
    }

    /// Reads data from the DVD using vectored I/O.
    pub fn readv(&self, iovec: *mut c_void, blocks: i32, flags: i32) -> Result<i32, String> {
        debug!("dvdcss_readv({}, {})", blocks, flags);
        let result = unsafe { dvdcss_readv(self.handle, iovec, blocks, flags) };
        if result < 0 {
            Err(self.error())
        } else {
            Ok(result)
        }
    }

    /// Checks if the DVD is scrambled.
    pub fn is_scrambled(&self) -> bool {
        debug!("dvdcss_is_scrambled()");
        unsafe { dvdcss_is_scrambled(self.handle) != 0 }
    }

    /// Returns the last error message.
    fn error(&self) -> String {
        debug!("dvdcss_error()");
        unsafe {
            let err_ptr = dvdcss_error(self.handle);
            if err_ptr.is_null() {
                "Unknown error".to_string()
            } else {
                CStr::from_ptr(err_ptr).to_string_lossy().into_owned()
            }
        }
    }
    /// Closes the DVD device or file.
    pub fn close(self) -> Result<(), String> {
        debug!("dvdcss_close()");
        let result = unsafe { dvdcss_close(self.handle) };
        if result < 0 {
            Err(self.error())
        } else {
            Ok(())
        }
    }
}

impl Drop for DvdCss {
    fn drop(&mut self) {
        debug!("dvdcss_close()");
        unsafe {
            dvdcss_close(self.handle);
        }
    }
}

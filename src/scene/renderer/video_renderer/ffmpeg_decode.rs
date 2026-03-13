use std::ffi::*;

use ffmpeg_sys_next::{
    self as ffmpeg, av_free, av_malloc, avformat_alloc_context, avio_alloc_context,
};

struct BufReaderC {
    content: *mut u8,
    pos: usize,
}

unsafe extern "C" fn read_callback(opague: *mut c_void, buf: *mut u8, buf_size: c_int) -> c_int {
    let state = opague as *mut BufReaderC;
    0
}

unsafe extern "C" fn seek_callback(opague: *mut c_void, offset: i64, whence: c_int) -> i64 {
    0
}

pub fn decode_to_h264(mut container: Vec<u8>) -> Result<(), &'static str> {
    unsafe {
        ffmpeg::avformat_network_init();

        let mut reader = Box::new(BufReaderC {
            content: container.as_mut_ptr(),
            pos: 0,
        });

        const AVIO_BUF_SIZE: usize = 4096;
        let avio_buf = av_malloc(AVIO_BUF_SIZE) as *mut u8;
        if avio_buf.is_null() {
            return Err("Failed to malloc avio");
        }

        let avio_ctx = avio_alloc_context(
            avio_buf,
            AVIO_BUF_SIZE as i32,
            0,
            reader.as_mut() as *mut _ as *mut c_void,
            Some(read_callback),
            None,
            Some(seek_callback),
        );

        if avio_ctx.is_null() {
            av_free(avio_ctx as *mut c_void);
            return Err("Failed to alloc avio");
        }

        let mut fmt_ctx = avformat_alloc_context();
        if fmt_ctx.is_null() {
            av_free(fmt_ctx as *mut c_void);
            return Err("Failed to alloc fmt");
        }

        Ok(())
    }
}

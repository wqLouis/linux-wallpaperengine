use std::{ffi::*, ptr, slice::from_raw_parts};

use ffmpeg_sys_next::*;

struct BufReaderffmpeg {
    content: Vec<u8>,
    pos: usize,
}

unsafe extern "C" fn read_callback(opague: *mut c_void, buf: *mut u8, buf_size: c_int) -> c_int {
    // copy the data to ffmpeg buffer with size

    unsafe {
        let state = &mut *(opague as *mut BufReaderffmpeg);

        if buf_size < 0 {
            return AVERROR(EINVAL);
        }

        if state.pos + buf_size as usize > state.content.len() {
            return AVERROR(AVERROR_EOF);
        }

        let readable = (buf_size as i64 as usize).min(state.content.len() - state.pos);

        ptr::copy(state.content.as_ptr().add(state.pos), buf, readable);

        state.pos += readable;

        readable as c_int
    }
}

unsafe extern "C" fn seek_callback(opague: *mut c_void, offset: i64, whence: c_int) -> i64 {
    // seek the buffer in the bufreaderffmpeg

    unsafe {
        let state = &mut *(opague as *mut BufReaderffmpeg);

        let pos = match whence {
            SEEK_SET => offset as usize,
            SEEK_CUR => state.pos.checked_add(offset as usize).unwrap_or(usize::MAX),
            SEEK_END => state.content.len(),
            _ => return -1,
        };

        if pos > state.content.len() {
            return -1;
        }

        state.pos = pos.clone();

        pos as i64
    }
}

/// decode mp4 container to raw h264 byte stream
pub fn decode_to_h264(container: Vec<u8>) -> Result<Vec<u8>, &'static str> {
    unsafe {
        let mut clean = Vec::<CleanUp>::with_capacity(4);

        avformat_network_init();

        let mut reader = Box::new(BufReaderffmpeg {
            content: container,
            pos: 0,
        });

        const AVIO_BUF_SIZE: usize = 4096;
        let avio_buf = av_malloc(AVIO_BUF_SIZE) as *mut u8;
        if avio_buf.is_null() {
            clean.push(CleanUp::av_free(avio_buf as *mut c_void));
            clean_up(clean);
            drop(reader);
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

        clean.push(CleanUp::av_free(avio_ctx as *mut c_void));

        if avio_ctx.is_null() {
            clean_up(clean);
            drop(reader);
            return Err("Failed to alloc avio");
        }

        let mut fmt_ctx = avformat_alloc_context();

        if fmt_ctx.is_null() {
            clean.push(CleanUp::avformat_free_context(fmt_ctx));
            clean_up(clean);
            drop(reader);
            return Err("Failed to alloc fmt");
        }

        (*fmt_ctx).pb = avio_ctx;
        (*fmt_ctx).flags |= AVFMT_FLAG_CUSTOM_IO;

        let ret = avformat_open_input(&mut fmt_ctx, ptr::null(), ptr::null(), ptr::null_mut());
        clean.push(CleanUp::avformat_close_input(&mut fmt_ctx));

        if ret < 0 {
            clean_up(clean);
            drop(reader);
            return Err("Failed to open input");
        }

        if avformat_find_stream_info(fmt_ctx, ptr::null_mut()) < 0 {
            clean_up(clean);
            drop(reader);
            return Err("Failed to find stream info");
        }

        let video_stream_idx = av_find_best_stream(
            fmt_ctx,
            AVMediaType::AVMEDIA_TYPE_VIDEO,
            -1,
            -1,
            ptr::null_mut(),
            0,
        );

        if video_stream_idx < 0 {
            clean_up(clean);
            drop(reader);
            return Err("No video found");
        }

        let mut output: Vec<u8> = vec![];
        let pkt: *mut AVPacket = ptr::null_mut();
        av_init_packet(pkt);
        clean.push(CleanUp::av_free(pkt as *mut c_void));

        loop {
            let ret = av_read_frame(fmt_ctx, pkt);
            if ret < 0 {
                if ret == AVERROR_EOF {
                    break;
                }

                clean_up(clean);
                drop(reader);
                return Err("Read frame failed");
            }

            if (*pkt).stream_index == video_stream_idx {
                if !(*pkt).data.is_null() && (*pkt).size > 0 {
                    let data_slice = from_raw_parts((*pkt).data, (*pkt).size as usize);
                    output.append(&mut data_slice.to_vec());
                }
            }
        }

        clean_up(clean);
        Ok(output)
    }
}

#[allow(non_camel_case_types)]
enum CleanUp {
    av_free(*mut c_void),
    avformat_free_context(*mut AVFormatContext),
    avformat_close_input(*mut *mut AVFormatContext),
}

unsafe fn clean_up(to_clean: Vec<CleanUp>) {
    unsafe {
        for item in to_clean {
            match item {
                CleanUp::avformat_free_context(fmt_ctx) => {
                    avformat_free_context(fmt_ctx);
                }
                CleanUp::avformat_close_input(fmt_ctx) => {
                    avformat_close_input(fmt_ctx);
                }
                CleanUp::av_free(ptr) => {
                    av_free(ptr);
                }
            }
        }
    }
}

//! Shared frame hand-off between the renderer and the GNOME Shell extension.
//!
//! The rendered frame is written into a file inside a tmpfs (/dev/shm) and
//! only its path is sent over D-Bus.  Two files are used alternately, so the
//! extension can read a complete frame while the next one is rendered.

use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// A file holding one frame.
struct FrameFile {
    path: PathBuf,
    file: File,
}

/// Writes frames into shared memory files.
pub struct FrameWriter {
    files: Vec<FrameFile>,
    next: usize,
}

impl FrameWriter {
    /// Create the shared frame files.
    ///
    /// `tag` is used to make the file names unique per process, so several
    /// instances of the app can run at the same time.
    pub fn new(tag: &str) -> io::Result<Self> {
        let dir = shared_dir();
        let mut files = Vec::new();

        for index in 0..2 {
            let path = dir.join(format!("lwe-frame-{}-{index}.rgba", tag));
            let file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)?;
            files.push(FrameFile { path, file });
        }

        Ok(Self { files, next: 0 })
    }

    /// Write a frame and return the path it was written to.
    ///
    /// The frame is written completely before the path is handed out, so the
    /// reader always sees a consistent image.
    pub fn write(&mut self, data: &[u8]) -> io::Result<&Path> {
        let index = self.next;
        self.next = (self.next + 1) % self.files.len();

        let frame = &mut self.files[index];

        // Rewind first: truncating the file does not move the write cursor,
        // so the frame would be appended after the previous one.
        frame.file.seek(SeekFrom::Start(0))?;
        frame.file.write_all(data)?;
        // Make sure the file has exactly the size of the frame.
        frame.file.set_len(data.len() as u64)?;

        Ok(&frame.path)
    }
}

impl Drop for FrameWriter {
    fn drop(&mut self) {
        for frame in &self.files {
            let _ = std::fs::remove_file(&frame.path);
        }
    }
}

/// The directory used for the shared frames.
///
/// /dev/shm is preferred because it is a tmpfs: the frames never reach the
/// disk.  When it is not available the temporary directory is used instead.
fn shared_dir() -> PathBuf {
    let shm = Path::new("/dev/shm");
    if shm.is_dir() {
        shm.to_path_buf()
    } else {
        std::env::temp_dir()
    }
}

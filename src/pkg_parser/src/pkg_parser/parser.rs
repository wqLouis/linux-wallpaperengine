use std::{
    collections::HashMap,
    fs::{self, File, create_dir_all},
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
};

use crate::pkg_parser::tex_parser;

#[derive(Debug, Clone)]
pub struct Pkg {
    pub header: Header,
    pub files: HashMap<String, Vec<u8>>,
}

#[derive(Debug, Clone)]
pub(self) struct Header {
    version: String,
    file_count: u32,
}

pub(self) struct Entry {
    pub path: String,
    pub offset: u32,
    pub size: u32,
}

impl Pkg {
    pub fn new(pkg_path: &Path) -> Pkg {
        let mut file = BufReader::new(File::open(pkg_path).unwrap());
        let header = Self::read_header(&mut file);
        let entries = Self::read_entries(&mut file, header.file_count);
        let files = Self::read_files(&mut file, &entries);

        Pkg { header, files }
    }

    fn read_header(file: &mut BufReader<File>) -> Header {
        // Read the header of the pkg

        const HEADER_LEN: usize = 4;
        const FILE_COUNT_LEN: usize = 4;

        fn header_len(file: &mut BufReader<File>) -> u32 {
            let mut header_len = [0u8; HEADER_LEN];
            file.read_exact(&mut header_len).unwrap();
            u32::from_le_bytes(header_len)
        }

        fn header_version(file: &mut BufReader<File>, len: usize) -> String {
            let mut header_v = vec![0u8; len];
            file.read_exact(&mut header_v).unwrap();
            String::from_utf8(header_v).unwrap()
        }

        fn file_count(file: &mut BufReader<File>) -> u32 {
            let mut file_count = [0u8; FILE_COUNT_LEN];
            file.read_exact(&mut file_count).unwrap();
            u32::from_le_bytes(file_count)
        }

        let len = header_len(file);

        Header {
            version: header_version(file, len as usize),
            file_count: file_count(file),
        }
    }

    fn read_entries(file: &mut BufReader<File>, entry_count: u32) -> Vec<Entry> {
        // Read the file entry of pkg

        const PATH_LEN: usize = 4;
        const DATA_OFFSET: usize = 4;
        const DATA_SIZE: usize = 4;

        let mut path_len = [0u8; PATH_LEN];
        let mut entries = Vec::<Entry>::with_capacity(entry_count as usize);

        for _ in 0..entry_count {
            file.read_exact(&mut path_len).unwrap();

            let mut path_buffer = vec![0u8; u32::from_le_bytes(path_len) as usize];
            let mut data_offset_buffer = [0u8; DATA_OFFSET];
            let mut data_size_buffer = [0u8; DATA_SIZE];

            file.read_exact(&mut path_buffer).unwrap();
            file.read_exact(&mut data_offset_buffer).unwrap();
            file.read_exact(&mut data_size_buffer).unwrap();

            entries.push(Entry {
                path: String::from_utf8(path_buffer).unwrap(),
                offset: u32::from_le_bytes(data_offset_buffer),
                size: u32::from_le_bytes(data_size_buffer),
            });
        }

        entries
    }

    fn read_files(file: &mut BufReader<File>, entries: &Vec<Entry>) -> HashMap<String, Vec<u8>> {
        let mut map = HashMap::<String, Vec<u8>>::new();
        let pos: u64 = file.stream_position().unwrap();

        for entry in entries {
            let mut buf = vec![0u8; entry.size as usize];
            file.seek(SeekFrom::Start(entry.offset as u64 + pos))
                .unwrap();
            file.read_exact(&mut buf).unwrap();
            map.insert(entry.path.clone(), buf);
        }

        map
    }

    pub fn save_pkg(&mut self, target: &Path, dry_run: bool, parse_tex: bool, verbose: bool) {
        for (path, bytes) in self.files.iter() {
            let mut path = target.join(path);
            if !dry_run {
                create_dir_all(path.parent().unwrap()).unwrap();
            }
            if path.extension().unwrap_or_default() == "tex" {
                if !parse_tex & !dry_run {
                    fs::write(path, bytes).unwrap();
                    continue;
                }
                let tex = tex_parser::Tex::new(bytes);
                let tex = match tex {
                    Some(val) => val,
                    None => {
                        println!(
                            "failed to parse path: {} \n",
                            path.to_str().unwrap_or_default()
                        );
                        continue;
                    }
                };

                if verbose {
                    println!("Texture:");
                    println!("Texv: {}", tex.texv);
                    println!("Texi: {}", tex.texi);
                    println!("Texb: {}", tex.texb);
                    println!("Image count: {}", tex.image_count);
                    println!("Mipmap count: {}", tex.mipmap_count);
                    println!("Lz4 compressed: {}", tex.lz4);
                    println!("Texture size: {}", tex.size);
                    println!("w: {} h: {}", tex.dimension[0], tex.dimension[1]);
                    println!();
                }

                let parsed = tex.parse_to_image();
                let parsed = match parsed {
                    None => {
                        println!(
                            "failed to parse image: {}\n",
                            path.to_str().unwrap_or_default()
                        );
                        continue;
                    }
                    Some(val) => val,
                };

                path.set_extension(&parsed.1);
                if parse_tex & !dry_run {
                    fs::write(path, parsed.0).unwrap();
                }
            } else {
                if !dry_run {
                    fs::write(path, bytes).unwrap();
                }
            }
        }
    }
}

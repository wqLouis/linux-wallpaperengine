use std::io::{BufReader, Cursor, Read};

use image::{ImageBuffer, Rgba};

#[derive(Debug, Clone)]
pub struct Tex {
    pub texv: String,
    pub texi: String,
    pub texb: String,
    pub size: u32,
    pub dimension: [u32; 2],
    pub image_count: u32,
    pub mipmap_count: u32,
    pub lz4: bool,
    pub decompressed_size: u32,
    pub extension: String,
    pub payload: Vec<u8>,
}

impl Tex {
    pub fn new(bytes: &Vec<u8>) -> Option<Tex> {
        const MAGIC: usize = 8;
        const SEP: i64 = 1;
        const TEX_SIZE: usize = 4;

        let mut buf = BufReader::new(Cursor::new(bytes));
        let mut extension: String;
        let is_lz4: bool;

        let mut texv = [0u8; MAGIC]; // I have no idea what all this magic variables are
        let mut texi = [0u8; MAGIC];
        let mut texb = [0u8; MAGIC];
        let mut size = [0u8; TEX_SIZE];
        let mut dimension = [[0u8; TEX_SIZE]; 2]; // w h
        let mut format = [0u8; TEX_SIZE];
        let mut image_count = [0u8; TEX_SIZE];
        let mut mipmap_count = [0u8; TEX_SIZE];
        let mut lz4 = [0u8; TEX_SIZE];
        let mut decompressed_size = [0u8; TEX_SIZE];
        let mut payload: Vec<u8>;

        buf.read_exact(&mut texv).ok()?;
        buf.seek_relative(SEP).ok()?;
        buf.read_exact(&mut texi).ok()?;
        buf.seek_relative(SEP).ok()?;
        buf.read_exact(&mut format).ok()?;
        buf.seek_relative(TEX_SIZE as i64).ok()?;
        buf.read_exact(&mut dimension[0]).ok()?;
        buf.read_exact(&mut dimension[1]).ok()?;
        buf.seek_relative((TEX_SIZE * 3) as i64).ok()?;
        buf.read_exact(&mut texb).ok()?;
        buf.seek_relative(SEP).ok()?;
        buf.read_exact(&mut image_count).ok()?;
        buf.seek_relative((TEX_SIZE * 2) as i64).ok()?;

        if String::from_utf8_lossy(&texb) == "TEXB0004" {
            buf.read_exact(&mut mipmap_count).ok()?;
        }

        buf.seek_relative(MAGIC as i64).ok()?;
        buf.read_exact(&mut lz4).ok()?;
        buf.read_exact(&mut decompressed_size).ok()?;
        buf.read_exact(&mut size).ok()?;

        is_lz4 = if u32::from_le_bytes(lz4) == 1 {
            true
        } else {
            false
        };

        payload = vec![0u8; u32::from_le_bytes(size) as usize];
        buf.read_exact(&mut payload).ok()?;
        extension = match u32::from_le_bytes(format) {
            0 => "raw".to_owned(),
            7 => "dxt1".to_owned(),
            4 | 6 => "dxt5".to_owned(),
            8 => "rg88".to_owned(),
            9 => "r8".to_owned(),
            _ => "tex".to_owned(),
        };

        extension = if extension == "raw" {
            Self::match_signature(&payload)
        } else {
            extension
        };

        if is_lz4 {
            payload = lz4_flex::block::decompress(
                &mut payload,
                u32::from_le_bytes(decompressed_size) as usize,
            )
            .ok()?;
        }

        Some(Tex {
            texv: String::from_utf8_lossy(&texv).into_owned(),
            texi: String::from_utf8_lossy(&texi).into_owned(),
            texb: String::from_utf8_lossy(&texb).into_owned(),
            size: u32::from_le_bytes(size),
            dimension: [
                u32::from_le_bytes(dimension[0]),
                u32::from_le_bytes(dimension[1]),
            ],
            image_count: u32::from_le_bytes(image_count),
            mipmap_count: u32::from_le_bytes(mipmap_count),
            lz4: is_lz4,
            decompressed_size: u32::from_le_bytes(decompressed_size),
            payload: payload,
            extension: extension,
        })
    }

    pub fn parse_to_image(&self) -> Option<(Vec<u8>, String)> {
        Some(match self.extension.as_str() {
            "r8" => Self::raw_to_png(
                self.payload.iter().flat_map(|&b| [b, b, b, 255]).collect(),
                self.dimension[0],
                self.dimension[1],
            )?,
            "rg88" => Self::raw_to_png(
                self.payload
                    .windows(2)
                    .flat_map(|b| [b[0], b[0], b[0], b[1]])
                    .collect(),
                self.dimension[0],
                self.dimension[1],
            )?,
            "dxt1" => Self::raw_to_png(
                bcndecode::decode(
                    &self.payload,
                    self.dimension[0] as usize,
                    self.dimension[1] as usize,
                    bcndecode::BcnEncoding::Bc1,
                    bcndecode::BcnDecoderFormat::RGBA,
                )
                .unwrap(),
                self.dimension[0],
                self.dimension[1],
            )?,
            "dxt5" => Self::raw_to_png(
                bcndecode::decode(
                    &self.payload,
                    self.dimension[0] as usize,
                    self.dimension[1] as usize,
                    bcndecode::BcnEncoding::Bc3,
                    bcndecode::BcnDecoderFormat::RGBA,
                )
                .unwrap(),
                self.dimension[0],
                self.dimension[1],
            )?,
            "jpg" => (self.payload.clone(), "jpg".to_owned()),
            "png" => (self.payload.clone(), "png".to_owned()),
            "mp4" => (self.payload.clone(), "mp4".to_owned()),
            _ => (self.payload.clone(), "tex".to_owned()),
        })
    }

    pub fn parse_to_rgba(&mut self) -> Option<()> {
        let parsed = match self.extension.as_str() {
            "png" => image::load_from_memory_with_format(&self.payload, image::ImageFormat::Png)
                .ok()?
                .into_rgba8()
                .as_raw()
                .to_owned(),
            "jpg" => image::load_from_memory_with_format(&self.payload, image::ImageFormat::Jpeg)
                .ok()?
                .into_rgba8()
                .as_raw()
                .to_owned(),
            "mp4" => self.payload.clone(),
            "rg88" => self.payload.clone(),
            "r8" => self.payload.clone(),
            "dxt1" => bcndecode::decode(
                &self.payload,
                self.dimension[0] as usize,
                self.dimension[1] as usize,
                bcndecode::BcnEncoding::Bc1,
                bcndecode::BcnDecoderFormat::RGBA,
            )
            .ok()?,
            "dxt5" => bcndecode::decode(
                &self.payload,
                self.dimension[0] as usize,
                self.dimension[1] as usize,
                bcndecode::BcnEncoding::Bc3,
                bcndecode::BcnDecoderFormat::RGBA,
            )
            .ok()?,
            _ => {
                return None;
            }
        };

        self.payload = parsed;

        Some(())
    }

    fn match_signature(bytes: &Vec<u8>) -> String {
        const PNG_SIG: ([u8; 8], &str) = ([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], "png");
        const JPG_SIG: ([u8; 3], &str) = ([0xff, 0xd8, 0xff], "jpg");
        const MP4_SIG: ([u8; 4], &str) = ([0x66, 0x74, 0x79, 0x70], "mp4");
        const PADDED_BITS: usize = 16;

        let mut padded_arr = [0u8; PADDED_BITS];
        let payload_len = std::cmp::min(PADDED_BITS, bytes.len());

        padded_arr[..payload_len].copy_from_slice(&bytes[..payload_len]);

        if padded_arr[..8] == PNG_SIG.0 {
            return PNG_SIG.1.to_owned();
        }
        if padded_arr[..3] == JPG_SIG.0 {
            return JPG_SIG.1.to_owned();
        }
        if padded_arr[4..8] == MP4_SIG.0 {
            return MP4_SIG.1.to_owned();
        }

        "tex".to_owned()
    }

    fn raw_to_png(bytes: Vec<u8>, w: u32, h: u32) -> Option<(Vec<u8>, String)> {
        let mut buf: Vec<u8> = Vec::new();
        let mut cur = Cursor::new(&mut buf);

        ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(w, h, bytes.to_owned())?
            .write_to(&mut cur, image::ImageFormat::Png)
            .ok()?;
        Some((buf, "png".to_owned()))
    }
}

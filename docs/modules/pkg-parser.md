# Package Parser (`src/pkg_parser/`)

A git submodule that parses Wallpaper Engine `.pkg` files and extracts/decodes their contents (textures, models, shaders, audio, etc.).

---

## Module Structure

```
src/pkg_parser/
└── src/pkg_parser/
    ├── mod.rs             # Module declarations
    ├── parser.rs          # .pkg file reader (Pkg struct)
    ├── tex_parser.rs      # .tex texture parser (Tex struct)
    ├── video_parser.rs    # Video/GIF format detection & frame extraction
    └── mdl_parser.rs      # .mdl puppet model parser (MdlFile struct)
```

---

## `parser` — Package File Reader

**File:** `parser.rs`

### `Pkg`

The top-level package container.

```rust
pub struct Pkg {
    pub header: Header,                    // Package metadata
    pub files: HashMap<String, Vec<u8>>,   // All files by path
}
```

### `Header`

```rust
pub struct Header {
    pub version: String,   // Version string of the package format
    pub file_count: u32,   // Number of files in the package
}
```

### `Pkg::new(pkg_path: &Path) -> Pkg`

Opens and reads a `.pkg` file:

1. **Reads header** — version string (prefixed with 4-byte length) and file count
2. **Reads entries** — for each file: path string (prefixed with 4-byte length), offset (4 bytes), size (4 bytes)
3. **Reads files** — sorts entries by offset for sequential reading, seeks to each offset + data_start, reads raw bytes

### `Pkg::save_pkg(&self, target, dry_run, parse_tex, parse_video, parse_mdl)`

Extracts package contents to a target directory. For supported file types, optionally parses into human-readable formats:

| File Type | `parse_*` flag | Output |
|-----------|---------------|--------|
| `.tex` | `parse_tex=true` | Decodes texture to PNG image, logs metadata |
| `.mp4` / `.webm` / `.gif` | `parse_video=true` | Saves as-is, also extracts GIF frames as PNG |
| `.mdl` | `parse_mdl=true` | Parses to JSON (`model.mdl.json`) and saves raw file |
| any other | (always saved) | Raw file bytes copied to output path |

When `dry_run=true`, only logs what would be written without creating any files.

---

## `tex_parser` — Texture Parser

**File:** `tex_parser.rs`

### `Tex`

Parsed `.tex` texture file.

```rust
pub struct Tex {
    pub texv: String,             // Magic version string (8 bytes)
    pub texi: String,             // Image block magic (8 bytes)
    pub texb: String,             // Data block magic (8 bytes): "TEXB0001" or "TEXB0004"
    pub size: u32,                // Payload size
    pub dimension: [u32; 2],      // Width and height
    pub image_count: u32,         // Number of images
    pub mipmap_count: u32,        // Number of mipmap levels (0 for TEXB0001)
    pub lz4: bool,                // Whether payload is LZ4-compressed
    pub decompressed_size: u32,   // Size after LZ4 decompression
    pub extension: String,        // Detected format: "r8", "rg88", "dxt1", "dxt5", "png", "jpg", "mp4", "gif", "tex"
    pub payload: Vec<u8>,         // (Decompressed) texture pixel data
}
```

### Format Detection and Decoding

The texture format is determined from a format field in the binary:

| Format ID | Extension | Description |
|-----------|-----------|-------------|
| 0 | auto-detected by `match_signature()` | Embedded PNG/JPG/MP4/GIF |
| 4, 6 | `dxt5` | BC3/DXT5 compressed |
| 7 | `dxt1` | BC1/DXT1 compressed |
| 8 | `rg88` | Two-channel 8-bit |
| 9 | `r8` | Single-channel 8-bit |

### `Tex::new(bytes: &[u8]) -> Option<Tex>`

Parses a `.tex` file from raw bytes:
1. Reads magic strings, format ID, dimensions
2. Reads image/mipmap counts (TEXB0004 has mipmap_count, TEXB0001 doesn't)
3. Reads LZ4 flag, decompressed size, payload size
4. Reads payload, LZ4-decompresses if flagged
5. Detects extension from format ID (uses magic byte signature for format 0)
6. Returns `None` on any read error

### `Tex::parse_to_image(&self) -> Option<(Vec<u8>, String)>`

Converts pixel data to a PNG image for extraction:
- R8 → expands to RGBA
- RG88 → expands to RGBA (R channel repeated, G as alpha)
- DXT1/5 → BCn decode via `bcndecode` crate
- PNG/JPG/MP4/GIF → passthrough

Returns `(image_bytes, extension_string)`.

### `Tex::parse_to_rgba(&mut self) -> Option<()>`

In-place conversion to RGBA for GPU upload:
- PNG → decode to RGBA via `image` crate
- JPG → decode to RGBA
- DXT1/5 → BCn decode to RGBA
- R8, RG88, MP4, GIF → kept as-is (no expansion; renderer uploads with correct GPU format)
- Unknown format → returns `None` if size doesn't match expected `w*h*4`

---

## `video_parser` — Video/GIF Parser

**File:** `video_parser.rs`

### `VideoFormat`

```rust
pub enum VideoFormat {
    Mp4,
    WebM,
    Gif,
    Unknown(String),
}
```

### `Video`

Parsed video or GIF data.

```rust
pub struct Video {
    pub data: Vec<u8>,
    pub format: VideoFormat,
    pub dimensions: Option<(u32, u32)>,
    pub frame_count: Option<u32>,
}
```

### `Video::new(bytes: &[u8]) -> Option<Video>`

Detects format from magic bytes and extracts info:
- **MP4:** `...ftyp` magic → `VideoFormat::Mp4`
- **WebM:** `0x1A45DFA3` EBML header → `VideoFormat::WebM`
- **GIF:** `GIF87a`/`GIF89a` → `VideoFormat::Gif` (extracts dimensions and frame count)

### Methods

| Method | Description |
|--------|-------------|
| `is_video()` | True for Mp4/WebM |
| `is_gif()` | True for Gif |
| `extract_frame(index)` | Extracts a single GIF frame as RGBA pixels (returns `None` for video) |
| `extract_all_frames()` | Extracts all GIF frames as RGBA pixel buffers (returns `None` for video) |

### `save_gif_frames(bytes, stem) -> Option<Vec<(Vec<u8>, String)>>`

Saves all frames of a GIF as separate PNG files. Returns `[(png_bytes, "stem_frame_0000.png"), ...]`.

---

## `mdl_parser` — Puppet Model Parser

**File:** `mdl_parser.rs`

### `MdlFile`

Parsed MDL (puppet model) file with three sections.

```rust
pub struct MdlFile {
    pub header: MdlvHeader,   // MDLV0023 section header
    pub data: MdlvData,       // Control points and triangles
    pub bones: Bones,         // MDLS skeleton/bone section
    pub animation: Animation, // MDLA animation section
}
```

### `MdlvHeader`

```rust
pub struct MdlvHeader {
    pub magic: String,          // "MDLV0023"
    pub type_val: u32,
    pub sub_version: u16,
    pub flags: u16,
    pub unknown_16: u32,
    pub material_path: String,  // Null-terminated path
    pub header_size: usize,     // Size until data marker (0x80000F00)
}
```

### `MdlvData`

```rust
pub struct MdlvData {
    pub marker_type: u32,           // 0x80000F00
    pub record_block_size: u32,
    pub records: Vec<ControlPoint>, // 80-byte control point records
    pub triangles: Vec<Triangle>,   // Index triplets (u16×3)
}
```

### `ControlPoint`

80-byte record with position, UV, and group data:

```rust
pub struct ControlPoint {
    pub index: u32,
    pub pos_x: i16, pub pos_y: i16,  // Position coordinates
    pub tex_u: i16, pub tex_v: i16,  // Texture coordinates
    pub group_id: u32,
    pub sub_group: u32,
    pub sub_sub_group: u32,
    // Various unknown/partially-understood fields
    pub field_0: u32, pub field_4: u32, pub field_12: u32, pub field_16: u32,
    pub field_28: f32, pub field_56: f32, pub field_60: u32,
    pub field_72: u32, pub field_76: f32,
}
```

### `Triangle`

```rust
pub struct Triangle {
    pub a: u16, pub b: u16, pub c: u16,  // Vertex indices
}
```

### `Bones`

```rust
pub struct Bones {
    pub header: String,             // "MDLS"
    pub bones: Vec<BoneEntry>,
}

pub struct BoneEntry {
    pub index: u32,
    pub tmp: u8,
    pub bone_type: u32,
    pub unk1: u32,
    pub matrix: [f32; 16],         // 4×4 transformation matrix
    pub info: String,              // Bone name/description
}
```

### `Animation`

```rust
pub struct Animation {
    pub header: String,             // "MDLA"
    pub end_offset: u32,
    pub num_animations: u32,
    pub num_frames: u32,
    pub animation_name: String,
    pub loop_mode: String,          // e.g. "loop", "once"
    pub animation_data: Vec<u8>,    // Raw animation keyframe data
}
```

### `MdlFile::new(bytes: &[u8]) -> Option<MdlFile>`

Parses an MDL file by finding the three section markers (`MDLS`, `MDLA`) via byte scanning:

1. **Header:** Reads magic, type, sub-version, flags, material path. Finds data start by locating the `0x00 0x0F 0x00 0x80` marker.
2. **Data:** At the marker (0x80000F00), reads record block size, parses 80-byte control point records, then parses triangles from the gap between records_end and the MDLS marker.
3. **Bones (MDLS):** Reads bone count, bone entries with 4×4 matrices and names.
4. **Animation (MDLA):** Reads frame count, animation name, loop mode, and raw data.

### `MdlFile::to_json() -> Result<String>`

Serializes the entire model to pretty-printed JSON.

### `MdlFile::to_json_compact() -> Result<String>`

Serializes to compact JSON.

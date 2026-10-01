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
    └── mdl_parser/        # .mdl puppet model parser (MdlFile struct)
        ├── mod.rs             # Section walker + MdlFile
        ├── reader.rs          # Bounds-checked little-endian readers
        ├── header.rs          # MDLV header
        ├── mesh.rs            # Control points, triangles, index batches
        ├── skeleton.rs        # MDLS bones
        ├── attachments.rs     # MDAT sockets
        ├── animation.rs       # MDLA clips, tracks, keyframes
        └── bone_matrices.rs   # MDLE matrices
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

**Directory:** `mdl_parser/` (one module per MDL section)

An `.mdl` file is a chain of null-terminated sections: `MDLV` (header +
mesh), `MDLS` (skeleton), and optionally `MDAT` (attachments), `MDLA`
(animation) and `MDLE` (bone matrices), ending in a single `0x00` sentinel
byte. Every section except `MDLV` is optional. The byte-level format is
documented in `src/pkg_parser/src/pkg_parser/README.md`.

| Module | Section | Contents |
|--------|---------|----------|
| `header.rs` | MDLV | Magic, type/sub-version fields, material path |
| `mesh.rs` | MDLV | 80-byte control points, triangles, index batches, secondary positions |
| `skeleton.rs` | MDLS | Bone hierarchy, bind-pose 4×4 matrices, JSON info, names |
| `attachments.rs` | MDAT | Named sockets parented to bones (4×4 transform) |
| `animation.rs` | MDLA | Clips → per-bone tracks → dense keyframes (36 bytes each) |
| `bone_matrices.rs` | MDLE | One 4×4 matrix per bone |
| `reader.rs` | — | Bounds-checked little-endian readers shared by all of the above |

### `MdlFile`

Parsed MDL (puppet model) file.

```rust
pub struct MdlFile {
    pub header: MdlvHeader,          // MDLV header
    pub data: MdlvData,              // Control points, triangles, batches
    pub bones: Bones,                // MDLS skeleton (+ undecoded trailing)
    pub attachments: Attachments,    // MDAT sockets (empty if absent)
    pub animation: Animation,        // MDLA clips (empty if absent)
    pub bone_matrices: BoneMatrices, // MDLE matrices (empty if absent)
}
```

### `MdlvHeader`

```rust
pub struct MdlvHeader {
    pub magic: String,          // "MDLV0021" / "MDLV0023"
    pub type_val: u32,          // 0x01800009
    pub sub_version: u16,
    pub flags: u16,
    pub unknown_16: u32,
    pub material_path: String,  // Null-terminated path
    pub header_size: usize,     // Bytes consumed by the header (string ends it)
}
```

### `MdlvData`

```rust
pub struct MdlvData {
    pub records: Vec<ControlPoint>,   // 80-byte control points
    pub triangles: Vec<Triangle>,     // u16 triplets indexing `records`
    pub index_batches: Vec<IndexBatch>, // Draw batches partitioning `triangles`
    pub alt_positions: Vec<[f32; 3]>, // Optional secondary vertex positions
}
```

### `ControlPoint`

80-byte record; the four skinning slots always sum to `1.0`.

```rust
pub struct ControlPoint {
    pub index: u32,
    pub pos_x: f32, pub pos_y: f32, pub pos_z: f32,
    pub bones: [u32; 4],    // Bone index per skinning slot
    pub weights: [f32; 4],  // Weights, sum == 1.0
    pub tex_u: f32, pub tex_v: f32,
}
```

### `Bones` / `BoneEntry`

```rust
pub struct Bones {
    pub header: String,        // "MDLS0004"
    pub next_offset: u32,      // Start of the next section
    pub bones: Vec<BoneEntry>,
    pub trailing: Vec<u8>,     // Undecoded metadata between bones and next section
}

pub struct BoneEntry {
    pub index: u32,
    pub bone_type: u32,
    pub parent_index: u32,     // 0xFFFFFFFF = root
    pub matrix: [f32; 16],     // Bind pose, row-major, translation in row 3
    pub info: String,          // JSON metadata (tp = pin position, tm = multiplier)
    pub name: String,          // e.g. "legs" (often empty)
}
```

### `Animation`

```rust
pub struct Animation {
    pub header: String,          // "MDLA0006"
    pub end_offset: u32,         // Start of the next section
    pub num_animations: u32,
    pub num_frames: u32,         // Opaque section field (not the timeline length)
    pub animation_name: String,  // First clip's name (compat)
    pub loop_mode: String,       // First clip's loop mode (compat)
    pub clips: Vec<AnimationClip>,
}

pub struct AnimationClip {
    pub name: String,
    pub loop_mode: String,       // "loop"
    pub fps: f32,
    pub frame_count: u32,        // Tracks hold frame_count + 1 keyframes
    pub tracks: Vec<Track>,      // One track per bone, in bone order
}

pub struct Track { pub keyframes: Vec<Keyframe> }

pub struct Keyframe { pub tx: f32, pub ty: f32, pub tz: f32,
                      pub rx: f32, pub ry: f32, pub rz: f32,
                      pub sx: f32, pub sy: f32, pub sz: f32 }
```

Keyframes are dense (one per frame, 36 bytes = translation + rotation in
radians + scale); frame 0 reproduces the bone's bind pose.

### `Attachments` / `BoneMatrices`

```rust
pub struct Attachments {
    pub header: String,       // "MDAT0001"
    pub next_offset: u32,
    pub entries: Vec<Attachment>,  // bone_index + name + 4×4 transform
}

pub struct BoneMatrices {
    pub header: String,       // "MDLE0002"
    pub end_offset: u32,
    pub matrices: Vec<[f32; 16]>,  // One per bone
}
```

### `MdlFile::new(bytes: &[u8]) -> Option<MdlFile>`

Parses an MDL file from raw bytes. **Strict** — returns `None` when the
bytes are not an MDL (bad magic), when the mesh block isn't exactly where
the header says, or when a present section doesn't match the documented
layout. Absent sections are skipped (empty default); a truncated buffer
never panics.

1. **Header (MDLV):** validates the magic and reads the material path.
2. **Mesh:** reads the block at the offset derived from the header
   (verified by its `0x0180000F` tag — no byte scan), then parses the
   trailer (secondary positions + index batches), whose end is the exact
   offset of the next section.
3. **Section walk:** `MDLS` → `MDAT` → `MDLA` → `MDLE` are parsed at the
   offsets the format provides (each section carries the next offset);
   a model may legitimately lack any of them. An unknown magic ends the
   walk instead of being searched for.

### `MdlFile::to_json() -> Result<String>`

Serializes the entire model to pretty-printed JSON.

### `MdlFile::to_json_compact() -> Result<String>`

Serializes to compact JSON.

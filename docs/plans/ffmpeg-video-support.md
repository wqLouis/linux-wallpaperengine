# Plan — Video assets & video wallpapers (ffmpeg_next in `pkg_parser`)

Status: **planned** (no code written yet)
Scope: `src/pkg_parser` (git submodule, repo `wqlouis/pkg_parser`, branch `crate`) + the host crate `linux-wallpaperengine`.

---

## 1. Current state (what exists today)

### `pkg_parser` (submodule)

| File | State |
|---|---|
| `parser.rs` | `Pkg` container: header + `FileMap` (path → bytes). `save_pkg()` dispatches per-extension. |
| `tex_parser.rs` | `Tex` — parses `.tex`. Format table includes `"mp4"` and `"gif"` payloads (WE stores **video textures as a `.tex` with an embedded mp4**), but `parse_to_rgba()` leaves those payloads untouched. |
| `video_parser.rs` | `Video` — magic-byte detection (mp4/webm/gif) + **GIF-only** frame extraction via the `image` crate. mp4/webm are opaque bytes (`extract_frame()` returns `None`). |
| `mdl_parser/` | puppet models (unrelated). |

Dependencies: `ahash, bcndecode, hashbrown, image, log, lz4_flex, serde, serde_json`. **No ffmpeg.** No `project.json` handling.

### Host crate

| Area | State |
|---|---|
| `loader/assets_loader.rs` | `AssetType::Video` (mp4/webm/gif/avi) → `Asset::Video(Vec<u8>)`. **No accessor, never consumed** — video assets are parsed, copied into RAM, and dropped on the floor. |
| `loader/object_loader.rs` | `resolve_texture()` resolves **only** `materials/{name}.tex`. A material pointing at a video never resolves → object is skipped. |
| `renderer/draw.rs` | `upload_texture()` maps `Tex.extension` → wgpu format; `"mp4"` falls into the `_ => Rgba8UnormSrgb` arm, i.e. **raw mp4 bytes would be uploaded as RGBA** (size mismatch → wgpu validation panic). |
| `renderer/app.rs` | `render()` has exactly one per-frame CPU update hook (`draw::update_animated_meshes`) — the natural place for a video frame pump. |
| `loader/scene_loader.rs` | `Scene::new()` **panics** if `scene.json` is missing → a *video wallpaper* (no `scene.json`) cannot even load. |

### Gaps

1. No real video decoding anywhere (GIF only).
2. No wallpaper-type detection (`project.json`) → only *scene* wallpapers load.
3. No video → GPU path (no per-frame upload, no video texture support inside scenes).
4. `pkg_parser` has no notion of "what kind of wallpaper is this".

### Environment (verified)

* FFmpeg **9.0.2** installed: `libavcodec 63.1.102`, `libavformat 63.1.102`, `libavutil 61.1.102`, `libswscale 10.1.102`, `libswresample 7.1.102`.
* `ffmpeg-next 9.0.0` + `ffmpeg-sys-next 9.0.1` are in the local cargo registry → **offline build works**; `ffmpeg-sys-next 9.0.1` declares `ffmpeg_9_0`, matching the system FFmpeg.
* Verified working decode chain (scratch crate, 320×240 h264 mp4 → **28 frames**, RGBA out):

```rust
ffmpeg::init()?;
let io = ffmpeg::format::context::StreamIo::from_read_seek(Cursor::new(bytes))?;
let mut ictx = ffmpeg::format::input_from_stream(io, Some("video.mp4"), None)?;
let vstream = ictx.streams().best(ffmpeg::media::Type::Video)?;
let ctx = ffmpeg::codec::context::Context::from_parameters(vstream.parameters())?;
let mut decoder = ctx.decoder().open()?.video()?;          // decoder::Video
let mut scaler = ffmpeg::software::scaling::Context::get(
    decoder.format(), decoder.width(), decoder.height(),
    ffmpeg::format::Pixel::RGBA, out_w, out_h,
    ffmpeg::software::scaling::Flags::BILINEAR)?;
let mut out = ffmpeg::util::frame::video::Video::new(ffmpeg::format::Pixel::RGBA, out_w, out_h);
let mut frame = ffmpeg::util::frame::video::Video::empty();
for (stream, packet) in ictx.packets() {
    if stream.index() != vindex { continue; }
    decoder.send_packet(&packet)?;
    while decoder.receive_frame(&mut frame).is_ok() {
        scaler.run(&frame, &mut out)?;                        // → RGBA8
    }
}
```

Gotchas to carry into the implementation: `Codec::from_parameters` does **not** exist (it is
`codec::context::Context::from_parameters` → `.decoder()` → `.open()` → `.video()`);
`receive_frame` returns `Err(Error::Eof)`/`Err(Error::Other{errno:EAGAIN})` when drained —
the loop must distinguish "needs another packet" from "end of stream"; the last frame(s) need an
explicit `send_eof()` + drain pass.

---

## 2. Goal

1. **pkg_parser owns video decoding** (ffmpeg_next): metadata + on-demand RGBA frames, exposed
   through a small, self-contained API so the host crate never links ffmpeg directly.
   The codec sits behind a `VideoBackend` trait (ffmpeg is *one* implementation, behind an
   optional cargo feature), so the core stays pure Rust and the crate remains buildable for
   `wasm32-unknown-unknown` — see section 3.8.
2. **The loader owns wallpaper-type detection**: the host crate parses `project.json` →
   `Scene | Video | Web | Application`. `pkg_parser` stays a pure format parser.
3. **Host crate renders video wallpapers** (fullscreen, looping) and **video textures inside scenes**
   (material → `.tex` with mp4 payload, or a standalone `.mp4` in `materials/`).

Non-goals: audio extraction from mp4 (phase 3+, optional), HDR/`yuv420p10` colour management
(pass-through, swscale to RGBA8), video *effects* on top of a video layer (the existing effect
chain already works on any texture, so it comes for free once the texture is a video).

---

## 3. Target design

### 3.1 `pkg_parser`: new module layout

```
src/pkg_parser/src/pkg_parser/
├── mod.rs                 # + pub mod video_parser;
├── parser.rs              # + Pkg::from_bytes()
├── tex_parser.rs          # + Tex::video_payload() / Tex::is_video()
└── video_parser/
    ├── mod.rs             # public façade (keeps the legacy Video/VideoFormat API)
    ├── info.rs            # VideoInfo, VideoFrame, VideoError, VideoFormat
    ├── backend.rs         # VideoBackend trait + BackendKind + detect_format()
    ├── player.rs          # VideoPlayer — backend-agnostic frame scheduler
    ├── gif.rs             # GIF backend (pure Rust, `image` crate)
    └── ffmpeg/mod.rs      # ffmpeg_next backend (cargo feature `ffmpeg`)
```

**The backend trait is the seam.** Everything except `video_parser/ffmpeg/` is pure Rust with no
codec dependency — that is what keeps a future browser (WASM) build possible.

### 3.2 `pkg_parser`: public API

```rust
// video_parser::info
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub duration: f64,      // seconds; 0.0 when unknown
    pub fps: f64,           // frames per second; 0.0 when unknown
    pub frame_count: u32,   // best estimate (counted when cheap, else duration*fps)
    pub codec: String,      // "h264", "vp9", …
    pub pixel_format: String,
    pub has_audio: bool,
}

pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub time: f64,          // presentation time in seconds
    pub duration: f64,      // how long the frame is shown (best effort)
    pub rgba: Arc<[u8]>,    // width*height*4, straight alpha; Arc so cached frames are not copied
}

pub enum VideoError { Unsupported(String), Decode(String), Io(String) }

// video_parser::backend
/// A pluggable video decoding backend.
///
/// The trait is deliberately *sequential* (next frame / rewind) so that any implementation —
/// ffmpeg on native targets, a WebCodecs bridge in the browser — can satisfy it. Random
/// access is implemented by the player on top of it.
pub trait VideoBackend {
    fn name(&self) -> &'static str;
    fn probe(&mut self) -> Result<VideoInfo, VideoError>;
    fn next_frame(&mut self) -> Result<Option<VideoFrame>, VideoError>;
    fn rewind(&mut self) -> Result<(), VideoError>;
    fn set_output_size(&mut self, _width: u32, _height: u32) {}
}

pub enum BackendKind { Auto, Ffmpeg, Gif }
pub fn detect_format(bytes: &[u8]) -> Option<VideoFormat>;

// video_parser::player
/// Backend-agnostic frame scheduler: maps a wall-clock time to a decoded frame,
/// caches recently decoded frames, and loops at the end of the stream.
pub struct VideoPlayer { /* backend + ring cache + cursor */ }

impl VideoPlayer {
    pub fn open(bytes: &[u8]) -> Result<Self, VideoError>;
    /// Extension point for alternative backends (e.g. WebCodecs in a browser build).
    pub fn open_with_backend(bytes: &[u8], backend: Box<dyn VideoBackend>) -> Result<Self, VideoError>;
    pub fn open_with_backend_kind(bytes: &[u8], kind: BackendKind) -> Result<Self, VideoError>;
    pub fn info(&self) -> &VideoInfo;
    pub fn backend_name(&self) -> &'static str;
    pub fn set_output_size(&mut self, width: u32, height: u32);
    /// The frame covering `time_secs`; sequential playback is O(1) amortised, a backwards
    /// jump rewinds and re-decodes. `Ok(None)` when the stream is exhausted.
    pub fn frame_at(&mut self, time_secs: f64) -> Result<Option<VideoFrame>, VideoError>;
    pub fn reset(&mut self) -> Result<(), VideoError>;
}
```

Design rules:

* **The backend trait is sequential on purpose** (decode the next frame / rewind) — every
  implementation satisfies it, and the player adds random access on top.
* **No `Vec<u8>` copy of the container** — the backend owns the bytes behind `Arc<[u8]>`; ffmpeg
  reads them through `Cursor<Arc<[u8]>>` (no payload copy).
* **`ffmpeg::init()`** is called lazily behind a `std::sync::Once` — ffmpeg global state is
  process-wide, and the host crate renders on one thread.
* **Frame scheduling**: monotonic playback is the common case, so the player keeps a cursor and
  decodes forward; a backwards jump or a loop rewind restarts the backend. A small ring cache
  (default 4 frames) avoids re-decoding when the compositor asks for the same frame twice.
* **Scaling at decode time**: the backend converts straight to `RGBA` **at the requested output
  size** (screen resolution), which keeps both CPU and memory bounded — a 4K video decoded at 4K
  would be 33 MB per frame.
* **GIF keeps the `image`-crate path** (no behaviour change, no ffmpeg dependency for GIF-only
  content); ffmpeg is used for mp4/webm/avi.

### 3.3 Host crate: wallpaper kind (`project.json`)

`project.json` is a *wallpaper* concept, not an archive-format concept, so it belongs to the
loader — `pkg_parser` stays a pure format parser.

```rust
// src/scene/loader/project.rs
pub enum WallpaperKind { Scene, Video, Web, Application, Unknown(String) }

pub struct Project {
    pub title: String,
    pub kind: WallpaperKind,
    /// Main file of the wallpaper ("scene.json" / "video.mp4"), when present.
    pub file: Option<String>,
    pub preview: Option<String>,
}

pub fn parse_project(json: &str) -> Option<Project>;

// src/scene/loader/assets_loader.rs
impl AssetStore {
    /// The wallpaper's project.json, if the package has one.
    pub fn project(&self) -> Option<Project>;
}

// src/scene/loader/scene_loader.rs
pub struct Scene {
    pub root: Root,
    pub assets: AssetStore,
    pub project: Option<Project>,
}
```

Lenient parsing (unknown fields ignored, missing `type` → `Scene`, a non-object JSON → `None`).
The manifest is read through the asset store, so it also works for the lazy-loading fallback
directory.

### 3.4 Host crate: asset layer

```rust
// loader/assets_loader.rs
pub enum Asset {
    …
    Video(Rc<VideoAsset>),   // was Video(Vec<u8>)
}

pub struct VideoAsset {
    pub bytes: Rc<[u8]>,
    pub info: VideoInfo,      // parsed lazily on first use
}

impl AssetStore {
    pub fn video(&self, key: &str) -> Option<Rc<VideoAsset>>;
}
```

`VideoAsset` keeps the bytes behind `Rc` (no per-asset copy) and parses metadata on first
access (ffmpeg probe of a 50 MB file at load time is wasteful if the asset is never shown).

### 3.5 Host crate: texture source

```rust
// loader/object_loader.rs
pub enum TextureSource {
    Static(Rc<Tex>),
    Video(Rc<VideoAsset>),
}

pub struct TextureObject {
    …
    pub source: TextureSource,   // replaces the bare texture: Rc<Tex>
}
```

Resolution order in `resolve_texture()`:

1. `materials/{name}.tex` whose payload is an mp4/webm → `TextureSource::Video`.
2. `materials/{name}.mp4` (standalone video asset) → `TextureSource::Video`.
3. `materials/{name}.tex` (normal) → `TextureSource::Static`.
4. otherwise → `None` (object skipped, as today).

### 3.6 Host crate: renderer

```rust
// renderer/draw.rs
pub struct VideoDrawState {
    pub texture: wgpu::Texture,          // Rgba8UnormSrgb, 1 mip, COPY_DST | TEXTURE_BINDING
    pub decoder: VideoDecoder,
    pub last_uploaded: Option<f64>,      // avoid re-uploading an unchanged frame
}

// new, called from app::render() next to update_animated_meshes()
pub fn update_video_textures(queue: &wgpu::Queue, draw_queue: &DrawQueue, elapsed: f32);
```

* The bind group is created **once** (the `wgpu::Texture` object never changes) → per-frame work
  is only `decoder.frame_at(t)` + `queue.write_texture`.
* Loop: `t = elapsed % duration`.
* Guard: `upload_texture()` must **not** treat an `"mp4"` `Tex.extension` as RGBA (today it would
  panic in wgpu validation) — video textures take the new path.
* Mipmaps: video textures are uploaded without a mip chain (level 0 only) — the sampler in
  `image.wgsl`/`post_process` must be configured with a matching mipmap filter
  (`mipmap_filter: Nearest` for video textures).

### 3.7 Host crate: video wallpaper mode

`Scene::new()` currently panics without `scene.json`. New entry point:

```rust
// loader/scene_loader.rs
pub enum Wallpaper {
    Scene(Scene),
    Video(VideoWallpaper),
}

pub struct VideoWallpaper {
    pub video: Rc<VideoAsset>,
    pub info: VideoInfo,
}
```

Loading flow in `renderer/load.rs`:

```
Pkg::new(path)
  → Pkg::project()?
      • kind == Video → video wallpaper: build a synthetic scene
      • otherwise      → Scene::new(...) as today
```

**Synthetic scene** (chosen over a special-cased renderer path, so the entire existing pipeline —
fit modes, projection, effects, adapters — keeps working):

* one `Image` object at `origin 0 0`, `size = projection width/height`, referencing a synthetic
  material whose texture is the video asset,
* `general.orthogonalprojection = {width, height}` = the video's native resolution (or the
  output size),
* `general.clearcolor` = black.

The existing `resolve_texture()` then resolves the synthetic material to the video asset via the
new video branch — one code path for "video wallpaper" and "video layer inside a scene".

---

### 3.8 WASM / browser target (a design constraint, not a later refactor)

The crate — and eventually the whole engine — must stay compilable for
`wasm32-unknown-unknown` so it can run in a browser (wgpu → WebGPU, `pkg_parser` → WASM).
The rules that keep that possible:

* **The core never mentions ffmpeg.** `ffmpeg_next` is an *optional* dependency behind the
  `ffmpeg` feature, lives only in `video_parser/ffmpeg/`, and no `ffmpeg_next` type appears in
  any public signature.
* **No filesystem in the core.** `Pkg::from_bytes(&[u8])` is the primitive; `Pkg::new(path)` is a
  thin wrapper over it. In the browser the archive is fetched over HTTP and parsed from bytes.
* **No threads in the core.** The player is single-threaded by design (the ffmpeg backend is
  `Send`, but the player is never shared between threads).
* **The browser gets its own backend.** `VideoBackend` is the extension point: a future
  `WebCodecsBackend` (wasm-bindgen over the browser's `VideoDecoder`) implements
  `probe`/`next_frame`/`rewind` and plugs in through `VideoPlayer::open_with_backend`.
  The player, the loader and the renderer stay *identical* in both builds.
* **Feature flags mirror the platforms:** `ffmpeg` (default **on**) = the native backend;
  `webcodecs` (future, wasm only) = the browser backend; `--no-default-features` = no codec
  backend at all (GIF only, pure Rust).
* The renderer therefore talks to `VideoPlayer`, never to ffmpeg.

Guard rail: a CI check `cargo check --target wasm32-unknown-unknown --no-default-features`
for the parser crate, so a native-only dependency cannot sneak into the core.

#### Known wasm blockers (found while probing)

* **`ahash` → `getrandom` — resolved.** `cargo check --target wasm32-unknown-unknown` on the
  parser crate failed with *"The wasm32-unknown-unknown targets are not supported by default"*:
  `ahash 0.8` enables its `runtime-rng` feature by default, which pulls in `getrandom 0.3`.
  This project does not need a random (let alone cryptographic) seed — the hash tables index
  assets, effects and shader symbols, they are not a security boundary — so `ahash` is now used
  with a **fixed seed** in both crates:

```toml
ahash = { version = "0.8.12", default-features = false, features = ["std", "no-rng"] }
```

  This drops the `getrandom` dependency entirely and makes
  `cargo check --target wasm32-unknown-unknown --no-default-features` pass for the parser crate.
  The same reasoning applies to the host crate (`src/collections.rs`) when the engine itself is
  ported to the browser.
* **`ffmpeg_next` cannot build for wasm** — it is exactly why the backend lives behind a trait.
* The `image` crate (`gif`/`png`/`jpg` decoders) is pure Rust and builds for wasm ✓.


## 4. Work packets

> **Progress:** W2 merged — the manifest parser lives in the **host loader**
> (`src/scene/loader/project.rs`); the parser crate only gained `Pkg::from_bytes()`
> (submodule commit `bda1017`). W1 (the pluggable video backend) is in flight.

Each packet is independent, has an explicit file scope, and hands off a patch.

### Phase 1 — `pkg_parser` (submodule; own repo/branch)

| ID | Scope | Deliverable | Acceptance |
|----|-------|-------------|------------|
| **W1** | `pkg_parser/src/pkg_parser/video_parser/**`, `Cargo.toml` | `ffmpeg-next = "9.0"` as an **optional** dep (feature `ffmpeg`, default on); `VideoBackend` trait + `BackendKind`; `VideoPlayer` (backend-agnostic frame scheduler with ring cache); `VideoInfo`/`VideoFrame`/`VideoError`; GIF backend (pure Rust) + ffmpeg backend (in-memory IO, swscale→RGBA) | `cargo test` passes **with and without** `--no-default-features`; a mock backend drives `VideoPlayer` (proves the seam); the committed mp4 fixture decodes to ≥3 frames of the right size; the legacy `Video`/`save_gif_frames` API is unchanged |
| **W2** | **host** `src/scene/loader/project.rs`, `assets_loader.rs`, `scene_loader.rs`; **parser** `parser.rs` | loader: `WallpaperKind`, `Project`, `parse_project()`, `AssetStore::project()`, `Scene.project`. parser: `Pkg::from_bytes()` (the WASM entry point) | unit tests: `{"type":"Video","file":"v.mp4"}` → `Video`; missing `type` → `Scene`; malformed JSON → `None`; `from_bytes` produces the same `Pkg` as `new` |
| **W3** | `pkg_parser/src/pkg_parser/tex_parser.rs`, `video_parser/mod.rs` | `Tex::video_payload()`, `Tex::is_video()`; `Video::new` upgraded to fill `dimensions`/`frame_count` for mp4/webm via ffmpeg; keep the old `Video` fields/API source-compatible | existing gif tests still pass; `Video::new(mp4).dimensions == Some((w,h))` |

Fixture: a tiny (≈2 KB) 2-frame h264 mp4 committed under
`pkg_parser/tests/data/` (generated with the system `ffmpeg` CLI), so CI needs no codec.

### Phase 2 — host crate

| ID | Scope | Deliverable | Acceptance |
|----|-------|-------------|------------|
| **W4** | `src/scene/loader/assets_loader.rs` | `Asset::Video(Rc<VideoAsset>)`, `AssetStore::video()`, no byte copy | build + clippy clean; existing asset tests pass |
| **W5** | `src/scene/renderer/draw.rs`, `app.rs` | `VideoDrawState`, `update_video_textures()`, video branch in `upload_texture()` | a scene with a video layer renders and advances frames at the video's own fps |
| **W6** | `src/scene/loader/scene_loader.rs`, `object_loader.rs`, `renderer/load.rs` | `Wallpaper` enum, synthetic video scene, video branch in `resolve_texture()` | a video-wallpaper `.pkg` renders fullscreen and loops; a scene `.pkg` still renders exactly as before |
| **W7** | `src/main.rs`, `pkg_parser/src/pkg_parser/parser.rs` (`save_pkg`) | `--parse-video` now extracts real frames (PNG) + a metadata JSON for mp4/webm; `--list` shows the wallpaper kind | CLI smoke test on a video `.pkg` |

### Phase 3 — polish (optional)

* mp4 audio track → rodio (`symphonia-isomp4` is already enabled in the host `Cargo.toml`).
* `--no-video` runtime switch to skip video decoding entirely.
* Doc updates: `docs/modules/pkg-parser.md` (new module + API), `docs/modules/loader.md`,
  `docs/modules/overview.md` (new "Wallpaper kinds" section), README build requirements.

---

## 5. Risks & mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| **System FFmpeg dev packages required** to build (`libavcodec-dev` …) | build breaks on user machines | cargo feature `video` (default on) in `pkg_parser`; `--no-default-features` builds without ffmpeg; document the apt/dnf packages in README + `packaging/` |
| ffmpeg global state / thread-safety | crashes if decoding from several threads | decode on the render thread only; `ffmpeg::init()` behind `Once`; document `VideoDecoder` as `Send` but not `Sync` |
| Memory: RGBA frames are 8 MB at 1080p | OOM on small machines | decode directly to the output resolution; ring cache of 4 frames; optional downscale to ≤ 1080p |
| CPU: software decode + scale per frame | dropped frames on 4K | decode at output size, `BILINEAR` scaler, reuse the scaler context, skip upload when the frame did not change |
| Timestamps: variable frame rate / missing pts | stutter or wrong frame | use pts × time_base when available, fall back to nominal fps; clamp to `duration` |
| `.tex` payloads that are mp4 uploaded as RGBA today | wgpu validation panic | fix in W5 (`upload_texture` must not treat `"mp4"` as RGBA) |
| Scene `project.json` absent (older/hand-made pkgs) | wallpaper kind unknown | fall back to "scene" when `project.json` is missing; only switch to video mode on an explicit `"type":"Video"` |
| A native-only dependency sneaks into the core | the future browser build dies late | the core may not reference `ffmpeg_next` at all; `ffmpeg-next` is optional behind the `ffmpeg` feature; CI checks `cargo check --target wasm32-unknown-unknown --no-default-features` |
| The host crate starts depending on `ffmpeg_next` directly | the WASM build of the engine breaks | the host talks to `VideoPlayer` only; ffmpeg types never cross the crate boundary |
| Submodule workflow | changes land in a separate repo | work on a branch in `wqlouis/pkg_parser` (`crate` branch), then bump the submodule pointer in the host repo — keep the host-side patches rebased on top |
| Colour space / HDR videos | washed-out or crushed colours | swscale to RGBA8 with default (bt601/bt709 auto) conversion; HDR (`yuv420p10`) is out of scope, log a warning |

---

## 6. Verification plan

1. `cargo test -p pkg_parser` — decoder, project parser, tex-video payload, existing gif/mipmap/mdl tests.
2. `cargo clippy --all-targets` + `cargo build --release` for both crates.
3. **Fixture-driven** end-to-end: build a synthetic video-wallpaper `.pkg` (a tiny mp4 +
   `project.json`) and assert the renderer produces N distinct frames (frame checksum changes
   over time, loops back to the first frame).
4. **Regression**: an existing scene `.pkg` renders pixel-identically to before the change
   (the video path must not touch the static texture path).
5. `cargo build --no-default-features` (without ffmpeg) still compiles the host crate.

---

## 7. Open questions

1. **Frame budget vs. compositor**: the host renders as fast as possible (`--target-fps` unset);
   a video layer should be pumped at the *video's* fps, not per compositor frame — decouple the
   decode cadence from the render cadence.
2. **Video layer fitting**: WE video textures honour the material's `uv`/`fit` settings; the
   first implementation uses the same `cover/contain/stretch` fit modes as the wallpaper.
3. **Audio in video wallpapers**: mp4 audio via rodio/symphonia (phase 3) or ignore for now?
4. Should the **submodule** keep the ffmpeg dependency, or should the *host* crate own it and pass
   decoded frames into `pkg_parser`? (Current plan: submodule owns it — it is the parser's job and
   keeps the host free of ffmpeg types.)

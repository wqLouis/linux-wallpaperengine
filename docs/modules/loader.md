# Scene Loader (`src/scene/loader/`)

Parses Wallpaper Engine wallpapers and converts raw scene data into structured objects ready for rendering.

A wallpaper is either an **archive** (a `.pkg` file) or a **folder** (the
Steam workshop layout: a `project.json` and the files of the wallpaper next to
it).  Both shapes end up as the same map of files — the path of a file inside
the wallpaper and its bytes — so the loader does not care where they come
from.  The abstraction that does the reading lives in the façade
([engine.md](engine.md#sources--wallpapersource)); the loader only sees the
files.

---

## `scene` — Scene Root & Core Types

**File:** `scene.rs`

### `Root`

Top-level scene container parsed from `scene.json`.

| Field | Type | Description |
|-------|------|-------------|
| `camera` | `Camera` | Default camera |
| `general` | `General` | Scene-wide settings |
| `objects` | `Vec<Object>` | All scene objects |
| `version` | `i64` | Scene format version |

### `Camera`

Camera configuration for view/projection matrices.

| Field | Type | Description |
|-------|------|-------------|
| `center` | `Vectors` | Look-at target |
| `eye` | `Vectors` | Camera position |
| `up` | `Vectors` | Up vector |

### `General`

Scene-wide rendering parameters (selected key fields):

| Field | Type | Description |
|-------|------|-------------|
| `clearcolor` | `Vectors` | Background clear color (0-255) |
| `orthogonalprojection` | `Orthogonalprojection` | Scene resolution (width × height) |
| `nearz` / `farz` | `f64` | Near/far clip planes |
| `ambientcolor` | `Vectors` | Ambient light color |
| `bloom` / `bloomstrength` / `bloomthreshold` | bool/f64 | Bloom settings |
| `hdr` | `bool` | HDR enabled |
| `cameraparallaxamount` | `f64` | Parallax mouse influence |
| `fov` / `zoom` | `f64` | Perspective settings |
| `lightconfig` | `Option<Lightconfig>` | Point/spot light configuration |
| `skylightcolor` | `Vectors` | Skylight color |
| `gravitydirection` / `gravitystrength` | `Option<Vectors/f64>` | Gravity settings |
| `winddirection` / `windstrength` / `windenabled` | `Option<Vectors/f64/bool>` | Wind settings |
| `cameraparallax` / `cameraparallaxamount` / `cameraparallaxdelay` / `cameraparallaxmouseinfluence` | Value/f64/Value | Parallax settings |
| `bloomhdrfeather` / `bloomhdriterations` / `bloomhdrscatter` / `bloomhdrstrength` / `bloomhdrthreshold` | f64 | HDR bloom parameters |

### `Orthogonalprojection`

```rust
pub struct Orthogonalprojection {
    pub height: i64,
    pub width: i64,
}
```

### `Vectors`

Flexible 2D/3D vector type supporting three representations:

```rust
pub enum Vectors {
    Scaler(f64),              // Uniform scalar → Vec3(x, x, x)
    Vectors(String),         // Space-separated "x y" or "x y z"
    Object(Value),           // JSON object (not supported)
}
```

**Method:** `parse(&self) -> Option<Vec3>` — Converts to `glam::Vec3`. Scalar → `(x,x,x)`, 2-element string → `(x,y,0)`, 3-element string → `(x,y,z)`, Object → `None`.

### `BindUserProperty<T>`

Wallpaper Engine's property binding system for dynamic values:

```rust
pub enum BindUserProperty<T> {
    Value(T),                              // Direct value
    Object(serde_json::Map<String, Value>) // Bound to user property with optional "value" field
}
```

**Method:** `value(self) -> Option<T>` — Extracts the actual value (direct) or resolves binding from `{"value": ...}`.

---

## `object` — Scene Object Definitions

**File:** `object.rs`

### `Object`

Represents a single item in the scene. Selected key fields:

| Field | Type | Description |
|-------|------|-------------|
| `id` | `i64` | Unique object ID |
| `name` | `String` | Display name |
| `image` | `Option<String>` | Model JSON path (if texture object) |
| `origin` / `angles` / `scale` | `Option<Vectors>` | Transform.  `origin` is in scene pixels, measured from the scene's **bottom-left** corner (the scene spans `[0, w] × [0, h]`) |
| `size` | `Option<Vectors>` | Dimensions |
| `effects` | `Vec<Effect>` | Shader effects |
| `parent` | `Option<i64>` | Parent object ID for transform inheritance |
| `sound` | `Vec<String>` | Audio file paths |
| `playbackmode` | `Option<String>` | "loop" or other |
| `visible` | `Option<BindUserProperty<bool>>` | Visibility toggle |
| `color_blend_mode` | `Option<i64>` | Blend mode |
| `model` | `Option<Value>` | Model reference (`.mdl` files) |
| `animationlayers` | `Vec<Animationlayer>` | Animation layers |
| `particle` | `Option<String>` | Particle system reference |
| `color` | `Option<Vectors>` | Color value (used for solid fallback textures) |
| `alpha` | `Option<Value>` | Alpha value (used for solid fallback textures) |
| `instance` | `Option<Instance>` | Instance configuration |
| `instanceoverride` | `Option<Instanceoverride>` | Instance override parameters |
| `dependencies` | `Vec<i64>` | Dependent object IDs |
| `solid` / `castshadow` / `perspective` / `copybackground` | `Option<bool>` | Various flags |
| Text fields | `font`, `text`, `horizontalalign`, `verticalalign`, `alignment`, `padding`, `pointsize`, `limitrows`, etc. | Text object properties |
| Light fields | `light`, `density`, `exponent`, `innercone`, `outercone`, `radius`, `intensity`, `volumetricsexponent` | Light object properties |

### `Effect`

A shader effect applied to an object.

| Field | Type | Description |
|-------|------|-------------|
| `file` | `String` | Effect JSON path (e.g., `project.json`) |
| `id` | `i64` | Unique ID |
| `name` | `String` | Effect name |
| `passes` | `Vec<Pass>` | Render passes |
| `visible` | `Value` | Visibility |

### `Pass`

A single render pass in an effect.

| Field | Type | Description |
|-------|------|-------------|
| `constantshadervalues` | `Option<Map<String, Value>>` | Material constant overrides |
| `id` | `i64` | Pass ID |
| `textures` | `Vec<Option<String>>` | Additional textures: index 0=source, 1=mask, 2=noise |
| `combos` | `Option<Map<String, i64>>` | Shader combo defines |
| `usertextures` | `Option<(Value, Value)>` | User-defined textures |

### `Combos` (in `pass.combos`)

Shader compilation defines that control shader variants. Key combos:

| Combo | Effect |
|-------|--------|
| `VERTICAL` | Vertical orientation |
| `NOISE` | Noise displacement |
| `ANTIALIAS` | Anti-aliasing |
| `BLENDMODE` | Blend mode selection |
| `MODE` | General mode switch |
| `REPEAT` | Texture repeat |
| `ENABLEMASK` | Mask usage |
| `TRANSFORM` | Transformation mode |
| `MASK` | Mask enabled (set automatically when textures[1] present) |
| `TIMEOFFSET` | Time offset (set automatically when textures[2] present) |

### Additional types

| Type | Fields | Description |
|------|--------|-------------|
| `Animationlayer` | id, name, animation, additive, blend, blendin, blendout, blendtime, rate, visible | Animation layer definition |
| `Instance` | id, combos, textures, usertextures | Instance configuration |
| `Instanceoverride` | alpha, id, colorn, speed, size, lifetime, count, rate | Instance parameter overrides |
| `Zoom` | user, value | Zoom configuration |
| `Config` | passthrough | Pass-through mode |

---

## `scene_loader` — Package File Parser

**File:** `scene_loader.rs`

### `Scene`

The fully-loaded scene asset container.

```rust
pub struct Scene {
    pub root: Root,        // Parsed scene.json (or the synthetic video scene)
    pub assets: AssetStore, // Typed store of every parsed asset
    pub project: Option<Project>, // The package's project.json, when it has one
}
```

### `Scene::new(path: String, show_progress: bool, no_mdl: bool) -> Self`

Parses a `.pkg` file (the folder sources are read by the façade, which hands
the very same files to the loader — see [the two shapes of a
wallpaper](#the-two-shapes-of-a-wallpaper--the-folder-sources)):

1. Uses `pkg_parser::parser::Pkg::new(path)` to open the package
2. For each file, classifies it with `AssetType::from_path()`:
   - `.tex` → decoded in parallel by a bounded worker pool via `Tex::new` + `parse_to_rgba()` (a `.tex` whose payload is a video container becomes a video asset instead)
   - `.mdl` → parsed via `MdlFile::new()` (skipped entirely when `no_mdl` is set). Parse failures are logged at error level and skipped, since the puppet format is only partially documented.
   - `.json` (scene/effects/materials/particles/presets) → stored as `Rc<String>`
   - `.frag`/`.vert`/`.h` → stored as text (shaders are paired and parsed in [`Scene::prepare`])
   - audio/font/video/scripts/other → stored as typed assets
3. Shows a progress bar via `indicatif::ProgressBar`
4. Reads the package's `project.json` (see `project.rs`) — it tells the loader what kind of wallpaper the package holds
5. Parses `scene.json` as `Root` — or builds a **synthetic scene** for a video wallpaper (see below)
6. Returns the complete `Scene`

The archive is trusted to be well-formed: a package or asset that fails to
parse is fatal rather than silently skipped. The one exception is `.mdl`
models — the puppet format is only partially documented, so a model
that fails to parse is logged at error level and skipped instead of aborting
the scene. Asset *resolution* (model → material → texture) also stays lenient
because it can legitimately fall back to the Wallpaper Engine `assets/`
directory or reference runtime textures.

**Threading:** `.tex` files are decoded by a worker pool sized to the available CPU cores (capped at the number of textures). Work is handed out through an atomic index, so threads stay busy even when individual textures decode at different speeds; no thread is spawned per file.

### The two shapes of a wallpaper — the folder sources

A wallpaper is not always a `.pkg` archive: the Steam workshop ships the
wallpapers as **folders** (`…/workshop/content/431960/<id>/`), each with a
`project.json` and the files of the wallpaper next to it.

The façade describes such a wallpaper with a `WallpaperSource`
([engine.md](engine.md#sources--wallpapersource)):

```rust
pub enum WallpaperSource {
    Package(PathBuf),  // a .pkg archive
    Folder(PathBuf),   // a folder with project.json and the files next to it
}
```

`WallpaperSource::read_files()` returns the files of the wallpaper as a map
from the path of a file *inside* it to its bytes:

- for an **archive** it is the map the package parser produces,
- for a **folder** it is built by walking the folder: the paths are relative
  to the folder and use `/` as the separator, so the two shapes look exactly
  the same to the loader.

A file that cannot be read is an `EngineError::Io` that names the path it
happened at.  The loader itself stays shape-agnostic: it classifies every file
by its path inside the wallpaper, whether it came from an archive or from a
folder.

### The metadata — what wallpapers exist

The façade also knows *which* wallpapers exist.  `scan_wallpapers()` walks a
workshop folder and describes every wallpaper it finds — one folder per
wallpaper, each with a `project.json` at its root.

The manifest is parsed with the parser of the loader
([`project.rs`](#project--the-manifest-of-a-wallpaper)): it tells the loader
what kind of wallpaper it is holding (a scene, a video, a web page, an
application), which file is its main file and which one is its preview image.
The preview is what a picker shows, so the host does not have to load a
wallpaper to draw its thumbnail.

### Video wallpapers — the synthetic scene

A package whose `project.json` says `"type": "Video"` has **no `scene.json`**:
it holds a single looping clip. Such a package is brought up as a *synthetic
scene*, so that the whole existing pipeline (camera projection, fit modes,
effects, adapters) works exactly like it does for a scene wallpaper:

* the video is found through `project.file` (`"video.mp4"`), falling back to the
  first video of the package;
* a synthetic material (`materials/video.json` →
  `{"passes":[{"textures":["video"]}]}`) and model (`models/video.json` →
  `{"material":"materials/video.json","autosize":false}`) are injected into the
  asset store, and the video is bound under `materials/video.mp4` — the name the
  material refers to;
* the scene itself holds **one** image object that covers the whole projection:
  `origin = "0 0"` (the scene's origin is its bottom-left corner),
  `size` = the video's own resolution, `id = 1`, plus
  `general.orthogonalprojection = {width, height}` = the video's resolution and
  a black `clearcolor`.

The `scene.json` lookup is skipped entirely for such a package.

### `Scene::set_assets_path(&mut self, assets_path: PathBuf)`

Enables lazy-loading fallback to a Wallpaper Engine `assets/` directory on disk. When a typed accessor can't find a key in memory, the file is read from `{assets_path}/{key}`, parsed according to its [`AssetType`], cached, and returned.

### `Scene::prepare(&mut self)`

Pre-parses every shader pair now that the assets directory is known. Each pair is turned into a [`ShaderProgram`] (interface layout, `[COMBO]` defaults, source macros and resolved headers) so pipeline compilation never has to re-read or re-scan the sources.

---

## `assets_loader` — Typed Asset Store

**File:** `assets_loader.rs`

Classifies every scene file into an [`AssetType`] and stores the parsed result as an [`Asset`]. Lazy-loads from the Wallpaper Engine assets directory on disk when a key is not present in the in-memory store.

### `AssetType`

Enumerates every asset category Wallpaper Engine uses, resolved from the path (extension plus directory for JSON documents):

| Variant | Source |
|---------|--------|
| `Scene` | `scene.json` |
| `Effect` | `effects/**/effect.json` |
| `Material` | `materials/**/*.json` |
| `Model` | `models/**/*.mdl` |
| `Particle` | `particles/**/*.json` |
| `Preset` | `presets/**/*.json` |
| `Shader` | `.frag`, `.vert`, `.h`, `.glsl`, `.wgsl` |
| `Texture` | `.tex` |
| `Sound` | `.ogg`, `.mp3`, `.wav`, `.flac` |
| `Font` | `.ttf`, `.otf` |
| `Video` | `.mp4`, `.webm`, `.gif`, `.avi` |
| `Script` | `.js` |
| `Json` | Any other JSON document |
| `Other` | Anything else |

### `Asset`

```rust
pub enum Asset {
    Texture(Rc<Tex>),
    Model(Rc<MdlFile>),
    Json(Rc<String>),      // scene/effects/materials/particles/presets
    Script(Rc<String>),
    Shader(Rc<ShaderProgram>),
    Text(Rc<String>),      // shader sources / headers before pairing
    Sound(Vec<u8>),
    Font(Vec<u8>),
    Video(Rc<VideoAsset>), // mp4/webm/gif — the bytes and their metadata
    Raw(Vec<u8>),
}
```

### `VideoAsset`

A video (or animated image): the raw container bytes, the metadata that was
probed once, when the asset was parsed, and — when the clip fits into the
memory budget of the parser — the frames that were decoded eagerly.

```rust
pub struct VideoAsset {
    pub bytes: Rc<[u8]>,             // the container bytes (mp4/webm/gif)
    pub info: VideoInfo,             // width, height, duration, fps, frame_count, …
    pub frames: Option<Rc<[VideoFrame]>>, // the decoded frames, when the clip was decoded eagerly
}
```

The bytes are kept behind an `Rc`, so the loader and the renderer share one
copy of the container. A container that cannot be probed keeps an empty
`VideoInfo` — the problem is reported when the renderer tries to decode it.

The clip is decoded **once**, when the asset is parsed: the renderer plays the
frames back by index (a binary search over the frame times) and never decodes
a frame again. A clip that does not fit into the memory budget of the parser
is not decoded at all: `frames` stays `None` and the renderer streams the
clip with a `VideoPlayer`, exactly like it did before.

> Wallpaper Engine stores its **video textures as a `.tex` file whose payload
> is the video container**. Such a file is parsed into an `Asset::Video` too,
> not into a texture.

### `AssetStore`

```rust
pub struct AssetStore {
    map: RefCell<Map<String, Asset>>,
    headers: RefCell<Option<Rc<Map<String, String>>>>,
    assets_path: Option<PathBuf>,
}
```

| Method | Description |
|--------|-------------|
| `set_assets_path(path)` | Sets the disk fallback directory |
| `get(key) -> Option<Asset>` | Lookup by key, lazy-parses from disk if missing |
| `texture(key) -> Option<Rc<Tex>>` | Parsed `.tex` texture |
| `model(key) -> Option<Rc<MdlFile>>` | Parsed `.mdl` puppet model |
| `video(key) -> Option<Rc<VideoAsset>>` | Parsed video (`.mp4`, or a `.tex` with a video payload) |
| `first_video() -> Option<(String, Rc<VideoAsset>)>` | The first video of the store, with its key |
| `json(key) -> Option<Rc<String>>` | JSON-backed document |
| `project() -> Option<Project>` | The package's `project.json` (see `project.rs`) |
| `insert(key, asset)` | Inserts (or replaces) a parsed asset |
| `take_sound(key) -> Option<Vec<u8>>` | Removes and returns audio data (consumed on playback) |
| `shader(frag, vert) -> Option<Rc<ShaderProgram>>` | Pre-parsed shader pair |
| `preparse_shaders()` | Eagerly parses every shader pair in the store |

### Expected assets directory layout:

```
assets/
├── effects/     # Effect JSON definitions
├── fonts/       # Font files
├── materials/   # .tex textures + material JSONs
├── models/      # .mdl puppet model files
├── particles/   # Particle system definitions
├── presets/     # Preset configurations
├── scenes/      # Scene configurations
├── scripts/     # JavaScript scripts
├── shaders/     # GLSL shader source files (.frag, .vert)
└── zcompat/     # Compatibility layer files
```

---

## `project` — The manifest of a wallpaper

**File:** `project.rs`

Every Wallpaper Engine wallpaper carries a `project.json` at its root.  It
describes *what kind* of wallpaper it is and a couple of descriptive fields:

```json
{ "title": "My Video", "type": "Video", "file": "video.mp4", "preview": "preview.jpg" }
```

The loader needs it to decide how a wallpaper should be brought up: a scene
wallpaper is loaded through its `scene.json`, a video wallpaper is a single
looping clip (see [the synthetic scene](#video-wallpapers--the-synthetic-scene)).

### `WallpaperKind`

```rust
pub enum WallpaperKind {
    Scene,        // scene.json + its materials, effects and models
    Video,        // a single looping clip
    Web,          // a web page (HTML/JS)
    Application,  // an application
    Unknown(String),  // a kind this parser does not know about
}
```

### `Project`

```rust
pub struct Project {
    pub title: String,          // the human readable title
    pub kind: WallpaperKind,    // what kind of wallpaper this is
    pub file: Option<String>,   // the main file ("scene.json", "video.mp4")
    pub preview: Option<String>,// the preview image
}
```

### `parse_project(json: &str) -> Option<Project>`

Parses the JSON text of a `project.json`.  The parser is deliberately lenient:
unknown fields are ignored, a missing `type` means a scene and an unknown
`type` is preserved as `WallpaperKind::Unknown` instead of failing the whole
parse.  A text that is not a JSON *object* is not a project at all, so it is
rejected (`None`).

The manifest is also what the façade reads when it scans a workshop folder
([the metadata](#the-metadata--what-wallpapers-exist)).

---

## `object_loader` — Object Conversion

**File:** `object_loader.rs`

Converts raw `Object`/`Effect` definitions into render-ready types.

### `TextureObject`

| Field | Type | Description |
|-------|------|-------------|
| `source` | `TextureSource` | The pixels of the object: a static texture or a video |
| `transform` | `Transform` | Local position/rotation/scale/alignment |
| `model` | `Mat4` | World model matrix (parent chain applied) |
| `local_model` | `Mat4` | The object's own matrix (before the parent chain) — needed for bone-socket attachments |
| `size` | `Vec2` | Width/height |
| `uv_scale` | `Vec2` | The sampled region of the texture (`(1, 1)` for a video) |
| `parent` | `Option<i64>` | Parent object ID |
| `effects` | `Vec<Effect>` | Shader effects |
| `visible` | `bool` | Whether the object is visible |
| `mesh` | `Option<PuppetMesh>` | Optional puppet mesh extracted from a `.mdl` file |

### `TextureSource`

Where an object's pixels come from.

```rust
pub enum TextureSource {
    Static(Rc<Tex>),        // a static texture parsed from a `.tex` file
    Video(Rc<VideoAsset>),  // a video or animated image, decoded frame by frame
}
```

`resolve_texture()` resolves them in this order:

1. `materials/{name}.tex` whose payload is a video container — Wallpaper Engine
   stores its video textures that way,
2. `materials/{name}.mp4` (and `.webm`/`.gif`/`.avi`) — a standalone video next
   to the material,
3. `materials/{name}.tex` — an ordinary texture.

A video has no padded region, so its `uv_scale` is always `(1, 1)` and its
dimension is the video's own resolution.

### `AudioObject`

| Field | Type | Description |
|-------|------|-------------|
| `sounds` | `Vec<String>` | Audio file paths |
| `playback_mode` | `PlaybackMode` | `Loop` or `Others` |

### `PlaybackMode`

```rust
pub enum PlaybackMode {
    Loop,
    Others,
}
```

### `ObjectMap`

```rust
pub struct ObjectMap {
    pub texture: Vec<TextureObject>,
    pub audio: Vec<AudioObject>,
}
```

### `ObjectMap::with_clear_color(objects: &Vec<Object>, scene: &Scene, clear_color: Vec3, no_mdl: bool) -> Self`

Processes all scene objects:

1. **Classifies each object** via `Object::element_type()`, which returns an [`ElementType`](#elementtype):
   - `Image` — has an `image` field. Resolves model JSON → material JSON → texture reference. Falls back to a **solid-colour 1×1 fallback texture** (using the object's `color`/`alpha` properties) if any step of the chain fails.
   - `Sound` — has `sound` files
   - `Particle` / `Text` / `Light` / `Camera` — not rendered yet; treated as transform-only nodes so children keep their world transforms
   - `Node` — transform-only parent for the hierarchy
   
2. **Resolves parent-child transform inheritance**: iterates the hierarchy, composing each child's model matrix with its parent's (`M_child_world = M_parent_world · M_child_local`). Also propagates invisibility (if parent is not visible, child is also not visible).

   Objects with an `attachment` (a bone socket of their puppet parent) are placed **relative to that socket**: `M_world(child) = M_world(parent) · M_socket · M_local` — the socket is the base and the object's own `origin` is the offset from it.

   > The socket is re-evaluated every frame from the puppet's **animated** bone
   > matrices (see `draw::update_animated_meshes`), so an attached prop follows the
   > puppet's animation — like in Wallpaper Engine.

   > Real scenes confirm the composition: a puppet's face is attached to the
   > controller's `Attachment` socket (which sits at the controller's origin, i.e. at
   > the character's feet) with an origin of ~750 px — the origin has to be
   > applied *on top of* the socket, otherwise the face would end up at the feet.

3. **Returns ordered `texture` and `audio` vectors** — invisible objects are excluded from the output.

**Visibility:** Objects with `visible == false` are skipped during loading. Child objects whose parent is not visible are also skipped.

**Model Loading:** For `Image` objects, the chain is: `object.image` → model JSON → `model.material` → material JSON → `passes[0].textures[0]` → `.tex` file loaded through `scene.assets.texture()`. The model's `puppet` path (if any) is resolved through `scene.assets.model()` and meshed via `mdl::extract_mesh()` unless `no_mdl` is set.

**Puppet meshes:** the control points of an `.mdl` are in the model's *own* units (the same scale as the texture, centred on the origin) — they are used verbatim, exactly like Wallpaper Engine, which never rescales them.  The layer's `size` (which equals the texture's real size) therefore only affects the alignment anchor.  The mesh's UVs are relative to the texture's *real* region, so they carry the same `uv_scale` the plain quads use (textures that are stored padded only carry the image in their top-left corner).

**Solid-Colour Fallback:** When a texture object's image/material/texture chain fails to resolve, a 1×1 RGBA texture is synthesized from the object's `color` and `alpha` properties (falling back to white and 1.0 alpha respectively).

### `ElementType`

```rust
pub enum ElementType {
    Image,       // has `image`
    Sound,       // has `sound`
    Particle,    // has `particle`
    Text,        // has `text` / `font`
    Light,       // has `light`
    Camera,      // has `camera`
    Node,        // transform-only
}
```

`Object::element_type()` infers the kind from the populated fields, mirroring `AssetType::from_path()` on the asset side.

---

## `model` — Material Model

**File:** `model.rs`

```rust
pub struct Model {
    pub autosize: bool,
    pub cropoffset: Option<String>,
    pub material: String,        // Path to material .json file
    pub puppet: Option<String>,  // Skeletal animation reference (.puppet)
}
```

Referenced by texture objects in `object.image`. The `material` field points to the material JSON that contains the actual `.tex` file reference to load and display.

---

## `shader` — GLSL Preprocessing & Parsing

**Files:** `shader/mod.rs`, `shader/asset.rs`, `shader/header.rs`, `shader/layout.rs`, `shader/preprocessor.rs`, `shader/c_expr.rs`, `shader/compat.rs`, `shader/dialect.rs`, `shader/glsl/`, `shader/varying.rs`

All Wallpaper Engine shader parsing and GLSL→Vulkan preprocessing lives here so
shaders are understood as soon as the scene is loaded.

The preprocessing has four stages:

1. **`preprocessor`** (with `c_expr`) — resolves the conditionals of a C
   preprocessor, inlines the headers the shader includes and collects the
   macros of the shader and of its headers.
2. **`compat`** — rewrites the constructs of the Wallpaper Engine dialect that
   have no GLSL equivalent (`saturate`, `frac`, `texSample2D`, `gl_FragColor`,
   `clip`, …).
3. **`glsl`** — normalises the types of the shader, so that it satisfies the
   strict typing rules of GLSL.
4. **`dialect`** — rewrites the rest of the dialect of the engine: the `mul`
   calls in their row vector form, the precision qualifiers, the sampler types
   and the macros of the engine.

### `ShaderProgram` (`shader/asset.rs`)

A `.frag`/`.vert` pair parsed once by `Scene::prepare()`:

| Field | Description |
|-------|-------------|
| `vertex` / `fragment` | Raw GLSL sources |
| `headers` | Shared built-in headers (`Rc`) |
| `layout` | Combined `EffectLayout` (samplers, uniforms, varyings) |
| `default_defines` | `[COMBO]` defaults from either stage |
| `source_defines` | Plain `#define` macros from either stage |

### `shader/mod.rs`

- `preprocess_pair_with_layout(...)` — the define-dependent GLSL→Vulkan pass, using the precomputed layout
- `collect_layout(...)` / `collect_source_defines(...)` / `collect_default_defines(...)`
- `preprocess_with_layout_tracked(...)`

### `shader/layout.rs` — `EffectLayout`

Shader interface introspection plus the std140 uniform size helpers
(`align_up`, `type_align`, `type_size`, `compute_uniform_size`) shared with
`renderer::post_processor::effect_param::UniformLayout`.

### `shader/header.rs` — Built-in Headers

`get_headers(load)` pulls `shaders/common*.h` through a byte loader and
`WM_SAMPLER_BINDING` defines the shared sampler binding.

### `shader/varying.rs` — The varyings of the compiled branch

A varying that only appears inside a conditional block of the vertex shader is
hoisted to an unconditional vertex output, so that the fragment shader always
finds it.  A varying that is missing on one side is synthesised.

### `shader/compat.rs` and `shader/dialect.rs`

The GLSL builtin `mul`/`saturate`/`texSample2D`/… replacements.  The
compatibility stage is a plain textual substitution, the dialect stage works on
the source text alone as well — neither of them needs the layout of the
effect, so both can be tested on their own.

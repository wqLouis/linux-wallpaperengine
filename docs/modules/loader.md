# Scene Loader (`src/scene/loader/`)

Parses Wallpaper Engine `.pkg` files and converts raw scene data into structured objects ready for rendering.

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
| `origin` / `angles` / `scale` | `Option<Vectors>` | Transform |
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
| `constantshadervalues` | `Option<BTreeMap<String, Value>>` | Material constant overrides |
| `id` | `i64` | Pass ID |
| `textures` | `Vec<Option<String>>` | Additional textures: index 0=source, 1=mask, 2=noise |
| `combos` | `Option<BTreeMap<String, i64>>` | Shader combo defines |
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
    pub root: Root,                                    // Parsed scene.json
    pub textures: TextureBucket,                       // .tex → Rc<Tex>
    pub mdls: MdlBucket,                               // .mdl → Rc<MdlFile>
    pub jsons: JsonBucket,                              // .json → Rc<String>
    pub misc: MiscBucket,                               // Other files (shaders, audio)
}
```

### `Scene::new(path: String) -> Self`

Parses a `.pkg` file:

1. Uses `pkg_parser::parser::Pkg::new(path)` to open the package
2. For each file in the package:
   - `.tex` → parse in parallel thread via `Tex::new` + `parse_to_rgba()`
   - `.mdl` → parse in parallel thread via `MdlFile::new()`
   - `.json` → store as `Rc<String>`
   - Other → store as raw bytes
3. Shows a progress bar via `indicatif::ProgressBar`
4. Parses `scene.json` as `Root`
5. Returns the complete `Scene`

**Threading:** `.tex` and `.mdl` files are parsed in parallel using `thread::spawn`, with results merged into the respective buckets.

### `Scene::set_assets_path(&mut self, assets_path: PathBuf)`

Enables lazy-loading fallback to a Wallpaper Engine `assets/` directory on disk. When `get(key)` is called on any bucket and the key isn't found in memory, the file is read from `{assets_path}/{key}`, parsed, cached, and returned.

---

## `assets_loader` — Lazy-Loading Bucket Wrappers

**File:** `assets_loader.rs`

Lazy-loading wrappers that fall back to the Wallpaper Engine assets directory on disk when a requested asset is not found in the in-memory map.

### `TextureBucket`

```rust
pub struct TextureBucket {
    map: RefCell<BTreeMap<String, Rc<Tex>>>,
    assets_path: Option<PathBuf>,
}
```

| Method | Description |
|--------|-------------|
| `new(map, assets_path) -> Self` | Creates bucket from initial map |
| `set_assets_path(path)` | Sets disk fallback path |
| `get(key) -> Option<Rc<Tex>>` | Lookup by key, lazy-loads from disk if missing |

### `MdlBucket`

```rust
pub struct MdlBucket {
    map: RefCell<BTreeMap<String, Rc<MdlFile>>>,
    assets_path: Option<PathBuf>,
}
```

Same pattern as `TextureBucket` but for `.mdl` puppet model files.

### `JsonBucket`

```rust
pub struct JsonBucket {
    map: RefCell<BTreeMap<String, Rc<String>>>,
    assets_path: Option<PathBuf>,
}
```

Stores JSON files as `Rc<String>`. Lazy-loads from disk if missing.

### `MiscBucket`

```rust
pub struct MiscBucket {
    map: RefCell<BTreeMap<String, Vec<u8>>>,
    assets_path: Option<PathBuf>,
}
```

Stores binary files (shaders, audio, etc.). Additional method:

| Method | Description |
|--------|-------------|
| `remove(key) -> Option<Vec<u8>>` | Removes and returns data (used for audio consumption), lazy-loading from disk if needed |

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

## `object_loader` — Object Conversion

**File:** `object_loader.rs`

Converts raw `Object`/`Effect` definitions into render-ready types.

### `TextureObject`

| Field | Type | Description |
|-------|------|-------------|
| `texture` | `Rc<Tex>` | Parsed RGBA texture data |
| `origin` | `Vec3` | World-space position |
| `angles` | `Vec3` | Rotation (Euler, degrees, Z-up) |
| `size` | `Vec2` | Width/height |
| `scale` | `Vec3` | Scale multiplier |
| `parent` | `Option<i64>` | Parent object ID |
| `effects` | `Vec<Effect>` | Shader effects |
| `visible` | `bool` | Whether the object is visible |

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

### `ObjectMap::with_clear_color(objects: &Vec<Object>, scene: &Scene, clear_color: Vec3) -> Self`

Replaces the old `ObjectMap::new()`. Processes all scene objects:

1. **Classifies each object**:
   - **Texture** — has `image` field. Resolves model JSON → material JSON → texture reference. Falls back to a **solid-colour 1×1 fallback texture** (using object's `color`/`alpha` properties and the scene's `clear_color`) if any step of the chain fails.
   - **Audio** — has `sound` files
   - **Node** — transform-only parent for hierarchy (no image, no sound)
   
2. **Resolves parent-child transform inheritance**: iterates the hierarchy, accumulating `angles`, `scale`, and `origin` from parents. Also propagates invisibility (if parent is not visible, child is also not visible).

3. **Returns ordered `texture` and `audio` vectors** — invisible objects are excluded from the output.

**Visibility:** Objects with `visible == false` are skipped during loading. Child objects whose parent is not visible are also skipped.

**Model Loading:** For texture objects, the chain is: `object.image` → model JSON → `model.material` → material JSON → `passes[0].textures[0]` → `.tex` file loaded from scene textures.

**Solid-Colour Fallback:** When a texture object's image/material/texture chain fails to resolve, a 1×1 RGBA texture is synthesized from the object's `color` and `alpha` properties (falling back to `clear_color` and 1.0 alpha respectively).

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

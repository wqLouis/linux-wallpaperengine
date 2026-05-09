# Scene Loader (`src/scene/loader/`)

Parses Wallpaper Engine `.pkg` files and converts raw scene data into structured objects ready for rendering.

---

## `scene` — Scene Root & Core Types

**File:** `scene.rs`

### `Root`

Top-level scene container.

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

Scene-wide rendering parameters. Key fields:

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

**Method:** `parse(&self) -> Option<Vec3>` — Converts to `glam::Vec3`.

### `BindUserProperty<T>`

Wallpaper Engine's property binding system for dynamic values:

```rust
pub enum BindUserProperty<T> {
    Value(T),                              // Direct value
    Object(serde_json::Map<String, Value>) // Bound to user property
}
```

**Method:** `value(self) -> Option<T>` — Extracts the actual value or resolves binding from `{"value": ...}`.

---

## `object` — Scene Object Definitions

**File:** `object.rs`

### `Object`

Represents a single item in the scene. Fields include:

| Field | Type | Description |
|-------|------|-------------|
| `id` | `i64` | Unique object ID |
| `name` | `String` | Display name |
| `image` | `Option<String>` | Texture path (if texture object) |
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
| `textures` | `Vec<Option<String>>` | Additional textures: index 0=mask, 1=noise |
| `combos` | `Option<Combos>` | Shader combo defines |
| `usertextures` | `Option<(Value, Value)>` | User-defined textures |
| `material` | (in JSON) | Material file path |

### `Combos`

Shader compilation defines that control shader variants. All fields are `Option<i64>`:

| Field | Effect |
|-------|--------|
| `VERTICAL` | Vertical orientation |
| `NOISE` | Noise displacement |
| `ANTIALIAS` | Anti-aliasing |
| `A_SMOOTH_CURVE` | Smooth curve blending |
| `BLENDMODE` | Blend mode selection |
| `MODE` | General mode switch |
| `REPEAT` | Texture repeat |
| `ENABLEMASK` | Mask usage |
| `TRANSFORM` | Transformation mode |
| `...` | Many more variants |

---

## `scene_loader` — Package File Parser

**File:** `scene_loader.rs`

### `Scene`

The fully-loaded scene asset container.

```rust
pub struct Scene {
    pub root: Root,                              // Parsed scene.json
    pub textures: BTreeMap<String, Rc<Tex>>,     // .tex → parsed texture
    pub jsons: BTreeMap<String, String>,          // .json → raw string
    pub misc: BTreeMap<String, Vec<u8>>,          // Other files (shaders, audio)
}
```

### `Scene::new(path: String) -> Self`

Parses a `.pkg` file:

1. Uses `depkg::Pkg::new(path)` to open the package
2. For each file:
   - `.tex` → parse to RGBA (threaded via `Tex::new` + `parse_to_rgba`)
   - `.json` → store as string
   - Other → store as raw bytes
3. Shows a progress bar via `indicatif::ProgressBar`
4. Parses `scene.json` as `Root`
5. Returns the complete `Scene`

**Threading:** `.tex` files are parsed in parallel using `thread::spawn`, with results merged into a single `BTreeMap`.

---

## `object_loader` — Object Conversion

**File:** `object_loader.rs`

Converts raw `Object`/`Effect` definitions into render-ready types.

### `TextureObject`

| Field | Type | Description |
|-------|------|-------------|
| `texture` | `Rc<Tex>` | Parsed RGBA texture data |
| `origin` | `Vec3` | World-space position |
| `angles` | `Vec3` | Rotation (Euler, degrees) |
| `size` | `Vec2` | Width/height |
| `scale` | `Vec3` | Scale multiplier |
| `parent` | `Option<i64>` | Parent object ID |
| `effects` | `Vec<Effect>` | Shader effects |

### `AudioObject`

| Field | Type | Description |
|-------|------|-------------|
| `sounds` | `Vec<String>` | Audio file paths |
| `playback_mode` | `PlaybackMode` | `Loop` or `Others` |

### `ObjectMap`

```rust
pub struct ObjectMap {
    pub texture: Vec<TextureObject>,
    pub audio: Vec<AudioObject>,
}
```

### `ObjectMap::new(objects: &Vec<Object>, scene: &Scene) -> Self`

1. Iterates all objects, classifying each as:
   - **Texture** — has `image` field, loads `.tex` via `Model` JSON
   - **Audio** — has `sound` files, maps `playbackmode` to `PlaybackMode`
   - **Node** — transform-only parent for hierarchy
2. Resolves parent-child transform inheritance (accumulates angles, scale, origin)
3. Returns ordered `texture` and `audio` vectors

**Visibility:** Objects with `visible == false` are skipped.

**Model Loading:** For texture objects, reads `Model` JSON (material path), resolves `.tex` file.

---

## `model` — Material Model

**File:** `model.rs`

```rust
pub struct Model {
    pub autosize: bool,
    pub cropoffset: Option<String>,
    pub material: String,        // Path to material .tex file
    pub puppet: Option<String>,  // Skeletal animation reference (.puppet)
}
```

Referenced by texture objects in `object.image`. The `material` field points to the actual `.tex` file to load and display.
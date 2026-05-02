# Scene Loader (`src/scene/loader/`)

Parses Wallpaper Engine `.pkg` files and converts raw scene data into structured objects ready for rendering.

---

## `scene` — Scene Root & Core Types

**File:** `scene.rs`  
**Re-exports from:** `object.rs`

### Public Types

#### `Root`
Top-level scene container.

| Field | Type | Description |
|-------|------|-------------|
| `camera` | `Camera` | Default camera |
| `general` | `General` | Scene-wide settings |
| `objects` | `Vec<Object>` | All scene objects |
| `version` | `i64` | Scene format version |

#### `Camera`
| Field | Type | Description |
|-------|------|-------------|
| `center` | `Vectors` | Look-at target |
| `eye` | `Vectors` | Camera position |
| `up` | `Vectors` | Up vector |

#### `General`
Scene-wide rendering parameters. Key fields:

| Field | Type | Description |
|-------|------|-------------|
| `clearcolor` | `Vectors` | Background clear color (0-255) |
| `orthogonalprojection` | `Orthogonalprojection` | Scene resolution (width × height) |
| `nearz` / `farz` | `f64` | Near/far clip planes |
| `ambientcolor` | `Vectors` | Ambient light color |
| `bloom` | `bool` | Bloom enabled |
| `bloomstrength` / `bloomthreshold` | `f64` | Bloom parameters |
| `hdr` | `bool` | HDR enabled |

#### `Vectors`

Flexible 2D/3D vector type supporting three representations:

```rust
pub enum Vectors {
    Scaler(f64),              // Uniform scalar → Vec3(x, x, x)
    Vectors(String),          // Space-separated "x y" or "x y z"
    Object(Value),            // JSON object (not supported)
}
```

**Method:** `parse(&self) -> Option<Vec3>` — Converts to `glam::Vec3`.

#### `BindUserProperty<T>`

Wallpaper Engine's property binding system:

```rust
pub enum BindUserProperty<T> {
    Value(T),
    Object(serde_json::Map<String, Value>),  // Bound to user property
}
```

**Method:** `value(self) -> Option<T>` — Extracts the actual value or resolves binding.

---

## `object` — Scene Object Definitions

**File:** `object.rs`

### `Object`

Represents a single item in the scene. Key fields:

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

### `Effect`
| Field | Type | Description |
|-------|------|-------------|
| `file` | `String` | Effect JSON path |
| `id` | `i64` | Unique ID |
| `name` | `String` | Effect name |
| `passes` | `Vec<Pass>` | Render passes |
| `visible` | `Value` | Visibility |

### `Pass`
| Field | Type | Description |
|-------|------|-------------|
| `constantshadervalues` | `Option<BTreeMap<String, Value>>` | Material constant overrides |
| `id` | `i64` | Pass ID |
| `textures` | `Vec<Option<String>>` | Additional texture paths (index 0=mask, 1=noise) |
| `combos` | `Option<Combos>` | Shader combo defines |

### `Combos`

Shader compilation defines (all `Option<i64>`). Examples: `VERTICAL`, `NOISE`, `ANTIALIAS`, `BLENDMODE`, `MODE`, `REPEAT`, etc. These control shader variant selection.

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
2. For each file: `.tex` → parse to RGBA (threaded), `.json` → store as string, other → store as bytes
3. Shows a progress bar via `indicatif::ProgressBar`
4. Parses `scene.json` as `Root`
5. Returns the complete `Scene`

---

## `object_loader` — Object Conversion

**File:** `object_loader.rs`

Converts raw `Object`/`Effect` definitions into render-ready types.

### Public Types

#### `TextureObject`
| Field | Type | Description |
|-------|------|-------------|
| `texture` | `Rc<Tex>` | Parsed RGBA texture data |
| `origin` | `Vec3` | World-space position |
| `angles` | `Vec3` | Rotation (Euler, degrees) |
| `size` | `Vec2` | Width/height |
| `scale` | `Vec3` | Scale multiplier |
| `parent` | `Option<i64>` | Parent object ID |
| `effects` | `Vec<Effect>` | Shader effects |

#### `AudioObject`
| Field | Type | Description |
|-------|------|-------------|
| `sounds` | `Vec<String>` | Audio file paths |
| `playback_mode` | `PlaybackMode` | `Loop` or `Others` |

#### `PlaybackMode`
```rust
pub enum PlaybackMode { Loop, Others }
```

#### `ObjectMap`
```rust
pub struct ObjectMap {
    pub texture: Vec<TextureObject>,
    pub audio: Vec<AudioObject>,
}
```

### `ObjectMap::new(objects: &Vec<Object>, scene: &Scene) -> Self`

1. Iterates all objects, classifying each as `Texture`, `Audio`, or `Node` (transform-only parent)
2. Resolves parent-child transform inheritance: child accumulates parent's angles, scale, origin
3. Returns ordered `texture` and `audio` vectors

**Visibility:** Objects with `visible == false` are skipped during loading.

---

## `model` — Material Model

**File:** `model.rs`

```rust
pub struct Model {
    pub autosize: bool,
    pub cropoffset: Option<String>,
    pub material: String,       // Path to material .tex file
    pub puppet: Option<String>, // Skeletal animation reference
}
```

Parsed from `model.json` referenced by texture objects. The `material` field points to the actual `.tex` file to use.

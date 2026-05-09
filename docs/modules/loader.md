# Scene Loader (`src/scene/loader/`)

Parses Wallpaper Engine `.pkg` files and converts raw scene data into structured objects ready for rendering.

---

## `scene` — Scene Root & Core Types

### `Root`

Top-level scene container parsed from `scene.json`.

| Field | Type | Description |
|-------|------|-------------|
| `camera` | `Camera` | Default camera configuration |
| `general` | `General` | Scene-wide settings (clear color, projection, bloom, etc.) |
| `objects` | `Vec<Object>` | All scene objects |
| `version` | `i64` | Scene format version |

### `Camera`

| Field | Type | Description |
|-------|------|-------------|
| `center` | `Vectors` | Look-at target position |
| `eye` | `Vectors` | Camera position |
| `up` | `Vectors` | Up vector |

### `General` (key fields)

| Field | Type | Description |
|-------|------|-------------|
| `clearcolor` | `Vectors` | Background clear color (RGB 0–255) |
| `orthogonalprojection` | `Orthogonalprojection` | Scene resolution (width × height) |
| `nearz` / `farz` | `f64` | Near/far clip planes |
| `ambientcolor` | `Vectors` | Ambient light color |
| `bloom` / `bloomstrength` | `bool` / `f64` | Bloom settings |
| `cameraparallaxamount` | `f64` | Parallax mouse influence |

### `Vectors`

Flexible 2D/3D vector parsed from `scene.json`. Supports three representations:

| Variant | Example | Result |
|---------|---------|--------|
| `Scaler(f64)` | `1.0` | `Vec3(x, x, x)` |
| `Vectors(String)` | `"1 2 3"` or `"1 2"` | `Vec3(x, y, z)` or `Vec3(x, y, 0)` |
| `Object(Value)` | JSON object | `None` (unsupported) |

**Method:** `parse() -> Option<Vec3>` — converts to `glam::Vec3`.

### `Orthogonalprojection`

| Field | Type | Description |
|-------|------|-------------|
| `width` | `i64` | Scene width in pixels |
| `height` | `i64` | Scene height in pixels |

---

## `object` — Scene Object Definitions

### `Object`

A single item in the scene (texture, audio, node, light, etc.). Key fields:

| Field | Type | Description |
|-------|------|-------------|
| `id` | `i64` | Unique ID |
| `name` | `String` | Display name |
| `image` | `Option<String>` | Texture material path (if texture object) |
| `origin` / `angles` / `scale` | `Option<Vectors>` | Transform (position, rotation, scale) |
| `size` | `Option<Vectors>` | Dimensions |
| `effects` | `Vec<Effect>` | Shader effects applied to this object |
| `parent` | `Option<i64>` | Parent ID for transform inheritance |
| `sound` | `Vec<String>` | Audio file paths |
| `visible` | `Option<BindUserProperty<bool>>` | Visibility toggle |

### `Effect`

| Field | Type | Description |
|-------|------|-------------|
| `file` | `String` | Effect JSON path (e.g. `project.json`) |
| `id` | `i64` | Unique ID |
| `name` | `String` | Display name |
| `passes` | `Vec<Pass>` | Render passes |
| `visible` | `Value` | Visibility |

### `Pass`

| Field | Type | Description |
|-------|------|-------------|
| `constantshadervalues` | `Option<BTreeMap<String, Value>>` | Material constant overrides |
| `id` | `i64` | Pass ID |
| `textures` | `Vec<Option<String>>` | Additional textures (mask, noise) |
| `combos` | `Option<Combos>` | Shader compilation flags |

### `Combos`

Shader compilation defines (`[COMBO]` annotations) that control shader variants. Includes flags like `VERTICAL`, `NOISE`, `ANTIALIAS`, `ENABLEMASK`, `BLENDMODE`, `TRANSFORM`, and many more.

---

## `scene_loader` — Package File Parser

### `Scene`

The fully-loaded asset container:

| Field | Type | Description |
|-------|------|-------------|
| `root` | `Root` | Parsed `scene.json` |
| `textures` | `BTreeMap<String, Rc<Tex>>` | Parsed `.tex` texture data |
| `jsons` | `BTreeMap<String, String>` | Raw JSON file contents (shader configs, materials) |
| `misc` | `BTreeMap<String, Vec<u8>>` | Other files (shader source, audio) |

### `Scene::new(path) -> Self`

1. Opens `.pkg` via `Pkg::new(path)`
2. Classifies each file:
   - `.tex` → parse to RGBA via `Tex::new` + `parse_to_rgba` (threaded)
   - `.json` → store as string
   - Other → store as raw bytes
3. Parses `scene.json` as `Root`
4. Returns complete `Scene`

---

## `object_loader` — Object Conversion

### `TextureObject`

| Field | Type | Description |
|-------|------|-------------|
| `texture` | `Rc<Tex>` | Parsed RGBA texture data |
| `origin` | `Vec3` | World-space position |
| `angles` | `Vec3` | Rotation (Euler, degrees) |
| `size` | `Vec2` | Width / height |
| `scale` | `Vec3` | Scale multiplier |
| `effects` | `Vec<Effect>` | Shader effects |

### `AudioObject`

| Field | Type | Description |
|-------|------|-------------|
| `sounds` | `Vec<String>` | Audio file paths |
| `playback_mode` | `PlaybackMode` | `Loop` or `Others` |

### `ObjectMap`

| Field | Type | Description |
|-------|------|-------------|
| `texture` | `Vec<TextureObject>` | Ordered texture draw list |
| `audio` | `Vec<AudioObject>` | Audio objects |

### `ObjectMap::new(objects, scene) -> Self`

1. Classifies each object as texture, audio, or transform node
2. Skips invisible objects
3. Resolves parent-child transform inheritance (accumulates angles, scale, origin)
4. Returns ordered texture and audio vectors

---

## `model` — Material Model

| Field | Type | Description |
|-------|------|-------------|
| `material` | `String` | Path to the `.tex` file to load and display |
| `autosize` | `bool` | Auto-size flag |
| `cropoffset` | `Option<String>` | Crop offset |
| `puppet` | `Option<String>` | Skeletal animation reference (`.puppet`) |

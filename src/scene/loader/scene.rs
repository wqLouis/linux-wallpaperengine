//! The scene of a wallpaper: the `scene.json` of a Wallpaper Engine package.
//!
//! The parser is deliberately lenient, because Wallpaper Engine does not
//! write the scene in one shape:
//!
//! * a field that is missing is simply not written (a scene has no
//!   `camera` when it is a particle editor preview, no `version` when it
//!   was written by an older version of the editor),
//! * a field may be `null` (`"orthogonalprojection": null`),
//! * a number may be written as a number *or* as a string that holds one,
//!   two or three space separated numbers (`"size": 2` and
//!   `"size": "2.00000 2.00000"` are the same thing).
//!
//! Every field of the scene therefore has a default, and every number is
//! read through [`Vectors`], which understands all of the shapes a number
//! comes in.

use glam::Vec3;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

pub use super::object::*;

/// The root of a `scene.json`.
///
/// Every field is optional: Wallpaper Engine writes what a scene needs and
/// nothing else.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Root {
    pub camera: Camera,
    pub general: General,
    pub objects: Vec<Object>,
    pub version: i64,
}

/// The camera of the scene.
///
/// A scene that has no camera still needs a usable one, so the default is
/// the camera of the editor: it looks at the origin from the front.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Camera {
    pub center: Vectors,
    pub eye: Vectors,
    pub up: Vectors,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            center: Vectors::Text("0 0 -1".to_string()),
            eye: Vectors::Text("0 0 0".to_string()),
            up: Vectors::Text("0 1 0".to_string()),
        }
    }
}

/// The general settings of the scene.
///
/// Every field is optional: a scene only writes the settings it uses.
/// The settings that are plain numbers are also read leniently, because
/// Wallpaper Engine writes `null` for the ones it has no value for
/// (`"clearenabled": null`).
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct General {
    pub ambientcolor: Vectors,
    #[serde(deserialize_with = "null_as_default")]
    pub bloom: bool,
    #[serde(deserialize_with = "null_as_default")]
    pub bloomhdrfeather: f64,
    #[serde(deserialize_with = "null_as_default")]
    pub bloomhdriterations: i64,
    #[serde(deserialize_with = "null_as_default")]
    pub bloomhdrscatter: f64,
    #[serde(deserialize_with = "null_as_default")]
    pub bloomhdrstrength: f64,
    #[serde(deserialize_with = "null_as_default")]
    pub bloomhdrthreshold: f64,
    #[serde(deserialize_with = "null_as_default")]
    pub bloomstrength: f64,
    #[serde(deserialize_with = "null_as_default")]
    pub bloomthreshold: f64,
    #[serde(deserialize_with = "null_as_default")]
    pub camerafade: bool,
    pub cameraparallax: Value,
    #[serde(deserialize_with = "null_as_default")]
    pub cameraparallaxamount: f64,
    #[serde(deserialize_with = "null_as_default")]
    pub cameraparallaxdelay: f64,
    pub cameraparallaxmouseinfluence: Value,
    #[serde(deserialize_with = "null_as_default")]
    pub camerapreview: bool,
    pub camerashake: Value,
    #[serde(deserialize_with = "null_as_default")]
    pub camerashakeamplitude: f64,
    #[serde(deserialize_with = "null_as_default")]
    pub camerashakeroughness: f64,
    #[serde(deserialize_with = "null_as_default")]
    pub camerashakespeed: f64,
    pub clearcolor: Vectors,
    #[serde(deserialize_with = "null_as_default")]
    pub clearenabled: bool,
    #[serde(default = "default_farz", deserialize_with = "null_as_default")]
    pub farz: f64,
    #[serde(deserialize_with = "null_as_default")]
    pub fov: f64,
    #[serde(deserialize_with = "null_as_default")]
    pub hdr: bool,
    #[serde(default = "default_nearz", deserialize_with = "null_as_default")]
    pub nearz: f64,
    /// The size of the projection.
    ///
    /// Wallpaper Engine writes `null` when the scene has no projection, so
    /// the field is read leniently.
    #[serde(deserialize_with = "null_as_default")]
    pub orthogonalprojection: Orthogonalprojection,
    pub skylightcolor: Vectors,
    #[serde(deserialize_with = "null_as_default")]
    pub zoom: f64,
    pub bloomtint: Option<Vectors>,
    pub gravitydirection: Option<Vectors>,
    pub gravitystrength: Option<f64>,
    pub perspectiveoverridefov: Option<f64>,
    pub winddirection: Option<Vectors>,
    pub windenabled: Option<bool>,
    pub windstrength: Option<f64>,
    pub lightconfig: Option<Lightconfig>,
}

/// The size of the orthographic projection of the scene.
///
/// The renderer sizes its textures after the projection, so a scene that
/// has none still needs a size that can be rendered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Orthogonalprojection {
    pub height: i64,
    pub width: i64,
}

impl Default for Orthogonalprojection {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
        }
    }
}

/// How many point and spot lights the scene may have.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Lightconfig {
    pub point: i64,
    pub spot: i64,
}

/// A number of the scene, in any of the shapes Wallpaper Engine writes.
///
/// The numeric fields of a scene come in two flavours: a plain number
/// (`"size": 2`) and a string that holds one, two or three space separated
/// numbers (`"size": "2.00000 2.00000"`).  Wallpaper Engine switched
/// between the two over time and both are found in the wild — a text
/// object writes its `padding` as a string, an older wallpaper writes it
/// as a number — so every number of the scene is read through this type.
///
/// The components are kept apart: [`Vectors::parse`] splits a `"x y"`
/// string into its components and broadcasts a single number to all the
/// axes, exactly like Wallpaper Engine's own property loader does.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(untagged)]
pub enum Vectors {
    Scaler(f64),
    Text(String),
    Object(Value),
}

impl Default for Vectors {
    fn default() -> Self {
        Vectors::Scaler(0.0)
    }
}

impl Vectors {
    /// The components of the number.
    ///
    /// A number that is written as a plain number is broadcast to all the
    /// axes, a string is split into its components and an object is read
    /// through the `value` it holds (that is how Wallpaper Engine binds a
    /// user property to a number).
    pub fn parse(&self) -> Option<Vec3> {
        match self {
            Vectors::Scaler(val) => Some(Vec3 {
                x: *val as f32,
                y: *val as f32,
                z: *val as f32,
            }),
            Vectors::Text(val) => parse_vector_text(val),
            Vectors::Object(value) => parse_vector_value(value),
        }
    }
}

/// Parse a `"x y z"` (or `"x y"`, or `"x"`) vector string.
///
/// A string that holds a single number is broadcast to all the axes, like
/// the plain number it stands for.
fn parse_vector_text(text: &str) -> Option<Vec3> {
    let mut numbers = text
        .split_whitespace()
        .map(|field| field.parse::<f32>().unwrap_or_default());

    let x = numbers.next()?;

    // A single number is broadcast to all the axes.
    let Some(y) = numbers.next() else {
        return Some(Vec3 { x, y: x, z: x });
    };

    // The third component is optional: a two dimensional number has no
    // depth.
    let z = numbers.next().unwrap_or(0.0);

    // More than three numbers are not a vector.
    if numbers.next().is_some() {
        return None;
    }

    Some(Vec3 { x, y, z })
}

/// Parse a vector from a JSON value.
///
/// Wallpaper Engine's property loader understands a nested `value` object
/// (a user property bound to a JSON value) and broadcasts a scalar to all
/// three axes, so `"scale": 1` means `1 1 1`.
fn parse_vector_value(value: &Value) -> Option<Vec3> {
    match value {
        Value::Number(n) => {
            let v = n.as_f64()? as f32;
            Some(Vec3 { x: v, y: v, z: v })
        }
        Value::String(s) => parse_vector_text(s),
        Value::Array(items) => {
            let mut out = [0.0f32; 3];
            for (i, item) in items.iter().take(3).enumerate() {
                out[i] = item.as_f64()? as f32;
            }
            Some(Vec3 {
                x: out[0],
                y: out[1],
                z: out[2],
            })
        }
        Value::Object(map) => map.get("value").and_then(parse_vector_value),
        _ => None,
    }
}

/// Read a field that Wallpaper Engine may write as `null`.
///
/// `#[serde(default)]` only fills in the fields that are *missing*, so a
/// field that is present but `null` needs its own handling: Wallpaper
/// Engine writes `"orthogonalprojection": null` for a scene that has no
/// projection.
fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// The default of the camera's near plane.
///
/// The orthographic projection divides by the distance of the two planes,
/// so they must not be degenerate.
fn default_nearz() -> f64 {
    0.01
}

/// The default of the camera's far plane.
fn default_farz() -> f64 {
    10000.0
}

/// A value that is either the value itself or an object that holds it.
///
/// Wallpaper Engine binds a user property to a JSON value by wrapping it
/// in an object: `{"value": 1}`.
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(untagged)]
pub enum BindUserProperty<T> {
    Value(T),
    Object(serde_json::Map<String, Value>),
}

impl<T: DeserializeOwned> BindUserProperty<T> {
    pub fn value(self) -> Option<T> {
        match self {
            BindUserProperty::Value(val) => Some(val),
            BindUserProperty::Object(obj) => {
                Some(serde_json::from_value::<T>(obj.get("value")?.clone()).ok()?)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_is_broadcast_to_all_axes() {
        // Wallpaper Engine's vec3 loader broadcasts a scalar ("scale": 1).
        let v = Vectors::Scaler(2.0).parse().unwrap();
        assert_eq!(v, Vec3::new(2.0, 2.0, 2.0));
    }

    #[test]
    fn text_vector_is_parsed() {
        let v = Vectors::Text("10 20".into()).parse().unwrap();
        assert_eq!(v, Vec3::new(10.0, 20.0, 0.0));
        let v = Vectors::Text("10 20 30".into()).parse().unwrap();
        assert_eq!(v, Vec3::new(10.0, 20.0, 30.0));
    }

    #[test]
    fn a_single_number_is_broadcast_to_all_axes() {
        // Wallpaper Engine writes a lone number as a string too.
        let v = Vectors::Text("10".into()).parse().unwrap();
        assert_eq!(v, Vec3::new(10.0, 10.0, 10.0));
    }

    #[test]
    fn a_vector_with_more_than_three_numbers_is_not_a_vector() {
        assert!(Vectors::Text("1 2 3 4".into()).parse().is_none());
    }

    #[test]
    fn user_property_binding_is_resolved() {
        // A nested "value" object (a user property bound to a JSON value) is
        // understood by Wallpaper Engine's property loader.
        let json = serde_json::json!({ "value": "1 2 3" });
        let v = Vectors::Object(json).parse().unwrap();
        assert_eq!(v, Vec3::new(1.0, 2.0, 3.0));

        let json = serde_json::json!({ "value": 4 });
        let v = Vectors::Object(json).parse().unwrap();
        assert_eq!(v, Vec3::new(4.0, 4.0, 4.0));
    }

    /// A scene that does not write every field still parses: the fields
    /// that are missing fall back to their default.
    #[test]
    fn a_scene_without_the_optional_fields_parses() {
        // The particle editor writes a scene without a camera and without
        // a version.
        let json = r#"{
            "general": { "clearcolor": "0 0 0" },
            "objects": []
        }"#;

        let root: Root = serde_json::from_str(json).expect("the scene must parse");
        assert_eq!(root.version, 0);
        assert!(root.objects.is_empty());
        // The camera and the projection fall back to their defaults.
        assert_eq!(root.camera.eye.parse(), Some(Vec3::new(0.0, 0.0, 0.0)));
        assert!(root.general.orthogonalprojection.width > 0);
    }

    /// A projection that is written as `null` is read as the default.
    #[test]
    fn a_null_projection_is_read_as_the_default() {
        let json = r#"{ "general": { "orthogonalprojection": null } }"#;

        let root: Root = serde_json::from_str(json).expect("the scene must parse");
        assert_eq!(
            root.general.orthogonalprojection,
            Orthogonalprojection::default()
        );
    }
}

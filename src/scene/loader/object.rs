use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::scene::Vectors;
use crate::collections::{Map};

/// An object of the scene.
///
/// Wallpaper Engine stores every element of a scene (an image, a text, a
/// particle system, a light, a camera, a plain node) in the same
/// `objects` array, so the struct holds the union of all their fields.
///
/// Every field has a default: a scene only writes the fields the element
/// it describes uses.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Object {
    pub alignment: Option<String>,
    pub alpha: Option<Value>,
    pub angles: Option<Vectors>,
    pub brightness: Option<f64>,
    pub color: Option<Vectors>,
    pub color_blend_mode: Option<i64>,
    pub copybackground: Option<bool>,
    pub effects: Vec<Effect>,
    pub id: i64,
    pub image: Option<String>,
    pub ledsource: Option<bool>,
    pub locktransforms: Option<bool>,
    pub name: String,
    pub origin: Option<Vectors>,
    /// How much the object moves with the mouse.
    ///
    /// Wallpaper Engine writes it as a `"x y"` string, but as a plain
    /// number in older wallpapers.
    pub parallax_depth: Option<Vectors>,
    pub perspective: Option<bool>,
    pub scale: Option<Vectors>,
    pub size: Option<Vectors>,
    pub solid: Option<bool>,
    pub visible: Option<super::scene::BindUserProperty<bool>>,
    pub instanceoverride: Option<Instanceoverride>,
    pub particle: Option<String>,
    pub model: Option<Value>,
    pub castshadow: Option<bool>,
    pub dependencies: Vec<Value>,
    pub anchor: Option<String>,
    pub backgroundbrightness: Option<f64>,
    pub backgroundcolor: Option<String>,
    pub blockalign: Option<bool>,
    pub depthtest: Option<String>,
    pub font: Option<String>,
    pub horizontalalign: Option<String>,
    pub limitrows: Option<bool>,
    pub limituseellipsis: Option<bool>,
    pub limitwidth: Option<bool>,
    pub maxrows: Option<i64>,
    pub maxwidth: Option<Value>,
    pub opaquebackground: Option<bool>,
    /// The padding of a text object.
    ///
    /// Wallpaper Engine writes it as a `"x y"` string (`"32.00000
    /// 32.00000"`), but as a plain number in older wallpapers.
    pub padding: Option<Vectors>,
    pub pointsize: Option<Value>,
    pub text: Option<Value>,
    pub verticalalign: Option<String>,
    pub instance: Option<Instance>,
    pub maxtime: Option<f64>,
    pub mintime: Option<f64>,
    pub muteineditor: Option<bool>,
    pub playbackmode: Option<String>,
    pub sound: Vec<String>,
    pub startsilent: Option<bool>,
    /// The spacing of a text object: a `"x y"` string.
    pub spacing: Option<Vectors>,
    pub volume: Option<Value>,
    pub parent: Option<i64>,
    pub shape: Option<String>,
    pub density: Option<f64>,
    pub exponent: Option<f64>,
    pub innercone: Option<f64>,
    pub intensity: Option<Value>,
    pub light: Option<String>,
    pub outercone: Option<f64>,
    pub radius: Option<f64>,
    pub volumetricsexponent: Option<f64>,
    pub animationlayers: Vec<Animationlayer>,
    pub attachment: Option<String>,
    pub camera: Option<String>,
    pub fov: Option<f64>,
    pub path: Option<String>,
    pub queuemode: Option<String>,
    pub zoom: Option<Zoom>,
    pub config: Option<Config>,
    pub clampuvs: Option<bool>,
    pub disablepropagation: Option<bool>,
}

/// An effect of an object.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Effect {
    pub file: String,
    pub id: i64,
    pub name: String,
    pub passes: Vec<Pass>,
    pub visible: Value,
}

/// The kind of element an [`Object`] represents.
///
/// Wallpaper Engine stores every element in the same `objects` array, so the
/// kind has to be inferred from the fields that are populated — mirroring how
/// assets are classified by path in the loader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementType {
    /// Texture-backed element (`image`).
    Image,
    /// Audio element (`sound`).
    Sound,
    /// Particle system (`particle`).
    Particle,
    /// Text element (`text` / `font`).
    Text,
    /// Light source (`light`).
    Light,
    /// Camera (`camera`).
    Camera,
    /// Transform-only element (parent for the hierarchy).
    Node,
}

impl Object {
    /// Classify this element by the fields Wallpaper Engine populated.
    pub fn element_type(&self) -> ElementType {
        if self.image.is_some() {
            ElementType::Image
        } else if !self.sound.is_empty() {
            ElementType::Sound
        } else if self.particle.is_some() {
            ElementType::Particle
        } else if self.text.is_some() || self.font.is_some() {
            ElementType::Text
        } else if self.light.is_some() {
            ElementType::Light
        } else if self.camera.is_some() {
            ElementType::Camera
        } else {
            ElementType::Node
        }
    }
}

/// A pass of an effect.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Pass {
    pub constantshadervalues: Option<Map<String, Value>>,
    pub id: i64,
    pub textures: Vec<Option<String>>,
    pub combos: Option<Map<String, i64>>,
    pub usertextures: Option<(Value, Value)>,
}

/// The overrides of a particle instance.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Instanceoverride {
    pub alpha: Option<Value>,
    pub id: Option<i64>,
    pub colorn: Option<Value>,
    pub speed: Option<f64>,
    pub size: Option<Value>,
    pub lifetime: Option<f64>,
    pub count: Option<Value>,
    pub rate: Option<Value>,
}

/// A particle instance.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Instance {
    pub combos: Map<String, Value>,
    pub id: i64,
    pub textures: Vec<String>,
    pub usertextures: Vec<Value>,
}

/// A layer of the animation of a puppet.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Animationlayer {
    pub additive: bool,
    pub animation: i64,
    pub blend: Value,
    pub blendin: bool,
    pub blendout: bool,
    pub blendtime: f64,
    pub id: i64,
    pub name: String,
    pub rate: Value,
    pub visible: Value,
}

/// The zoom of a scene.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Zoom {
    pub user: String,
    pub value: f64,
}

/// The configuration of an object.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub passthrough: bool,
}

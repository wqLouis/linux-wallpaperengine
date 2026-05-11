use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::scene::Vectors;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Object {
    pub alignment: Option<String>,
    pub alpha: Option<Value>,
    pub angles: Option<Vectors>,
    pub brightness: Option<f64>,
    pub color: Option<Vectors>,
    pub color_blend_mode: Option<i64>,
    pub copybackground: Option<bool>,
    #[serde(default)]
    pub effects: Vec<Effect>,
    pub id: i64,
    pub image: Option<String>,
    pub ledsource: Option<bool>,
    pub locktransforms: Option<bool>,
    pub name: String,
    pub origin: Option<Vectors>,
    pub parallax_depth: Option<Value>,
    pub perspective: Option<bool>,
    pub scale: Option<Vectors>,
    pub size: Option<Vectors>,
    pub solid: Option<bool>,
    pub visible: Option<super::scene::BindUserProperty<bool>>,
    pub instanceoverride: Option<Instanceoverride>,
    pub particle: Option<String>,
    pub model: Option<Value>,
    pub castshadow: Option<bool>,
    #[serde(default)]
    pub dependencies: Vec<i64>,
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
    pub padding: Option<i64>,
    pub pointsize: Option<Value>,
    pub text: Option<Value>,
    pub verticalalign: Option<String>,
    pub instance: Option<Instance>,
    pub maxtime: Option<f64>,
    pub mintime: Option<f64>,
    pub muteineditor: Option<bool>,
    pub playbackmode: Option<String>,
    #[serde(default)]
    pub sound: Vec<String>,
    pub startsilent: Option<bool>,
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
    #[serde(default)]
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

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Effect {
    pub file: String,
    pub id: i64,
    pub name: String,
    pub passes: Vec<Pass>,
    pub visible: Value,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pass {
    pub constantshadervalues: Option<BTreeMap<String, Value>>,
    pub id: i64,
    #[serde(default)]
    pub textures: Vec<Option<String>>,
    pub combos: Option<BTreeMap<String, i64>>,
    pub usertextures: Option<(Value, Value)>,
}



#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub combos: BTreeMap<String, Value>,
    pub id: i64,
    pub textures: Vec<String>,
    #[serde(default)]
    pub usertextures: Vec<Value>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Zoom {
    pub user: String,
    pub value: f64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub passthrough: bool,
}

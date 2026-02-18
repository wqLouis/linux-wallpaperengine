use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Root {
    pub camera: Camera,
    pub general: General,
    pub objects: Vec<Object>,
    pub version: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Camera {
    pub center: Vectors,
    pub eye: Vectors,
    pub up: Vectors,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct General {
    pub ambientcolor: Vectors,
    pub bloom: bool,
    pub bloomhdrfeather: f64,
    pub bloomhdriterations: i64,
    pub bloomhdrscatter: f64,
    pub bloomhdrstrength: f64,
    pub bloomhdrthreshold: f64,
    pub bloomstrength: f64,
    pub bloomthreshold: f64,
    pub camerafade: bool,
    pub cameraparallax: Value,
    pub cameraparallaxamount: f64,
    pub cameraparallaxdelay: f64,
    pub cameraparallaxmouseinfluence: Value,
    pub camerapreview: bool,
    pub camerashake: Value,
    pub camerashakeamplitude: f64,
    pub camerashakeroughness: f64,
    pub camerashakespeed: f64,
    pub clearcolor: Vectors,
    pub clearenabled: bool,
    pub farz: f64,
    pub fov: f64,
    pub hdr: bool,
    pub nearz: f64,
    pub orthogonalprojection: Orthogonalprojection,
    pub skylightcolor: Vectors,
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

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Orthogonalprojection {
    pub height: i64,
    pub width: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lightconfig {
    pub point: i64,
    pub spot: i64,
}

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
    pub visible: Option<Value>,
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
    pub constantshadervalues: Option<HashMap<String, Value>>,
    pub id: i64,
    #[serde(default)]
    pub textures: Vec<Option<String>>,
    pub combos: Option<Combos>,
    pub usertextures: Option<(Value, Value)>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Combos {
    #[serde(rename = "VERTICAL")]
    pub vertical: Option<i64>,
    #[serde(rename = "NOISE")]
    pub noise: Option<i64>,
    #[serde(rename = "ANTIALIAS")]
    pub antialias: Option<i64>,
    #[serde(rename = "A_SMOOTH_CURVE")]
    pub a_smooth_curve: Option<i64>,
    #[serde(rename = "BLENDMODE")]
    pub blendmode: Option<i64>,
    #[serde(rename = "CLIP_HIGH")]
    pub clip_high: Option<i64>,
    #[serde(rename = "CLIP_LOW")]
    pub clip_low: Option<i64>,
    #[serde(rename = "RESOLUTION")]
    pub resolution: Option<i64>,
    #[serde(rename = "SHAPE")]
    pub shape: Option<i64>,
    #[serde(rename = "TRANSPARENCY")]
    pub transparency: Option<i64>,
    #[serde(rename = "KERNEL")]
    pub kernel: Option<i64>,
    #[serde(rename = "WRITEALPHA")]
    pub writealpha: Option<i64>,
    #[serde(rename = "AUDIOPROCESSING")]
    pub audioprocessing: Option<i64>,
    #[serde(rename = "AXIS")]
    pub axis: Option<i64>,
    #[serde(rename = "REPEAT")]
    pub repeat: Option<i64>,
    #[serde(rename = "DYE")]
    pub dye: Option<i64>,
    #[serde(rename = "BACKGROUND")]
    pub background: Option<i64>,
    #[serde(rename = "MODE")]
    pub mode: Option<i64>,
    #[serde(rename = "ENABLEMASK")]
    pub enablemask: Option<i64>,
    #[serde(rename = "DIRECTDRAW")]
    pub directdraw: Option<i64>,
    #[serde(rename = "RAYCORNER")]
    pub raycorner: Option<i64>,
    #[serde(rename = "RAYMODE")]
    pub raymode: Option<i64>,
    #[serde(rename = "RENDERING")]
    pub rendering: Option<i64>,
    #[serde(rename = "DIRECTION")]
    pub direction: Option<i64>,
    #[serde(rename = "SEGMENT")]
    pub segment: Option<i64>,
    #[serde(rename = "TRANSFORM")]
    pub transform: Option<i64>,
    #[serde(rename = "V_REMAPPING")]
    pub v_remapping: Option<i64>,
    #[serde(rename = "DUALWAVES")]
    pub dualwaves: Option<i64>,
    #[serde(rename = "PRECISE")]
    pub precise: Option<i64>,
    #[serde(rename = "REF_RES")]
    pub ref_res: Option<i64>,
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
    pub combos: Combos2,
    pub id: i64,
    pub textures: Vec<String>,
    #[serde(default)]
    pub usertextures: Vec<Value>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Combos2 {
    pub version: i64,
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

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(untagged)]
pub enum Vectors {
    Scaler(f64),
    Vectors(String),
}

impl Default for Vectors {
    fn default() -> Self {
        Vectors::Scaler(0.0)
    }
}

impl Vectors {
    pub fn parse(&self) -> Option<Vec<f64>> {
        match self {
            Vectors::Scaler(s) => Some(vec![s.to_owned()]),
            Vectors::Vectors(s) => s
                .split_whitespace()
                .into_iter()
                .map(|f| f.parse::<f64>().ok())
                .collect(),
        }
    }
}

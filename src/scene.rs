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
    pub center: String,
    pub eye: String,
    pub up: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct General {
    pub ambientcolor: String,
    pub bloom: bool,
    pub bloomhdrfeather: f64,
    pub bloomhdriterations: i64,
    pub bloomhdrscatter: f64,
    pub bloomhdrstrength: f64,
    pub bloomhdrthreshold: f64,
    pub bloomstrength: f64,
    pub bloomthreshold: f64,
    pub bloomtint: String,
    pub camerafade: bool,
    pub cameraparallax: bool,
    pub cameraparallaxamount: f64,
    pub cameraparallaxdelay: f64,
    pub cameraparallaxmouseinfluence: f64,
    pub camerapreview: bool,
    pub camerashake: bool,
    pub camerashakeamplitude: f64,
    pub camerashakeroughness: f64,
    pub camerashakespeed: f64,
    pub clearcolor: String,
    pub clearenabled: bool,
    pub farz: f64,
    pub fov: f64,
    pub gravitydirection: String,
    pub gravitystrength: f64,
    pub hdr: bool,
    pub nearz: f64,
    pub orthogonalprojection: Orthogonalprojection,
    pub perspectiveoverridefov: f64,
    pub skylightcolor: String,
    pub winddirection: String,
    pub windenabled: bool,
    pub windstrength: f64,
    pub zoom: f64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Orthogonalprojection {
    pub height: i64,
    pub width: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Object {
    pub alpha: Option<f64>,
    pub castshadow: Option<bool>,
    pub id: i64,
    pub image: Option<String>,
    pub name: String,
    pub origin: String,
    pub size: Option<String>,
    pub visible: Option<bool>,
    pub angles: Option<String>,
    pub instanceoverride: Option<Instanceoverride>,
    pub particle: Option<String>,
    pub scale: Option<String>,
    #[serde(default)]
    pub effects: Vec<Effect>,
    #[serde(default)]
    pub animationlayers: Vec<Animationlayer>,
    pub attachment: Option<String>,
    pub parent: Option<i64>,
    pub locktransforms: Option<bool>,
    pub anchor: Option<String>,
    pub backgroundbrightness: Option<f64>,
    pub backgroundcolor: Option<String>,
    pub blockalign: Option<bool>,
    pub brightness: Option<f64>,
    pub color: Option<String>,
    pub depthtest: Option<String>,
    pub font: Option<String>,
    pub horizontalalign: Option<String>,
    pub limitrows: Option<bool>,
    pub limituseellipsis: Option<bool>,
    pub limitwidth: Option<bool>,
    pub maxrows: Option<i64>,
    pub maxwidth: Option<f64>,
    pub opaquebackground: Option<bool>,
    pub padding: Option<i64>,
    pub pointsize: Option<f64>,
    pub text: Option<Text>,
    pub verticalalign: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instanceoverride {
    pub alpha: f64,
    pub count: f64,
    pub id: i64,
    pub rate: f64,
    pub size: Option<f64>,
    pub speed: Option<f64>,
    pub lifetime: Option<f64>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Effect {
    pub file: String,
    pub id: i64,
    pub name: String,
    pub passes: Vec<Pass>,
    pub visible: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pass {
    pub constantshadervalues: Constantshadervalues,
    pub id: i64,
    #[serde(default)]
    pub textures: Vec<Option<String>>,
    pub combos: Option<Combos>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Constantshadervalues {
    pub alpha: Option<f64>,
    pub colorend: Option<String>,
    pub colorstart: Option<String>,
    pub distortion: Option<f64>,
    pub feather: Option<f64>,
    pub scale: Option<Value>, // String or float
    pub smoothness: Option<f64>,
    pub speed: Option<f64>,
    pub threshold: Option<f64>,
    pub phase: Option<f64>,
    pub power: Option<f64>,
    pub ratio: Option<f64>,
    pub scrolldirection: Option<f64>,
    pub speeduv: Option<f64>,
    pub strength: Option<f64>,
    pub direction: Option<f64>,
    pub exponent: Option<f64>,
    pub amount: Option<f64>,
    pub center: Option<f64>,
    pub point0: Option<String>,
    pub point1: Option<String>,
    pub size: Option<f64>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Combos {
    #[serde(rename = "ENABLEMASK")]
    pub enablemask: i64,
    #[serde(rename = "VERTICAL")]
    pub vertical: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Animationlayer {
    pub additive: bool,
    pub animation: i64,
    pub blend: f64,
    pub blendin: bool,
    pub blendout: bool,
    pub blendtime: f64,
    pub id: i64,
    pub name: String,
    pub rate: f64,
    pub visible: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Text {
    pub script: String,
    pub scriptproperties: Scriptproperties,
    pub value: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scriptproperties {
    pub add_delimiter: Option<String>,
    pub align_vertical: Option<bool>,
    pub day_format: Option<String>,
    pub month_format: Option<String>,
    pub show_day: Option<bool>,
    pub use_delimiter: Option<bool>,
    pub delimiter: Option<String>,
    pub show_seconds: Option<bool>,
    pub use24h_format: Option<bool>,
}

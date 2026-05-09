use glam::Vec3;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use super::object::*;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Root {
    pub camera: Camera,
    pub general: General,
    pub objects: Vec<Object>,
    pub version: i64,
}

/// Camera configuration parsed from `scene.json`.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Camera {
    pub center: Vectors,
    pub eye: Vectors,
    pub up: Vectors,
}

/// Scene-wide settings parsed from `scene.json`.
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

/// Orthographic projection bounds (the scene's native resolution).
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

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(untagged)]
pub enum Vectors {
    Scaler(f64),
    Vectors(String),
    Object(Value),
}

impl Default for Vectors {
    fn default() -> Self {
        Vectors::Scaler(0.0)
    }
}

impl Vectors {
    pub fn parse(&self) -> Option<Vec3> {
        match self {
            Vectors::Scaler(val) => Some(Vec3 {
                x: val.clone() as f32,
                y: val.clone() as f32,
                z: val.clone() as f32,
            }),
            Vectors::Vectors(val) => {
                let vec = val
                    .split_whitespace()
                    .into_iter()
                    .map(|f| f.parse::<f32>().unwrap_or_default())
                    .collect::<Vec<f32>>();

                match vec.as_slice() {
                    [x, y] => Some(Vec3 {
                        x: x.clone(),
                        y: y.clone(),
                        z: 0.0,
                    }),
                    [x, y, z] => Some(Vec3 {
                        x: x.clone(),
                        y: y.clone(),
                        z: z.clone(),
                    }),
                    _ => None,
                }
            }
            Vectors::Object(_) => None,
        }
    }
}

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

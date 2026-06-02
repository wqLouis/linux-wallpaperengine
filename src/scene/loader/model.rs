use serde::Deserialize;
use serde::Serialize;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Model {
    pub autosize: bool,
    pub cropoffset: Option<String>,
    pub material: String,
    pub puppet: Option<String>,
    /// `true` when the model is a solid-colour layer (no real texture).
    pub solidlayer: Option<bool>,
    /// `true` when the model passes through a runtime framebuffer
    /// (composelayer — used for effect compositing).
    pub passthrough: Option<bool>,
}

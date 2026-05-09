use serde::Deserialize;
use serde::Serialize;

/// Material model configuration from a `.json` file.
///
/// References the texture file and optional skeletal animation.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub autosize: bool,
    pub cropoffset: Option<String>,
    pub material: String,
    pub puppet: Option<String>,
}

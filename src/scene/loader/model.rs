use serde::Deserialize;
use serde::Serialize;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub autosize: bool,
    pub cropoffset: Option<String>,
    pub material: String,
    pub puppet: Option<String>,
}

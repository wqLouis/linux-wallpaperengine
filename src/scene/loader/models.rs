use serde::Deserialize;
use serde::Serialize;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Root {
    pub autosize: bool,
    pub cropoffset: Option<String>,
    pub material: String,
    pub puppet: Option<String>,
}

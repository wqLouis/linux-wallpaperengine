use std::{borrow::Cow, collections::BTreeMap};

use serde_json::Value;
use wgpu::{naga::ShaderStage, *};

pub struct ShaderEffect {
    vars: Vec<ShaderVariable>,
    combos: Option<Vec<BTreeMap<String, Value>>>,
}

pub struct ShaderVariable {
    data_type: String,
    config: Option<BTreeMap<String, Value>>,
    name: String,
    index: u32,
}

pub fn load(device: &Device, shader: String, stage: ShaderStage) {
    let shader = device.create_shader_module(ShaderModuleDescriptor {
        label: None,
        source: ShaderSource::Glsl {
            shader: Cow::Owned(shader),
            stage: stage,
            defines: &[],
        },
    });
}

impl ShaderEffect {
    pub fn new(shader: String) {
        let mut variables: Vec<ShaderVariable> = Vec::new();
        let mut combos: Vec<BTreeMap<String, Value>> = Vec::new();

        for (index, line) in shader.lines().into_iter().enumerate() {
            let mut comments: Option<BTreeMap<String, Value>> = None;

            if line.find("//").is_some() {
                let chunk: Vec<&str> = line.split("//").into_iter().collect();

                let comments_str = chunk.get(1).unwrap();

                if comments_str.find("[COMBO]").is_some() {
                    let Some(combo): Option<BTreeMap<String, Value>> =
                        serde_json::from_str(&*comments_str.replace("[COMBO]", "")).ok()
                    else {
                        continue;
                    };

                    combos.push(combo);
                } else {
                    comments = serde_json::from_str::<BTreeMap<String, Value>>(comments_str).ok();
                }
            }

            let words: Vec<&str> = line.split_whitespace().into_iter().collect();

            let Some(data_type) = words.get(1) else {
                continue;
            };
            let Some(name) = words.get(2) else {
                continue;
            };

            variables.push(ShaderVariable {
                data_type: data_type.to_string(),
                config: comments,
                name: name.to_string(),
                index: index as u32,
            });
        }
    }
}

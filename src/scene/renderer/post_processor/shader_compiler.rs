use std::{borrow::Cow, collections::BTreeMap};

use serde_json::Value;
use wgpu::{naga::ShaderStage, *};

#[derive(Debug)]
pub struct ShaderEffect {
    pub vars: Vec<ShaderVariable>,
    pub combos: Option<Vec<BTreeMap<String, Value>>>,
    pub source: String,
}

#[derive(Debug)]
pub struct ShaderVariable {
    pub data_type: String,
    pub config: Option<BTreeMap<String, Value>>,
    pub name: String,
}

pub fn load(
    device: &Device,
    shader: String,
    stage: ShaderStage,
    defines: &[(&str, &str)],
) -> ShaderModule {
    device.create_shader_module(ShaderModuleDescriptor {
        label: None,
        source: ShaderSource::Glsl {
            shader: Cow::Owned(shader),
            stage,
            defines,
        },
    })
}

impl ShaderEffect {
    pub fn new(shader: String) -> Self {
        let mut variables: Vec<ShaderVariable> = Vec::new();
        let mut combos: Vec<BTreeMap<String, Value>> = Vec::new();

        for line in shader.lines() {
            let trimmed = line.trim();

            // Skip empty lines and preprocessor directives
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Check for combo comments
            if let Some(combo) = Self::parse_combo_comment(trimmed) {
                combos.push(combo);
                continue;
            }

            // Try to parse variable declaration
            if let Some(var) = Self::parse_variable(trimmed) {
                variables.push(var);
            }
        }

        ShaderEffect {
            vars: variables,
            combos: if combos.is_empty() {
                None
            } else {
                Some(combos)
            },
            source: shader,
        }
    }

    fn parse_combo_comment(line: &str) -> Option<BTreeMap<String, Value>> {
        if let Some(comment_start) = line.find("//") {
            let comment = line[comment_start + 2..].trim();
            if let Some(combo_start) = comment.find("[COMBO]") {
                let json_str = comment[combo_start + 7..].trim();
                return serde_json::from_str(json_str).ok();
            }
        }
        None
    }

    fn parse_variable(line: &str) -> Option<ShaderVariable> {
        // Check if line has a variable declaration
        if !line.contains(';') {
            return None;
        }

        // Split line and comment
        let (decl_part, comment_part) = if let Some(comment_idx) = line.find("//") {
            (&line[..comment_idx], Some(&line[comment_idx + 2..].trim()))
        } else {
            (line, None)
        };

        // Parse config from comment
        let config = comment_part
            .and_then(|comment| serde_json::from_str::<BTreeMap<String, Value>>(comment).ok());

        // Simple parsing: look for uniform keyword then type then name
        let words: Vec<&str> = decl_part.split_whitespace().collect();

        // Find "uniform" keyword position
        let uniform_pos = words.iter().position(|&w| w == "uniform")?;

        // Type should be after uniform
        if uniform_pos + 1 >= words.len() {
            return None;
        }

        let data_type = words[uniform_pos + 1];

        // Name should be after type (could have assignment or array)
        if uniform_pos + 2 >= words.len() {
            return None;
        }

        let name_part = words[uniform_pos + 2];
        // Extract name (remove trailing semicolon, array brackets, or assignment)
        let name = name_part
            .trim_end_matches(';')
            .split('=')
            .next()
            .unwrap()
            .split('[')
            .next()
            .unwrap()
            .to_string();

        Some(ShaderVariable {
            data_type: data_type.to_string(),
            config,
            name,
        })
    }

    pub fn compile(&self, device: &Device, stage: ShaderStage) -> ShaderModule {
        // Generate defines from combos
        let mut defines_vec = Vec::new();
        let mut define_strings = Vec::new();

        if let Some(combos) = &self.combos {
            for combo in combos {
                for (key, value) in combo {
                    let value_str = match value {
                        Value::Number(n) => n.to_string(),
                        Value::String(s) => s.clone(),
                        Value::Bool(b) => b.to_string(),
                        _ => continue,
                    };
                    define_strings.push((key.clone(), value_str));
                }
            }
        }

        // Convert to slices with proper lifetimes
        for (key, value) in &define_strings {
            defines_vec.push((key.as_str(), value.as_str()));
        }

        load(device, self.source.clone(), stage, &defines_vec)
    }
}

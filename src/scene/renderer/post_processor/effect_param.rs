//! GPU uniform buffer layout and per-frame value population.
//!
//! Parses shader uniform declarations into an [`UniformLayout`] (offsets
//! + sizes), then fills and uploads the buffer each frame with system
//! values (time, projection, cursor) and material constants.

use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct UniformLayout {
    offsets: BTreeMap<String, (u64, u64)>,
    total_size: u64,
}

impl UniformLayout {
    pub fn new(decls: &[(String, String)]) -> Self {
        let mut offsets = BTreeMap::new();
        let mut offset: u64 = 0;

        for (name, ty) in decls {
            offset = align_up(offset, type_align(ty));
            let size = type_size(ty);
            offsets.insert(name.clone(), (offset, size));
            offset += size;
        }

        let total_size = align_up(offset, 16).max(16);
        UniformLayout {
            offsets,
            total_size,
        }
    }

    pub fn total_size(&self) -> u64 {
        self.total_size
    }

    /// Write a value into the uniform buffer at the offset for `name`.
    /// Returns false if `name` is not in the layout (no-op for optional uniforms).
    fn write(&self, buf: &mut [u8], name: &str, data: &[u8]) -> bool {
        if let Some(&(off, size)) = self.offsets.get(name) {
            let end = off as usize + size as usize;
            if end <= buf.len() && data.len() == size as usize {
                buf[off as usize..end].copy_from_slice(data);
                return true;
            }
        }
        false
    }

    pub fn write_f32(&self, buf: &mut [u8], name: &str, value: f32) -> bool {
        self.write(buf, name, &value.to_le_bytes())
    }

    pub fn write_vec2(&self, buf: &mut [u8], name: &str, value: [f32; 2]) -> bool {
        self.write(buf, name, bytemuck::bytes_of(&value))
    }

    pub fn write_vec3(&self, buf: &mut [u8], name: &str, value: [f32; 3]) -> bool {
        self.write(buf, name, bytemuck::bytes_of(&value))
    }

    pub fn write_vec4(&self, buf: &mut [u8], name: &str, value: [f32; 4]) -> bool {
        self.write(buf, name, bytemuck::bytes_of(&value))
    }

    /// Write a mat4 in column-major order (GLSL convention).
    pub fn write_mat4(&self, buf: &mut [u8], name: &str, value: &[[f32; 4]; 4]) -> bool {
        // Flatten column-major: value[col][row]
        let mut flat = [0u8; 64];
        for (col, row_data) in value.iter().enumerate() {
            for (row, &v) in row_data.iter().enumerate() {
                let idx = col * 16 + row * 4;
                flat[idx..idx + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        self.write(buf, name, &flat)
    }

    fn write_all_defaults(&self, buf: &mut [u8]) {
        buf.fill(0);
    }

    pub fn populate_effect_params(
        &self,
        buf: &mut [u8],
        constants: &BTreeMap<String, serde_json::Value>,
        material_keys: &BTreeMap<String, String>,
        time: f32,
        projection: &[[f32; 4]; 4],
        sys: &SystemUniforms,
    ) {
        self.write_all_defaults(buf);

        self.write_f32(buf, "g_Time", time);
        self.write_mat4(buf, "g_ModelViewProjectionMatrix", projection);

        self.write(
            buf,
            "g_Screen",
            bytemuck::bytes_of(&[
                sys.screen_resolution[0] as f32,
                sys.screen_resolution[1] as f32,
                sys.screen_resolution[0] as f32 / sys.screen_resolution[1] as f32,
            ]),
        );

        self.write_mat4(buf, "g_EffectTextureProjectionMatrix", projection);
        self.write_mat4(
            buf,
            "g_EffectTextureProjectionMatrixInverse",
            &[
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        );

        self.write_vec2(buf, "g_ParallaxPosition", sys.cursor_position);

        for (name, res) in &sys.tex_resolutions {
            self.write_vec4(buf, name, *res);
        }

        for (material_key, value) in constants {
            let uniform_name = material_keys
                .get(material_key)
                .cloned()
                .unwrap_or_else(|| material_key.clone());

            // Resolve scene values that have a script/value wrapper:
            //   {"script": "...", "value": <inner>}  →  <inner>
            let resolved = match value {
                serde_json::Value::Object(obj) => {
                    obj.get("value").cloned()
                }
                _ => None,
            };
            let value = resolved.as_ref().unwrap_or(value);

            match value {
                serde_json::Value::Number(n) => {
                    if let Some(v) = n.as_f64() {
                        let _ = self.write_f32(buf, &uniform_name, v as f32);
                    }
                }
                serde_json::Value::String(s) => {
                    let parts: Vec<f32> = s
                        .split_whitespace()
                        .filter_map(|p| p.parse::<f32>().ok())
                        .collect();
                    match parts.len() {
                        2 => {
                            let _ = self.write_vec2(buf, &uniform_name, [parts[0], parts[1]]);
                        }
                        3 => {
                            let _ = self.write_vec3(
                                buf,
                                &uniform_name,
                                [parts[0], parts[1], parts[2]],
                            );
                        }
                        4 => {
                            let _ = self.write_vec4(
                                buf,
                                &uniform_name,
                                [parts[0], parts[1], parts[2], parts[3]],
                            );
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
}

pub struct SystemUniforms {
    pub screen_resolution: [u32; 2],
    pub tex_resolutions: BTreeMap<String, [f32; 4]>,
    /// Normalized cursor position in [0, 1] range, (0,0) = top-left (UV space)
    pub cursor_position: [f32; 2],
}

impl SystemUniforms {
    #[cfg(test)]
    pub fn with_resolution(res: [u32; 2]) -> Self {
        SystemUniforms {
            screen_resolution: res,
            tex_resolutions: BTreeMap::new(),
            cursor_position: [0.0, 0.0],
        }
    }
}

fn align_up(val: u64, align: u64) -> u64 {
    (val + align - 1) & !(align - 1)
}

fn type_align(ty: &str) -> u64 {
    match ty {
        "mat4" | "mat3" | "vec4" | "vec3" => 16,
        "vec2" => 8,
        _ => 4,
    }
}

fn type_size(ty: &str) -> u64 {
    match ty {
        "mat4" => 64,
        "mat3" => 48,
        "vec4" => 16,
        "vec3" => 12,
        "vec2" => 8,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uniform_layout_waterwaves() {
        let decls = vec![
            ("g_Direction".to_string(), "float".to_string()),
            ("g_Exponent".to_string(), "float".to_string()),
            (
                "g_ModelViewProjectionMatrix".to_string(),
                "mat4".to_string(),
            ),
            ("g_Scale".to_string(), "float".to_string()),
            ("g_Speed".to_string(), "float".to_string()),
            ("g_Strength".to_string(), "float".to_string()),
            ("g_Texture1Resolution".to_string(), "vec4".to_string()),
            ("g_Time".to_string(), "float".to_string()),
        ];
        let layout = UniformLayout::new(&decls);
        let mut buf = vec![0u8; layout.total_size() as usize];
        let sys = SystemUniforms::with_resolution([1920, 1080]);

        layout.populate_effect_params(
            &mut buf,
            &BTreeMap::new(),
            &BTreeMap::new(),
            1.5,
            &[
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            &sys,
        );

        assert!(layout.write_f32(&mut buf, "g_Speed", 5.0));
        assert!(layout.write_f32(&mut buf, "g_Time", 1.5));
        assert!(layout.write_mat4(
            &mut buf,
            "g_ModelViewProjectionMatrix",
            &[
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ]
        ));
    }
}

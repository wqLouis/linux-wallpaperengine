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

    pub fn write_f32(&self, buf: &mut [u8], name: &str, value: f32) -> bool {
        if let Some(&(off, _)) = self.offsets.get(name) {
            let end = off as usize + 4;
            if end <= buf.len() {
                buf[off as usize..end].copy_from_slice(&value.to_le_bytes());
                return true;
            }
        }
        false
    }

    pub fn write_vec2(&self, buf: &mut [u8], name: &str, value: [f32; 2]) -> bool {
        if let Some(&(off, _)) = self.offsets.get(name) {
            let end = off as usize + 8;
            if end <= buf.len() {
                buf[off as usize..off as usize + 4].copy_from_slice(&value[0].to_le_bytes());
                buf[off as usize + 4..end].copy_from_slice(&value[1].to_le_bytes());
                return true;
            }
        }
        false
    }

    pub fn write_vec3(&self, buf: &mut [u8], name: &str, value: [f32; 3]) -> bool {
        if let Some(&(off, _)) = self.offsets.get(name) {
            let end = off as usize + 12;
            if end <= buf.len() {
                buf[off as usize..off as usize + 4].copy_from_slice(&value[0].to_le_bytes());
                buf[off as usize + 4..off as usize + 8].copy_from_slice(&value[1].to_le_bytes());
                buf[off as usize + 8..end].copy_from_slice(&value[2].to_le_bytes());
                return true;
            }
        }
        false
    }

    pub fn write_vec4(&self, buf: &mut [u8], name: &str, value: [f32; 4]) -> bool {
        if let Some(&(off, _)) = self.offsets.get(name) {
            let end = off as usize + 16;
            if end <= buf.len() {
                for (i, v) in value.iter().enumerate() {
                    buf[off as usize + i * 4..off as usize + i * 4 + 4]
                        .copy_from_slice(&v.to_le_bytes());
                }
                return true;
            }
        }
        false
    }

    pub fn write_mat4(&self, buf: &mut [u8], name: &str, value: &[[f32; 4]; 4]) -> bool {
        if let Some(&(off, _)) = self.offsets.get(name) {
            let end = off as usize + 64;
            if end <= buf.len() {
                for (col, row_data) in value.iter().enumerate() {
                    for (row, &v) in row_data.iter().enumerate() {
                        let idx = off as usize + col * 16 + row * 4;
                        buf[idx..idx + 4].copy_from_slice(&v.to_le_bytes());
                    }
                }
                return true;
            }
        }
        false
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

        self.write_vec3(
            buf,
            "g_Screen",
            [
                sys.screen_resolution[0] as f32,
                sys.screen_resolution[1] as f32,
                sys.screen_resolution[0] as f32 / sys.screen_resolution[1] as f32,
            ],
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

        self.write_vec2(buf, "g_ParallaxPosition", [0.0, 0.0]);

        for (name, res) in &sys.tex_resolutions {
            self.write_vec4(buf, name, *res);
        }

        for (material_key, value) in constants {
            let uniform_name = material_keys
                .get(material_key)
                .cloned()
                .unwrap_or_else(|| material_key.clone());

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
}

impl SystemUniforms {
    pub fn with_resolution(res: [u32; 2]) -> Self {
        SystemUniforms {
            screen_resolution: res,
            tex_resolutions: BTreeMap::new(),
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

use indicatif::ProgressBar;
use pkg_parser::pkg_parser::{mdl_parser::MdlFile, parser::Pkg, tex_parser::Tex};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};

use super::assets_loader::{Asset, AssetStore, AssetType};

pub struct Scene {
    pub root: crate::scene::loader::scene::Root,
    pub assets: AssetStore,
}

impl Scene {
    /// Load a wallpaper scene from a .pkg file.
    ///
    /// The archive is trusted to be well-formed: an unreadable package, an
    /// invalid asset or a missing/invalid `scene.json` is fatal.
    ///
    /// `show_progress` controls whether the extraction progress bar is
    /// drawn. Set it to `false` when debug/trace logging is enabled,
    /// because the progress bar's redraw would otherwise eat those logs.
    ///
    /// When `no_mdl` is set, `.mdl` files are not parsed at all (they are
    /// unused without puppet meshes).
    pub fn new(path: String, show_progress: bool, no_mdl: bool) -> Self {
        let path = Path::new(&path);
        let pkg = Pkg::new(path).unwrap_or_else(|e| {
            panic!("Failed to load PKG file '{}': {}", path.display(), e);
        });

        let mut assets: BTreeMap<String, Asset> = BTreeMap::new();
        // Texture decoding is the expensive part, so collect the textures
        // first and decode them in parallel below.
        let mut textures: Vec<(String, Vec<u8>)> = Vec::new();

        // A no-op stand-in used when the progress bar is disabled. All
        // `pb.inc(1)` / `pb.finish_and_clear()` calls below then become
        // free no-ops without scattering `if show_progress` checks.
        let pb = if show_progress {
            ProgressBar::new(pkg.files.len() as u64)
        } else {
            ProgressBar::hidden()
        };

        for (key, val) in pkg.files.into_iter() {
            match AssetType::from_path(&key) {
                AssetType::Texture => textures.push((key, val)),

                AssetType::Model => {
                    pb.inc(1);
                    if no_mdl {
                        log::debug!("pkg: skipping mdl (--no-mdl): {}", key);
                        continue;
                    }
                    // The MDL format is only partially reverse-engineered, so the
                    // parser is allowed to fail: log the broken file and skip it
                    // rather than letting one model break the whole wallpaper.
                    match MdlFile::new(&val) {
                        Some(mdl) => {
                            log::debug!(
                                "pkg: loaded mdl: {} ({} records, {} triangles)",
                                key,
                                mdl.data.records.len(),
                                mdl.data.triangles.len()
                            );
                            assets.insert(key, Asset::Model(Rc::new(mdl)));
                        }
                        None => {
                            log::error!(
                                "pkg: failed to parse mdl '{}' ({} bytes) - skipping",
                                key,
                                val.len()
                            );
                        }
                    }
                }

                _ => {
                    pb.inc(1);
                    let asset = Asset::parse(&key, &val)
                        .unwrap_or_else(|| panic!("failed to parse asset: {}", key));
                    assets.insert(key, asset);
                }
            }
        }

        for (key, tex) in parse_textures(textures, &pb) {
            assets.insert(key, Asset::Texture(Rc::new(tex)));
        }

        pb.finish_and_clear();

        let assets = AssetStore::new(assets, None);
        let scene_string = assets
            .json("scene.json")
            .unwrap_or_else(|| panic!("scene.json not found in PKG archive"));
        let root: crate::scene::loader::scene::Root = serde_json::from_str(scene_string.as_str())
            .unwrap_or_else(|e| panic!("Failed to parse scene.json: {}", e));

        Self { root, assets }
    }

    /// Set the Wallpaper Engine assets directory for lazy-loading fallback.
    ///
    /// When a requested asset is not found in the store (populated from the
    /// `.pkg` file), the store reads it from `{assets_path}/{key}` on disk,
    /// parses it, caches it, and returns it.
    pub fn set_assets_path(&mut self, assets_path: PathBuf) {
        self.assets.set_assets_path(Some(assets_path));
    }

    /// Pre-parse shader programs now that the assets directory is known.
    pub fn prepare(&mut self) {
        self.assets.preparse_shaders();
    }
}

/// Decode textures with a bounded worker pool.
///
/// Uses one worker per available CPU core (capped at the number of textures)
/// and hands work out through an atomic index, so threads stay busy even when
/// individual textures decode at very different speeds.
fn parse_textures(textures: Vec<(String, Vec<u8>)>, pb: &ProgressBar) -> Vec<(String, Tex)> {
    if textures.is_empty() {
        return Vec::new();
    }

    let workers = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(textures.len());
    let next = AtomicUsize::new(0);

    thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);

        for _ in 0..workers {
            let next = &next;
            let textures = &textures;
            handles.push(scope.spawn(move || {
                let mut decoded = Vec::new();
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some((key, val)) = textures.get(index) else {
                        break;
                    };

                    let mut tex =
                        Tex::new(val).unwrap_or_else(|| panic!("invalid texture: {}", key));
                    tex.parse_to_rgba()
                        .unwrap_or_else(|| panic!("failed to decode texture: {}", key));
                    tex.build_mip_chain();

                    pb.inc(1);
                    decoded.push((key.clone(), tex));
                }
                decoded
            }));
        }

        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("texture worker panicked"))
            .collect()
    })
}

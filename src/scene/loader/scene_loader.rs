use indicatif::ProgressBar;
use pkg_parser::pkg_parser::{mdl_parser::MdlFile, parser::Pkg, tex_parser::Tex};
use std::{
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};

use super::assets_loader::{Asset, AssetStore, AssetType, VideoAsset, is_video_payload};
use super::project::{Project, WallpaperKind};
use super::scene::{
    BindUserProperty, Camera, General, Object, Orthogonalprojection, Root, Vectors,
};
use crate::collections::{Map, new_map};

/// The key of the synthetic material of a video wallpaper.
const VIDEO_MATERIAL: &str = "materials/video.json";
/// The key of the synthetic model of a video wallpaper.
const VIDEO_MODEL: &str = "models/video.json";
/// The key of the synthetic video asset of a video wallpaper.
const VIDEO_ASSET: &str = "materials/video.mp4";
/// The name of the texture the synthetic material binds.
const VIDEO_TEXTURE: &str = "video";

/// Why a wallpaper could not be loaded.
///
/// The loader reports the problem instead of panicking: a wallpaper that
/// cannot be loaded is skipped and the application stays alive.
#[derive(Debug)]
pub enum SceneError {
    /// The `.pkg` archive could not be read.
    Package(String),
    /// The archive holds no `scene.json`.
    MissingScene,
    /// The `scene.json` of the archive could not be parsed.
    Scene(String),
    /// The package is a video wallpaper, but it holds no video.
    MissingVideo,
}

impl std::fmt::Display for SceneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SceneError::Package(reason) => {
                write!(f, "the package could not be read: {}", reason)
            }
            SceneError::MissingScene => write!(f, "the package holds no scene.json"),
            SceneError::Scene(reason) => {
                write!(f, "the scene.json could not be parsed: {}", reason)
            }
            SceneError::MissingVideo => write!(f, "the video wallpaper holds no video"),
        }
    }
}

impl std::error::Error for SceneError {}

pub struct Scene {
    pub root: Root,
    pub assets: AssetStore,
    /// The package's `project.json`, when the archive has one.
    ///
    /// It tells the loader *what kind* of wallpaper the package holds (scene,
    /// video, web, …) — a scene package is described by `scene.json`, a video
    /// package by a single looping clip.
    #[allow(dead_code)]
    pub project: Option<Project>,
}

impl Scene {
    /// Load a wallpaper scene from a .pkg file.
    ///
    /// A wallpaper that cannot be loaded does not take the application
    /// down: the reason is logged and an empty scene is returned, so the
    /// wallpaper is skipped and the application stays alive.
    ///
    /// `show_progress` controls whether the extraction progress bar is
    /// drawn. Set it to `false` when debug/trace logging is enabled,
    /// because the progress bar's redraw would otherwise eat those logs.
    ///
    /// When `no_mdl` is set, `.mdl` files are not parsed at all (they are
    /// unused without puppet meshes).
    pub fn new(path: String, show_progress: bool, no_mdl: bool) -> Self {
        match Self::try_new(path.clone(), show_progress, no_mdl) {
            Ok(scene) => scene,
            Err(error) => {
                log::error!("failed to load the wallpaper '{}': {}", path, error);
                Scene::empty()
            }
        }
    }

    /// Load a wallpaper scene from a .pkg file, reporting why it failed.
    ///
    /// The errors are the ones [`Scene::new`] logs: an unreadable package,
    /// a missing or invalid `scene.json` and a video wallpaper without a
    /// video.
    pub fn try_new(path: String, show_progress: bool, no_mdl: bool) -> Result<Self, SceneError> {
        let path = Path::new(&path);
        let pkg = Pkg::new(path).map_err(|error| SceneError::Package(error.to_string()))?;

        let mut assets: Map<String, Asset> = new_map();
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
                    // The MDL format is only partially documented, so the
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
                    // An asset that cannot be parsed is skipped: the rest
                    // of the wallpaper may still be usable.
                    match Asset::parse(&key, &val) {
                        Some(asset) => {
                            assets.insert(key, asset);
                        }
                        None => {
                            log::error!("pkg: failed to parse asset '{}' - skipping", key);
                        }
                    }
                }
            }
        }

        for (key, decoded) in parse_textures(textures, &pb) {
            let asset = match decoded {
                DecodedTexture::Image(tex) => Asset::Texture(Rc::new(tex)),
                DecodedTexture::Video(bytes) => {
                    Asset::Video(Rc::new(VideoAsset::from_bytes(Rc::from(bytes))))
                }
            };
            assets.insert(key, asset);
        }

        pb.finish_and_clear();

        let assets = AssetStore::new(assets, None);

        // The project manifest describes the wallpaper as a whole (its title
        // and what kind of wallpaper it is).
        let project = assets.project();
        if let Some(project) = &project {
            log::info!(
                "wallpaper '{}' ({:?})",
                if project.title.is_empty() {
                    "<untitled>"
                } else {
                    &project.title
                },
                project.kind
            );
        }

        let root: Root = match &project {
            // A video wallpaper has no scene.json at all: it is brought up
            // as a synthetic scene, so the whole pipeline (projection, fit
            // modes, effects, adapters) works as it does for a scene.
            Some(project) if project.kind == WallpaperKind::Video => {
                Self::video_scene(&assets, project).ok_or(SceneError::MissingVideo)?
            }
            _ => {
                let scene_string = assets.json("scene.json").ok_or(SceneError::MissingScene)?;
                serde_json::from_str(scene_string.as_str())
                    .map_err(|error| SceneError::Scene(error.to_string()))?
            }
        };

        Ok(Self {
            root,
            assets,
            project,
        })
    }

    /// An empty scene.
    ///
    /// It is the scene of a wallpaper that could not be loaded: the
    /// wallpaper is skipped, but the application still has a projection to
    /// render (an empty picture).
    fn empty() -> Self {
        Self {
            root: Root::default(),
            assets: AssetStore::new(new_map(), None),
            project: None,
        }
    }

    /// Bring up a video wallpaper as a synthetic scene.
    ///
    /// A video package has no `scene.json`: it holds a single looping clip.
    /// The scene is faked with one image object that covers the whole
    /// projection and points at a synthetic material bound to the video, so
    /// the whole existing pipeline — camera projection, fit modes, effects,
    /// adapters — works exactly like it does for a scene wallpaper.
    fn video_scene(assets: &AssetStore, project: &Project) -> Option<Root> {
        let video = find_video(assets, project)?;

        // The synthetic material and model: the material binds the video
        // texture, the model points at the material.
        assets.insert(
            VIDEO_MATERIAL,
            Asset::Json(Rc::new(
                r#"{"passes":[{"textures":["video"]}]}"#.to_string(),
            )),
        );
        assets.insert(
            VIDEO_MODEL,
            Asset::Json(Rc::new(
                r#"{"material":"materials/video.json","autosize":false}"#.to_string(),
            )),
        );

        // Bind the video under a name the material can refer to.  The asset
        // that is already there is reused when it is the same video.
        let video = match assets.video(VIDEO_ASSET) {
            Some(bound) if Rc::ptr_eq(&bound, &video) => bound,
            _ => {
                assets.insert(VIDEO_ASSET, Asset::Video(Rc::clone(&video)));
                video
            }
        };

        Some(build_video_root(&video))
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

/// Find the video of a video wallpaper.
///
/// The manifest names the file ("video.mp4"); when the name does not match
/// the archive's layout, the first video of the package is used.
fn find_video(assets: &AssetStore, project: &Project) -> Option<Rc<VideoAsset>> {
    if let Some(file) = project.file.as_deref() {
        if let Some(video) = assets.video(file) {
            return Some(video);
        }
        // The manifest may name the file with a path the archive does not
        // use ("Video/video.mp4"): try the bare file name.
        if let Some(name) = file.rsplit('/').next()
            && let Some(video) = assets.video(name)
        {
            return Some(video);
        }
    }

    assets.first_video().map(|(_, video)| video)
}

/// Build the synthetic scene of a video wallpaper.
///
/// The scene holds a single image object that covers the whole projection,
/// which is the video's own resolution.
fn build_video_root(video: &VideoAsset) -> Root {
    let width = i64::from(video.info.width.max(1));
    let height = i64::from(video.info.height.max(1));

    Root {
        camera: Camera {
            center: Vectors::Text("0 0 -1".to_string()),
            eye: Vectors::Text("0 0 0".to_string()),
            up: Vectors::Text("0 1 0".to_string()),
        },
        general: General {
            orthogonalprojection: Orthogonalprojection { width, height },
            clearcolor: Vectors::Text("0 0 0".to_string()),
            // The camera's near and far planes: the orthographic projection
            // divides by their distance, so they must not be degenerate.
            nearz: 0.01,
            farz: 10000.0,
            ..Default::default()
        },
        objects: vec![Object {
            id: 1,
            name: VIDEO_TEXTURE.to_string(),
            image: Some(VIDEO_MODEL.to_string()),
            origin: Some(Vectors::Text("0 0".to_string())),
            size: Some(Vectors::Text(format!("{width} {height}"))),
            // The scene's origin is its bottom-left corner, so the video is
            // anchored there and covers the whole projection.
            alignment: Some("bottomleft".to_string()),
            visible: Some(BindUserProperty::Value(true)),
            ..Default::default()
        }],
        version: 1,
    }
}

/// What a worker thread hands back for a decoded `.tex` file.
///
/// The parsed assets are not `Send` (they hold `Rc`s), so the workers hand
/// back the plain data and the main thread builds the assets.
enum DecodedTexture {
    /// A texture with its pixels.
    Image(Tex),
    /// A `.tex` that carries a video container: the raw container bytes.
    Video(Vec<u8>),
}

/// Decode textures with a bounded worker pool.
///
/// Uses one worker per available CPU core (capped at the number of textures)
/// and hands work out through an atomic index, so threads stay busy even when
/// individual textures decode at very different speeds.
///
/// A `.tex` file that carries a video container (Wallpaper Engine stores its
/// video textures that way) is not decoded here: it becomes a video asset.
fn parse_textures(
    textures: Vec<(String, Vec<u8>)>,
    pb: &ProgressBar,
) -> Vec<(String, DecodedTexture)> {
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

                    // A texture that cannot be parsed is skipped: the rest
                    // of the wallpaper may still be usable.
                    let Some(mut tex) = Tex::new(val) else {
                        pb.inc(1);
                        log::error!("pkg: invalid texture '{}' - skipping", key);
                        continue;
                    };

                    // A video texture: the payload is the video container,
                    // not pixels.
                    if is_video_payload(&tex.payload) {
                        pb.inc(1);
                        decoded.push((key.clone(), DecodedTexture::Video(tex.payload)));
                        continue;
                    }

                    if tex.parse_to_rgba().is_none() {
                        pb.inc(1);
                        log::error!("pkg: failed to decode texture '{}' - skipping", key);
                        continue;
                    }
                    tex.build_mip_chain();

                    pb.inc(1);
                    decoded.push((key.clone(), DecodedTexture::Image(tex)));
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

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;
    use pkg_parser::pkg_parser::video_parser::VideoInfo;

    /// A video asset of the given size.
    fn video_asset(width: u32, height: u32) -> Rc<VideoAsset> {
        Rc::new(VideoAsset {
            bytes: Rc::from(&b"not a real video"[..]),
            info: VideoInfo {
                width,
                height,
                ..Default::default()
            },
            // The clip was not decoded: the renderer streams it.
            frames: None,
        })
    }

    /// The synthetic scene of a video wallpaper is a single fullscreen image.
    #[test]
    fn the_video_scene_is_a_single_fullscreen_image() {
        let video = video_asset(1920, 1080);
        let root = build_video_root(&video);

        assert_eq!(root.objects.len(), 1);

        let object = &root.objects[0];
        assert_eq!(object.id, 1);
        assert_eq!(object.image.as_deref(), Some(VIDEO_MODEL));
        assert_eq!(object.alignment.as_deref(), Some("bottomleft"));
        assert_eq!(
            object.origin.as_ref().and_then(|origin| origin.parse()),
            Some(Vec3::new(0.0, 0.0, 0.0)),
        );
        assert_eq!(
            object.size.as_ref().and_then(|size| size.parse()),
            Some(Vec3::new(1920.0, 1080.0, 0.0)),
        );

        // The projection is the video's own resolution.
        assert_eq!(root.general.orthogonalprojection.width, 1920);
        assert_eq!(root.general.orthogonalprojection.height, 1080);

        // The background is black and the camera is not degenerate.
        assert_eq!(
            root.general.clearcolor.parse(),
            Some(Vec3::new(0.0, 0.0, 0.0))
        );
        assert!(root.general.nearz > 0.0);
        assert!(root.general.farz > root.general.nearz);
    }

    /// The synthetic scene injects the material and the model the normal
    /// resolution path expects.
    #[test]
    fn the_video_scene_injects_its_material_and_model() {
        let mut map = new_map();
        map.insert("video.mp4".to_string(), Asset::Video(video_asset(640, 480)));
        let assets = AssetStore::new(map, None);

        let project = Project {
            title: "My Video".to_string(),
            kind: WallpaperKind::Video,
            file: Some("video.mp4".to_string()),
            preview: None,
        };

        let root = Scene::video_scene(&assets, &project).expect("the video scene must be built");

        assert_eq!(root.objects.len(), 1);
        assert_eq!(root.general.orthogonalprojection.width, 640);
        assert_eq!(root.general.orthogonalprojection.height, 480);

        // The material binds the video texture…
        let material = assets
            .json(VIDEO_MATERIAL)
            .expect("the material must be there");
        assert!(material.contains(r#""textures":["video"]"#));

        // …and the model points at the material.
        let model = assets.json(VIDEO_MODEL).expect("the model must be there");
        assert!(model.contains(VIDEO_MATERIAL));

        // The video is bound under the name the material refers to.
        assert!(assets.video(VIDEO_ASSET).is_some());
    }

    /// A video wallpaper without a video file has no scene to build.
    #[test]
    fn a_video_package_without_a_video_has_no_scene() {
        let assets = AssetStore::new(new_map(), None);
        let project = Project {
            title: "My Video".to_string(),
            kind: WallpaperKind::Video,
            file: Some("video.mp4".to_string()),
            preview: None,
        };

        assert!(Scene::video_scene(&assets, &project).is_none());
    }

    /// The synthetic scene resolves to a video object through the normal
    /// resolution path (model → material → video).
    #[test]
    fn the_video_scene_resolves_to_a_video_object() {
        use crate::scene::loader::object_loader::{ObjectMap, TextureSource};

        let mut map = new_map();
        map.insert("video.mp4".to_string(), Asset::Video(video_asset(640, 480)));
        let assets = AssetStore::new(map, None);

        let project = Project {
            title: "My Video".to_string(),
            kind: WallpaperKind::Video,
            file: Some("video.mp4".to_string()),
            preview: None,
        };

        let root = Scene::video_scene(&assets, &project).expect("the video scene must be built");
        let scene = Scene {
            root,
            assets,
            project: Some(project),
        };

        let objects =
            ObjectMap::with_clear_color(&scene.root.objects.clone(), &scene, Vec3::ZERO, true);

        assert_eq!(objects.texture.len(), 1);
        match &objects.texture[0].source {
            TextureSource::Video(video) => {
                assert_eq!(video.info.width, 640);
                assert_eq!(video.info.height, 480);
            }
            TextureSource::Static(_) => panic!("the object must be backed by the video"),
        }
    }
}

#[cfg(test)]
mod pkg_tests {
    use super::*;
    use crate::scene::loader::object_loader::{ObjectMap, TextureSource};
    use glam::Vec3;

    /// A tiny h264 clip (32×32) from the parser's test data.
    const SAMPLE_MP4: &[u8] = include_bytes!("../../pkg_parser/tests/data/sample.mp4");

    /// Build a `.pkg` archive around the given files.
    ///
    /// The format is the one the Wallpaper Engine packages use: a header
    /// (the version string and the file count), the file entries (the
    /// path, the offset and the size) and finally the file data.
    fn make_pkg(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();

        let version = b"P3\0";
        out.extend_from_slice(&(version.len() as u32).to_le_bytes());
        out.extend_from_slice(version);
        out.extend_from_slice(&(files.len() as u32).to_le_bytes());

        let mut offset = 0u32;
        for (path, data) in files {
            out.extend_from_slice(&(path.len() as u32).to_le_bytes());
            out.extend_from_slice(path.as_bytes());
            out.extend_from_slice(&offset.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            offset += data.len() as u32;
        }

        for (_, data) in files {
            out.extend_from_slice(data);
        }

        out
    }

    /// A video package comes up as a synthetic scene instead of panicking
    /// about the missing `scene.json`.
    #[test]
    fn a_video_package_comes_up_as_a_synthetic_scene() {
        let files: Vec<(&str, &[u8])> = vec![
            (
                "project.json",
                br#"{"title":"My Video","type":"Video","file":"video.mp4"}"#,
            ),
            ("video.mp4", SAMPLE_MP4),
        ];

        let path = std::env::temp_dir().join(format!(
            "linux-wallpaperengine-video-{}.pkg",
            std::process::id()
        ));
        std::fs::write(&path, make_pkg(&files)).expect("the package must be written");

        let scene = Scene::new(path.to_string_lossy().to_string(), false, true);
        let _ = std::fs::remove_file(&path);

        // The manifest is read…
        let project = scene.project.as_ref().expect("the manifest must be there");
        assert_eq!(project.kind, WallpaperKind::Video);

        // …and the scene is the synthetic one: a single image object that
        // covers the projection, which is the video's resolution.
        assert_eq!(scene.root.objects.len(), 1);
        assert_eq!(scene.root.general.orthogonalprojection.width, 32);
        assert_eq!(scene.root.general.orthogonalprojection.height, 32);

        // The video is in the store…
        assert!(scene.assets.video("video.mp4").is_some());

        // …and the object resolves to it.
        let objects =
            ObjectMap::with_clear_color(&scene.root.objects.clone(), &scene, Vec3::ZERO, true);
        assert_eq!(objects.texture.len(), 1);
        assert!(matches!(objects.texture[0].source, TextureSource::Video(_)));
    }

    /// A scene package is still loaded from its `scene.json`.
    #[test]
    fn a_scene_package_is_still_loaded_from_its_scene_json() {
        // The scene.json of a real package holds every field of the scene
        // types, so the default scene is serialized to get a valid one.
        let mut root = serde_json::to_value(Root::default()).expect("the scene must serialize");
        root["general"]["orthogonalprojection"]["width"] = serde_json::json!(800);
        root["general"]["orthogonalprojection"]["height"] = serde_json::json!(600);
        let scene_json = serde_json::to_string(&root).expect("the scene must serialize");

        let files: Vec<(&str, &[u8])> = vec![
            (
                "project.json",
                br#"{"title":"My Scene","type":"Scene","file":"scene.json"}"#,
            ),
            ("scene.json", scene_json.as_bytes()),
        ];

        let path = std::env::temp_dir().join(format!(
            "linux-wallpaperengine-scene-{}.pkg",
            std::process::id()
        ));
        std::fs::write(&path, make_pkg(&files)).expect("the package must be written");

        let scene = Scene::new(path.to_string_lossy().to_string(), false, true);
        let _ = std::fs::remove_file(&path);

        assert_eq!(scene.root.general.orthogonalprojection.width, 800);
        assert_eq!(scene.root.general.orthogonalprojection.height, 600);
        assert!(scene.root.objects.is_empty());
    }

    /// A text object of a real wallpaper.
    ///
    /// Wallpaper Engine writes the numbers of a text object as strings that
    /// hold the components of the number.
    const TEXT_OBJECT_SCENE: &str = r#"{
        "camera": { "center": "0 0 -1", "eye": "0 0 0", "up": "0 1 0" },
        "general": {
            "clearcolor": "0 0 0",
            "orthogonalprojection": { "height": 600, "width": 800 }
        },
        "objects": [
            {
                "id": 1,
                "name": "text",
                "font": "fonts/arial.ttf",
                "origin": "2051.18604 1134.85095 0.00000",
                "padding": "32.00000 32.00000",
                "parallaxDepth": "0.00000 0.00000",
                "size": "2.00000 2.00000",
                "spacing": "0.00000 0.00000"
            }
        ],
        "version": 3
    }"#;

    /// The same scene, with the numbers written as plain numbers.
    const PLAIN_NUMBER_SCENE: &str = r#"{
        "camera": { "center": "0 0 -1", "eye": "0 0 0", "up": "0 1 0" },
        "general": {
            "clearcolor": "0 0 0",
            "orthogonalprojection": { "height": 600, "width": 800 }
        },
        "objects": [
            {
                "id": 1,
                "name": "text",
                "font": "fonts/arial.ttf",
                "origin": 2051.18604,
                "padding": 32,
                "parallaxDepth": 0,
                "size": 2,
                "spacing": 0
            }
        ],
        "version": 3
    }"#;

    /// Write a scene package with the given `scene.json` and load it.
    ///
    /// The name of the package is unique per test: the tests run in
    /// parallel and every one of them writes its own package.
    fn load_scene_package(name: &str, scene_json: &str) -> Scene {
        let files: Vec<(&str, &[u8])> = vec![
            (
                "project.json",
                br#"{"title":"My Scene","type":"Scene","file":"scene.json"}"#,
            ),
            ("scene.json", scene_json.as_bytes()),
        ];

        let path = std::env::temp_dir().join(format!(
            "linux-wallpaperengine-{}-{}.pkg",
            name,
            std::process::id()
        ));
        std::fs::write(&path, make_pkg(&files)).expect("the package must be written");

        let scene = Scene::new(path.to_string_lossy().to_string(), false, true);
        let _ = std::fs::remove_file(&path);
        scene
    }

    /// Whether two numbers are the same (the precision of a `f32` is not
    /// exact, so the numbers of a scene are compared with a tolerance).
    fn close(left: f32, right: f32) -> bool {
        (left - right).abs() < 0.001
    }

    /// The numbers of a text object are written as strings that hold their
    /// components: the scene has to parse and the components have to be
    /// the numbers of the string.
    #[test]
    fn a_text_object_with_its_numbers_as_strings_parses() {
        let scene = load_scene_package("text-object", TEXT_OBJECT_SCENE);

        assert_eq!(scene.root.objects.len(), 1);
        let object = &scene.root.objects[0];

        // The padding is the two numbers of the string.
        let padding = object
            .padding
            .as_ref()
            .and_then(|padding| padding.parse())
            .expect("the padding must be there");
        assert!(close(padding.x, 32.0));
        assert!(close(padding.y, 32.0));

        // The origin and the size are split into their components too.
        let origin = object
            .origin
            .as_ref()
            .and_then(|origin| origin.parse())
            .expect("the origin must be there");
        assert!(close(origin.x, 2051.186_f32));
        assert!(close(origin.y, 1134.851_f32));
        assert!(close(origin.z, 0.0));

        let size = object
            .size
            .as_ref()
            .and_then(|size| size.parse())
            .expect("the size must be there");
        assert!(close(size.x, 2.0));
        assert!(close(size.y, 2.0));

        // The spacing and the parallax depth are read as well.
        let spacing = object
            .spacing
            .as_ref()
            .and_then(|spacing| spacing.parse())
            .expect("the spacing must be there");
        assert!(close(spacing.x, 0.0));
        assert!(close(spacing.y, 0.0));

        let depth = object
            .parallax_depth
            .as_ref()
            .and_then(|depth| depth.parse())
            .expect("the parallax depth must be there");
        assert!(close(depth.x, 0.0));
        assert!(close(depth.y, 0.0));
    }

    /// The same scene with the numbers written as plain numbers parses as
    /// well.
    #[test]
    fn the_same_scene_with_plain_numbers_parses() {
        let scene = load_scene_package("plain-numbers", PLAIN_NUMBER_SCENE);

        assert_eq!(scene.root.objects.len(), 1);
        let object = &scene.root.objects[0];

        // The padding is the plain number.
        let padding = object
            .padding
            .as_ref()
            .and_then(|padding| padding.parse())
            .expect("the padding must be there");
        assert!(close(padding.x, 32.0));
        assert!(close(padding.y, 32.0));

        // The size is the plain number.
        let size = object
            .size
            .as_ref()
            .and_then(|size| size.parse())
            .expect("the size must be there");
        assert!(close(size.x, 2.0));
        assert!(close(size.y, 2.0));
    }

    /// A scene.json that cannot be parsed does not take the application
    /// down: the loader reports the error and the wallpaper is skipped.
    #[test]
    fn a_malformed_scene_does_not_panic() {
        let files: Vec<(&str, &[u8])> = vec![
            (
                "project.json",
                br#"{"title":"My Scene","type":"Scene","file":"scene.json"}"#,
            ),
            (
                "scene.json",
                br#"{ "objects": [ "this is not an object" ] }"#,
            ),
        ];

        let path = std::env::temp_dir().join(format!(
            "linux-wallpaperengine-malformed-{}.pkg",
            std::process::id()
        ));
        std::fs::write(&path, make_pkg(&files)).expect("the package must be written");

        // The loader reports the error…
        let error = match Scene::try_new(path.to_string_lossy().to_string(), false, true) {
            Ok(_) => panic!("the malformed scene must be reported"),
            Err(error) => error,
        };
        assert!(
            matches!(error, SceneError::Scene(_)),
            "unexpected error: {:?}",
            error
        );

        // …and loading the wallpaper does not panic: the wallpaper is
        // skipped and an empty scene is returned.
        let scene = Scene::new(path.to_string_lossy().to_string(), false, true);
        let _ = std::fs::remove_file(&path);

        assert!(scene.root.objects.is_empty());

        // The empty scene still has a projection the renderer can use.
        assert!(scene.root.general.orthogonalprojection.width > 0);
        assert!(scene.root.general.orthogonalprojection.height > 0);
    }
}

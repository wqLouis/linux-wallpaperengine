//! The Wallpaper Engine project manifest (`project.json`).
//!
//! Every Wallpaper Engine package carries a `project.json` at its root. It
//! describes *what kind* of wallpaper the package holds — a scene, a video, a
//! web page or an application — and a couple of descriptive fields:
//!
//! ```json
//! { "title": "My Video", "type": "Video", "file": "video.mp4" }
//! ```
//!
//! The loader needs it to decide how a package should be brought up: a scene
//! package is loaded through `scene.json`, a video package is a single
//! looping clip.
//!
//! The parser is deliberately lenient — Wallpaper Engine keeps adding fields
//! to that file and third-party tools ship slightly different flavours of it:
//! unknown fields are ignored, a missing `type` means a scene and an
//! unrecognised `type` is reported as [`WallpaperKind::Unknown`] instead of
//! failing the whole parse.

use serde::Deserialize;

/// The kind of wallpaper a package contains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WallpaperKind {
    /// A scene wallpaper: `scene.json` plus its materials, effects and models.
    Scene,
    /// A video wallpaper: a single looping clip.
    Video,
    /// A web wallpaper (HTML/JS).
    Web,
    /// An application wallpaper.
    Application,
    /// A kind this parser does not know about (the raw string is preserved).
    Unknown(String),
}

/// The parsed contents of a Wallpaper Engine `project.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Human readable title.
    pub title: String,
    /// What kind of wallpaper this is.
    pub kind: WallpaperKind,
    /// The main file of the wallpaper ("scene.json" for scenes, "video.mp4" for
    /// videos), when present.
    pub file: Option<String>,
    /// The preview image, when present.
    pub preview: Option<String>,
}

/// The raw shape of `project.json`.
///
/// Every field is optional: Wallpaper Engine writes only what is relevant for
/// the given wallpaper, and other tools may write even less. Fields we do not
/// know about are simply ignored by serde.
#[derive(Debug, Deserialize)]
struct RawProject {
    /// Human readable title of the wallpaper.
    #[serde(default)]
    title: String,
    /// The wallpaper kind, as written by Wallpaper Engine ("Scene", "Video", …).
    #[serde(default, rename = "type")]
    kind: Option<String>,
    /// The main file of the wallpaper.
    #[serde(default)]
    file: Option<String>,
    /// The preview image.
    #[serde(default)]
    preview: Option<String>,
}

/// Parse the JSON text of a `project.json`.
///
/// Returns `None` when the text is not valid JSON or does not describe a
/// project (i.e. when it is not a JSON *object*).
///
/// The parser is deliberately lenient: unknown fields are ignored, a missing
/// `type` means [`WallpaperKind::Scene`], and an unknown `type` string is
/// preserved in [`WallpaperKind::Unknown`] instead of failing.
pub fn parse_project(json: &str) -> Option<Project> {
    let value: serde_json::Value = match serde_json::from_str(json) {
        Ok(value) => value,
        Err(err) => {
            log::debug!("failed to parse project.json: {}", err);
            return None;
        }
    };

    // Only a JSON *object* describes a project: an array or a scalar is not
    // one. (Serde would happily read `[]` as a struct with all-default
    // fields.)
    if !value.is_object() {
        log::debug!("project.json is not a JSON object");
        return None;
    }

    let raw: RawProject = match serde_json::from_value(value) {
        Ok(raw) => raw,
        Err(err) => {
            log::debug!("failed to read project.json: {}", err);
            return None;
        }
    };

    Some(Project {
        title: raw.title,
        kind: parse_kind(raw.kind.as_deref()),
        file: raw.file,
        preview: raw.preview,
    })
}

/// Map the raw `type` string onto a [`WallpaperKind`].
///
/// The comparison is case-insensitive ("video", "Video" and "VIDEO" are the
/// same kind). An absent or empty type means a scene, which is what Wallpaper
/// Engine assumes for the oldest packages.
fn parse_kind(raw: Option<&str>) -> WallpaperKind {
    let Some(raw) = raw else {
        return WallpaperKind::Scene;
    };

    if raw.is_empty() {
        return WallpaperKind::Scene;
    }

    match raw.to_ascii_lowercase().as_str() {
        "scene" => WallpaperKind::Scene,
        "video" => WallpaperKind::Video,
        "web" => WallpaperKind::Web,
        "application" => WallpaperKind::Application,
        _ => {
            log::debug!("unknown wallpaper type: {}", raw);
            WallpaperKind::Unknown(raw.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A realistic `project.json` as written by Wallpaper Engine.
    const VIDEO_PROJECT: &str = r#"{"title":"My Video","type":"Video","file":"video.mp4","preview":"preview.jpg","general":{"foo":1}}"#;

    #[test]
    fn parses_a_realistic_project() {
        let project = parse_project(VIDEO_PROJECT).expect("project.json should parse");

        assert_eq!(project.kind, WallpaperKind::Video);
        assert_eq!(project.title, "My Video");
        assert_eq!(project.file.as_deref(), Some("video.mp4"));
        assert_eq!(project.preview.as_deref(), Some("preview.jpg"));
    }

    #[test]
    fn maps_the_type_field() {
        let kind_of = |kind: &str| {
            let json = format!(r#"{{"title":"x","type":"{}"}}"#, kind);
            parse_project(&json)
                .expect("project.json should parse")
                .kind
        };

        assert_eq!(kind_of("Scene"), WallpaperKind::Scene);
        assert_eq!(kind_of("Video"), WallpaperKind::Video);
        assert_eq!(kind_of("Web"), WallpaperKind::Web);
        assert_eq!(kind_of("Application"), WallpaperKind::Application);
    }

    #[test]
    fn matches_the_type_field_case_insensitively() {
        let kind_of = |kind: &str| {
            let json = format!(r#"{{"title":"x","type":"{}"}}"#, kind);
            parse_project(&json)
                .expect("project.json should parse")
                .kind
        };

        assert_eq!(kind_of("web"), WallpaperKind::Web);
        assert_eq!(kind_of("APPLICATION"), WallpaperKind::Application);
        assert_eq!(kind_of("video"), WallpaperKind::Video);
        assert_eq!(kind_of("scene"), WallpaperKind::Scene);
    }

    #[test]
    fn a_missing_type_is_a_scene() {
        let project =
            parse_project(r#"{"title":"No type here"}"#).expect("project.json should parse");
        assert_eq!(project.kind, WallpaperKind::Scene);
        assert_eq!(project.title, "No type here");
    }

    #[test]
    fn an_unknown_type_is_preserved() {
        let project = parse_project(r#"{"title":"x","type":"SomethingElse"}"#)
            .expect("project.json should parse");
        assert_eq!(
            project.kind,
            WallpaperKind::Unknown("SomethingElse".to_string())
        );
    }

    #[test]
    fn malformed_json_is_rejected() {
        assert!(parse_project("{not json").is_none());
        assert!(parse_project("").is_none());
    }

    #[test]
    fn a_json_array_is_not_a_project() {
        assert!(parse_project("[]").is_none());
        assert!(parse_project(r#"[{"title":"x"}]"#).is_none());
    }

    #[test]
    fn a_json_scalar_is_not_a_project() {
        assert!(parse_project("42").is_none());
        assert!(parse_project(r#""scene""#).is_none());
    }

    #[test]
    fn missing_file_and_preview_are_none() {
        let project = parse_project(r#"{"title":"x"}"#).expect("project.json should parse");
        assert_eq!(project.file, None);
        assert_eq!(project.preview, None);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let project = parse_project(
            r#"{"title":"x","type":"Scene","somethingNew":{"a":[1,2,3]},"tags":["a"]}"#,
        )
        .expect("project.json should parse");
        assert_eq!(project.kind, WallpaperKind::Scene);
        assert_eq!(project.title, "x");
    }
}

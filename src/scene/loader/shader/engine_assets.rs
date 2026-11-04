//! The regression test against the real shaders of Wallpaper Engine.
//!
//! Wallpaper Engine ships its shaders as a directory of `.frag`/`.vert`
//! pairs.  They are written in the dialect of the engine and they are the
//! only real world input the shader pipeline has, so they are the input of
//! this test: every pair is preprocessed the way
//! `pipeline_handler::compile_pipeline` preprocesses it and the result is
//! compiled with naga, the shader compiler wgpu uses.
//!
//! The test needs the assets of Wallpaper Engine.  When they are not
//! installed, the test is skipped: it never fails on a machine that does not
//! have them.
//!
//! Run it with `cargo test -- --show-output` to see the report of the pairs
//! that do not compile.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::collections::{Map, sorted_entries};

use super::{ShaderProgram, get_headers, preprocess_pair_with_layout};

/// The pairs of the shaders of Wallpaper Engine that did not compile before
/// the dialect stage, with the class of the error they failed with.
///
/// The class is what the test asserts: a pair that used to fail with
/// `UnknownType(sampler2DComparison)` must not fail with that class any
/// more, whatever it fails with instead.  The classes are the ones the
/// dialect stage fixed:
///
/// * `UnknownType(sampler2DComparison)` and `UnknownType(sampler3D)`: the
///   sampler types of the engine,
/// * `UnknownType(sampler2DBackBuffer)`: the sampler of the back buffer
///   of a volumetric effect,
/// * `UnknownVariable(BONECOUNT)` and `UnknownVariable(BLENDROWCOUNT)`: the
///   macros the engine provides,
/// * `UnknownVariable(gl_InstanceID)`: the index of the instance of the
///   engine,
/// * `UnknownFunction(pow)`: the scalar argument of a vector parameter,
/// * `UnknownFunction(ApplyPosition)`: the headers of the models,
/// * `UnknownFunction(mul)`: the row vector form of the product,
/// * `UnknownFunction(PerformLighting_V1)`: the built-in functions of the
///   engine,
/// * `InvalidToken(PrecisionQualifier(Medium))`: the precision qualifiers.
const FIXED_CLASSES: &[(&str, &str)] = &[
    ("ccsimple", "UnknownType(sampler3D)"),
    ("chroma4", "UnknownType(sampler2DComparison)"),
    ("chroma4", "UnknownFunction(PerformLighting_V1)"),
    ("clippingmaskimage4", "UnknownVariable(BONECOUNT)"),
    ("combine_hdr_editor", "UnknownFunction(pow)"),
    ("downsample_quarter_linear", "UnknownFunction(pow)"),
    ("fade", "InvalidToken(PrecisionQualifier(Medium))"),
    ("flat", "InvalidToken(PrecisionQualifier(Medium))"),
    ("flatpoint", "InvalidToken(PrecisionQualifier(Medium))"),
    ("foliage4", "UnknownType(sampler2DComparison)"),
    ("foliage4", "UnknownFunction(PerformLighting_V1)"),
    ("fur4", "UnknownType(sampler2DComparison)"),
    ("fur4", "UnknownVariable(gl_InstanceID)"),
    ("fur4", "UnknownFunction(PerformLighting_V1)"),
    ("generic3", "UnknownVariable(BONECOUNT)"),
    ("generic4", "UnknownVariable(BONECOUNT)"),
    ("generic4", "UnknownFunction(PerformLighting_V1)"),
    ("genericimage2", "UnknownVariable(BONECOUNT)"),
    ("genericimage3", "UnknownVariable(BONECOUNT)"),
    ("genericimage4", "UnknownVariable(BONECOUNT)"),
    ("genericparticle", "UnknownType(sampler2DComparison)"),
    ("genericropeparticle", "UnknownType(sampler2DComparison)"),
    ("passthroughblend", "UnknownVariable(BONECOUNT)"),
    ("passthroughlinear", "UnknownFunction(pow)"),
    ("puppettexturechannels", "UnknownVariable(BLENDROWCOUNT)"),
    ("shadowcaster", "UnknownVariable(BONECOUNT)"),
    ("shadowcaster", "UnknownVariable(gl_InstanceID)"),
    ("shadowcasterfoliage4", "UnknownFunction(ApplyPosition)"),
    ("shadowcasterfoliage4", "UnknownVariable(gl_InstanceID)"),
    ("shadowcasterfur4", "UnknownFunction(ApplyPosition)"),
    ("shadowcasterfur4", "UnknownVariable(gl_InstanceID)"),
    ("volumetricsback", "UnknownFunction(mul)"),
    ("volumetricsfront", "UnknownType(sampler2DComparison)"),
    ("volumetricsfront", "UnknownType(sampler2DBackBuffer)"),
];

/// The pairs of the shaders of Wallpaper Engine that do not compile, with
/// the reason why.
///
/// The list is empty: every pair of the assets of Wallpaper Engine
/// compiles.  A pair that does not compile any more is a regression, and
/// the test says so.
///
/// The pairs that did not compile before the dialect stage grew, and the
/// rule that fixed them:
///
/// * `chroma4`, `foliage4`, `generic4` and `fur4` used
///   `PerformLighting_V1`, the lighting function of the engine.  It is in
///   none of the headers of the assets - it is in the engine itself - so
///   the pipeline provides an approximation of it (`ENGINE_BUILTINS`).
/// * `editorpaintbrush` initialised a vector with a scalar.  The
///   promotion of a scalar to a vector covers the initialisers of the
///   variables now.
/// * `fur4`, `shadowcaster`, `shadowcasterfoliage4` and `shadowcasterfur4`
///   used `gl_InstanceID`, the index of the instance of the engine.  It is
///   `gl_InstanceIndex` in GLSL, and the declaration of the engine is
///   dropped: it is a built-in of GLSL and not an attribute.
/// * `volumetricsfront` used the sampler of the back buffer of a
///   volumetric effect (`sampler2DBackBuffer`), the helpers of the engine
///   `texSample2DBackBuffer` and `texLoad2D` and the `clip` of GLSL, which
///   naga does not know.
const KNOWN_FAILURES: &[(&str, &str)] = &[];

/// The failure of one stage of a shader.
#[derive(Debug)]
struct Failure {
    /// The stage that failed.
    stage: &'static str,
    /// The class of the error: the kind of the error and the name it names.
    ///
    /// The class is stable: it does not depend on the handles and the
    /// spans of the error, so it can be compared with the class a shader
    /// failed with before.
    class: String,
    /// The error itself.
    message: String,
}

/// The result of the measurement of the shaders of Wallpaper Engine.
struct Measurement {
    /// The pairs that compile.
    compiled: Vec<String>,
    /// The pairs that do not compile, with the failures of their stages.
    failed: Vec<(String, Vec<Failure>)>,
    /// The pairs that were not measured, with the reason why.
    skipped: Vec<(String, String)>,
}

/// The directories the shaders of Wallpaper Engine are looked for in.
///
/// The first one that exists is used.  The environment variable
/// `WALLPAPER_ENGINE_ASSETS` points at the assets directory of Wallpaper
/// Engine - the one the `--assets-path` option of the program takes - and
/// the shaders are in its `shaders` subdirectory.
fn candidate_directories() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(assets) = std::env::var("WALLPAPER_ENGINE_ASSETS") {
        candidates.push(PathBuf::from(assets).join("shaders"));
    }

    // The usual places of a Steam installation.
    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        for prefix in [
            ".steam/steam",
            ".steam/root",
            ".local/share/Steam",
            ".var/app/com.valvesoftware.Steam/.local/share/Steam",
        ] {
            candidates.push(
                home.join(prefix)
                    .join("steamapps/common/wallpaper_engine/assets/shaders"),
            );
        }
    }

    // The installation of the machine the test was written on.
    candidates.push(PathBuf::from(
        "/mnt/DATA/Apps/Steam/steamapps/common/wallpaper_engine/assets/shaders",
    ));

    candidates
}

/// The directory the shaders of Wallpaper Engine are in, if they are
/// installed.
fn assets_directory() -> Option<PathBuf> {
    candidate_directories()
        .into_iter()
        .find(|directory| directory.is_dir())
}

/// Preprocess and compile every pair of the directory, the way the pipeline
/// of an effect does.
fn measure(directory: &Path) -> Measurement {
    // The headers of the engine, exactly as the loader of the scene loads
    // them: the key of a header is its path in the assets directory.
    let headers: Rc<Map<String, String>> = Rc::new(get_headers(|key| {
        let path = key.strip_prefix("shaders/")?;
        std::fs::read(directory.join(path)).ok()
    }));

    let mut names: Vec<String> = std::fs::read_dir(directory)
        .expect("the directory of the shaders can be read")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.ends_with(".frag"))
        .collect();
    names.sort();

    let mut measurement = Measurement {
        compiled: Vec::new(),
        failed: Vec::new(),
        skipped: Vec::new(),
    };

    for name in names {
        let base = name.trim_end_matches(".frag").to_string();
        let fragment = std::fs::read_to_string(directory.join(&name))
            .expect("the fragment shader of a pair can be read");
        let vertex_path = directory.join(format!("{base}.vert"));
        let Ok(vertex) = std::fs::read_to_string(&vertex_path) else {
            // A pair without a vertex stage has no `main`: the pipeline
            // does not substitute a full screen vertex shader, so there is
            // nothing to measure.
            measurement.skipped.push((
                base,
                format!("there is no vertex stage ({})", vertex_path.display()),
            ));
            continue;
        };

        // The program of the pair, exactly as the loader of the scene builds
        // it.
        let program = ShaderProgram::parse(vertex, fragment, Rc::clone(&headers));

        // The pipeline of `pipeline_handler::compile_pipeline` without a
        // material: the only macros that are known are the default values of
        // the combos of the shader.
        let defines = program.default_defines.clone();
        let mut merged = program.source_defines.clone();
        for (name, value) in &defines {
            merged.insert(name.clone(), value.clone());
        }
        let define_refs: Vec<(&str, &str)> = sorted_entries(&defines)
            .into_iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();

        let (vertex, fragment, _layout) = preprocess_pair_with_layout(
            &program.vertex,
            &program.fragment,
            &headers,
            &merged,
            &program.layout,
            false,
            false,
        );

        let mut failures = Vec::new();
        for (stage, source) in [
            (naga::ShaderStage::Vertex, &vertex),
            (naga::ShaderStage::Fragment, &fragment),
        ] {
            failures.extend(compile(source, stage, &define_refs));
        }

        if failures.is_empty() {
            measurement.compiled.push(base);
        } else {
            measurement.failed.push((base, failures));
        }
    }

    measurement
}

/// Compile a GLSL source with naga and validate the result.
///
/// The errors are returned instead of panicking: the test reports the pairs
/// that do not compile.
fn compile(source: &str, stage: naga::ShaderStage, defines: &[(&str, &str)]) -> Vec<Failure> {
    let name = stage_name(stage);
    let options = naga::front::glsl::Options {
        stage,
        defines: defines
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect(),
    };

    let module = match naga::front::glsl::Frontend::default().parse(&options, source) {
        Ok(module) => module,
        Err(errors) => {
            return errors
                .errors
                .iter()
                .map(|error| Failure {
                    stage: name,
                    class: parse_error_class(error),
                    message: format!("{:?}", error.kind),
                })
                .collect();
        }
    };

    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    match validator.validate(&module) {
        Ok(_) => Vec::new(),
        Err(error) => vec![Failure {
            stage: name,
            class: validation_error_class(&error),
            message: format!("{:?}", error.as_inner()),
        }],
    }
}

/// The class of an error of the GLSL front end of naga.
///
/// The class is the kind of the error and the name it names, without the
/// handles and the spans: it is the same for every shader that fails the
/// same way.
fn parse_error_class(error: &naga::front::glsl::Error) -> String {
    use naga::front::glsl::ErrorKind;

    match &error.kind {
        ErrorKind::SemanticError(message) => match message
            .strip_prefix("Unknown function '")
            .and_then(|rest| rest.strip_suffix('\''))
        {
            Some(function) => format!("UnknownFunction({function})"),
            None => format!("SemanticError({message})"),
        },
        ErrorKind::UnknownType(name) => format!("UnknownType({name})"),
        ErrorKind::UnknownVariable(name) => format!("UnknownVariable({name})"),
        ErrorKind::InvalidToken(token, _) => format!("InvalidToken({token:?})"),
        ErrorKind::PreprocessorError(error) => format!("PreprocessorError({error:?})"),
        other => format!("{other:?}"),
    }
}

/// The class of an error of the validator of naga.
fn validation_error_class(error: &naga::WithSpan<naga::valid::ValidationError>) -> String {
    use naga::valid::ValidationError;

    match error.as_inner() {
        ValidationError::EntryPoint { source, .. } => format!("EntryPoint({source:?})"),
        ValidationError::Function { source, .. } => format!("Function({source:?})"),
        other => format!("{other:?}"),
    }
}

/// The name of a shader stage, for the error messages.
fn stage_name(stage: naga::ShaderStage) -> &'static str {
    match stage {
        naga::ShaderStage::Vertex => "vertex",
        naga::ShaderStage::Fragment => "fragment",
        naga::ShaderStage::Compute => "compute",
        _ => "unknown",
    }
}

/// Print the report of the measurement.
fn report(directory: &Path, measurement: &Measurement) {
    eprintln!("the shaders of Wallpaper Engine: {}", directory.display());
    eprintln!(
        "{} pairs, {} compile, {} do not compile",
        measurement.compiled.len() + measurement.failed.len(),
        measurement.compiled.len(),
        measurement.failed.len()
    );

    for (name, reason) in &measurement.skipped {
        eprintln!("  skipped {name}: {reason}");
    }

    for (name, failures) in &measurement.failed {
        let reason = KNOWN_FAILURES
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, reason)| *reason)
            .unwrap_or("unknown");
        eprintln!("  {name} does not compile: {reason}");
        for failure in failures {
            eprintln!(
                "      {}: {} ({})",
                failure.stage, failure.message, failure.class
            );
        }
    }
}

/// The measurement of the shaders of Wallpaper Engine, or `None` when they
/// are not installed.
fn measurement() -> Option<(PathBuf, Measurement)> {
    let directory = assets_directory()?;
    let measurement = measure(&directory);
    Some((directory, measurement))
}

/// Every pair of the shaders of Wallpaper Engine compiles.
///
/// The pairs that are known not to compile are reported, not asserted: they
/// are the limits of the pipeline and they may be fixed.  Every other pair
/// has to compile: a pair that does not compile any more is a regression.
#[test]
fn the_shaders_of_wallpaper_engine_compile() {
    let Some((directory, measurement)) = measurement() else {
        eprintln!("the shaders of Wallpaper Engine are not installed: the test is skipped");
        return;
    };

    report(&directory, &measurement);

    for (name, failures) in &measurement.failed {
        assert!(
            KNOWN_FAILURES.iter().any(|(known, _)| known == name),
            "the shader {name} does not compile any more:\n{}",
            failures
                .iter()
                .map(|failure| format!("      {}: {}", failure.stage, failure.message))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

/// The classes of the errors the dialect stage fixed do not come back.
///
/// A pair that used to fail with `UnknownType(sampler2DComparison)` must
/// not fail with that class any more: it either compiles or it fails with
/// another class, which is a limit of the pipeline and not a regression.
#[test]
fn the_classes_the_dialect_stage_fixed_do_not_come_back() {
    let Some((directory, measurement)) = measurement() else {
        eprintln!("the shaders of Wallpaper Engine are not installed: the test is skipped");
        return;
    };

    report(&directory, &measurement);

    for (name, class) in FIXED_CLASSES {
        let Some((_, failures)) = measurement.failed.iter().find(|(failed, _)| failed == name)
        else {
            // The pair compiles, or Wallpaper Engine does not ship it any
            // more.
            continue;
        };

        assert!(
            !failures.iter().any(|failure| failure.class == *class),
            "the shader {name} fails with {class} again:\n{}",
            failures
                .iter()
                .map(|failure| format!("      {}: {}", failure.stage, failure.message))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

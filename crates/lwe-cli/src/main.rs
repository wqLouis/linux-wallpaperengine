use std::path::{Path, PathBuf};
use std::sync::mpsc;

use clap::Parser;
use log::LevelFilter;

use linux_wallpaper_engine::engine::EngineStatus;
use linux_wallpaper_engine::scene::adapters::FitMode;
#[cfg(target_os = "macos")]
use linux_wallpaper_engine::scene::adapters::mac_app;
use linux_wallpaper_engine::scene::adapters::{
    AdapterConfig, gnome_app, wallpaper_source, winit_adapter, wlr_app,
};
use linux_wallpaper_engine::scene::error::EngineError;

// ── Root CLI ─────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "linux-wallpaper-engine", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    // wallpaper .pkg file path
    #[arg(short, default_value = "./scene.pkg", global = false)]
    path: String,

    // different display mode [wlr, winit, gnome, mac]
    #[arg(short = 'm', default_value = "wlr")]
    modes: String,

    // How to fit wallpaper to output: cover, contain, or stretch
    #[arg(long, default_value = "cover")]
    fit_mode: String,

    // Bypass all post-process effects, render as static image
    #[arg(long, default_value_t = false)]
    no_effects: bool,

    // Ignore MDL puppet models, render as flat quads
    #[arg(long, default_value_t = false)]
    no_mdl: bool,

    // Log level: verbose (everything + wgpu/naga), debug, warning, errors
    #[arg(short = 'l', long, default_value = "warning")]
    log_level: String,

    /// Path to Wallpaper Engine assets directory for lazy-loading
    /// fallback (e.g. Steam/steamapps/common/wallpaper_engine/assets).
    #[arg(long)]
    assets_path: Option<String>,

    /// Target frames per second. If unset, renders as fast as possible.
    #[arg(long)]
    target_fps: Option<u32>,
}

// ── Subcommands ──────────────────────────────────────────────────────────────

#[derive(Parser)]
enum Commands {
    /// Parse and extract files from a .pkg archive.
    Parser(ParserArgs),
}

#[derive(Parser)]
struct ParserArgs {
    /// Path to the .pkg wallpaper file.
    #[arg(short)]
    path: String,

    /// Extract the .pkg archive to a directory.
    #[arg(short = 'x')]
    extract: Option<String>,

    /// Print the contents of a file from the .pkg to stdout.
    /// Specify the internal path of the file (must be valid UTF-8).
    /// Example: -c scene.json
    #[arg(short = 'c', long)]
    cat: Option<String>,

    /// List files inside the .pkg archive.
    /// Optionally filter by a prefix path (e.g. --list scenes/).
    /// If no value is given, lists everything from the archive root.
    #[arg(short = 'l', long, num_args = 0..=1, default_missing_value = "")]
    list: Option<String>,

    /// Parse .tex textures to PNG images during extraction.
    #[arg(long)]
    parse_tex: bool,

    /// Parse video/GIF metadata during extraction.
    #[arg(long)]
    parse_video: bool,

    /// Parse .mdl puppet model files to JSON during extraction.
    #[arg(long)]
    parse_mdl: bool,

    /// Dry run — show what would be extracted without writing files.
    #[arg(long)]
    dry_run: bool,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn setup_logging(level_str: &str) {
    let (level, verbose) = match level_str {
        "verbose" => (LevelFilter::Trace, true),
        "debug" => (LevelFilter::Debug, false),
        "warning" => (LevelFilter::Warn, false),
        "errors" => (LevelFilter::Error, false),
        _ => {
            eprintln!(
                "Unknown log-level '{}'. Valid: verbose, debug, warning, errors",
                level_str
            );
            std::process::exit(1);
        }
    };

    let mut builder = env_logger::Builder::new();
    builder.filter_level(level);
    if !verbose {
        builder.filter(Some("wgpu"), LevelFilter::Warn);
        builder.filter(Some("naga"), LevelFilter::Warn);
        builder.filter(Some("wgpu_core"), LevelFilter::Warn);
        builder.filter(Some("wgpu_hal"), LevelFilter::Warn);
    }
    builder.format_timestamp_millis().init();
}

/// Checks that the path is a wallpaper: an archive or a folder.
fn validate_pkg_path(path: &str) -> Result<(), String> {
    let p = Path::new(path);

    // A folder with the files of a wallpaper: the layout the Steam workshop
    // holds (a project.json next to the materials, the models, ...).
    if p.is_dir() {
        return Ok(());
    }

    if !p.exists() || p.extension().unwrap_or_default() != "pkg" {
        return Err(format!(
            "Path '{}' does not exist, is not a .pkg file and is not a wallpaper folder",
            path
        ));
    }
    Ok(())
}

/// Checks that the path is an archive: the parser only reads archives.
fn validate_archive_path(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if !p.is_file() || p.extension().unwrap_or_default() != "pkg" {
        return Err(format!("Path '{}' is not a .pkg file", path));
    }
    Ok(())
}

fn print_error_list(paths: &[String], msg: &str) {
    eprintln!("{}", msg);
    for p in paths {
        eprintln!("  {}", p);
    }
}

// ── Parser subcommand logic ──────────────────────────────────────────────────

fn run_parser(args: ParserArgs) {
    if let Err(e) = validate_archive_path(&args.path) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }

    let pkg = match linux_wallpaper_engine::pkg_parser::pkg_parser::parser::Pkg::new(Path::new(
        &args.path,
    )) {
        Ok(pkg) => pkg,
        Err(e) => {
            eprintln!("error: failed to parse pkg: {}", e);
            std::process::exit(1);
        }
    };

    // --list / -l : list files, optionally filtered by prefix
    if let Some(prefix) = &args.list {
        let mut paths: Vec<&String> = pkg.files.keys().collect();
        paths.sort();
        for p in paths {
            if prefix.is_empty() || p.starts_with(prefix) {
                println!("{}", p);
            }
        }
        return;
    }

    // --cat / -c : print a single file to stdout
    if let Some(requested) = &args.cat {
        match pkg.files.get(requested) {
            Some(bytes) => {
                use std::io::Write;
                let stdout = std::io::stdout();
                let mut handle = stdout.lock();
                handle.write_all(bytes).expect("failed to write to stdout");
                handle.flush().expect("failed to flush stdout");
            }
            None => {
                let mut paths: Vec<&String> = pkg.files.keys().collect();
                paths.sort();
                let paths_str: Vec<String> = paths.iter().map(|s| (*s).clone()).collect();
                print_error_list(
                    &paths_str,
                    &format!(
                        "No file found at internal path '{}'. Available files:",
                        requested
                    ),
                );
                std::process::exit(1);
            }
        }
        return;
    }

    // --extract / -x : save files to disk
    if let Some(output) = &args.extract {
        let target = Path::new(output);
        log::info!(
            "Extracting {} files to {} (parse_tex={}, parse_video={}, parse_mdl={}, dry_run={})",
            pkg.files.len(),
            target.display(),
            args.parse_tex,
            args.parse_video,
            args.parse_mdl,
            args.dry_run,
        );
        pkg.save_pkg(
            target,
            args.dry_run,
            args.parse_tex,
            args.parse_video,
            args.parse_mdl,
        );
        return;
    }

    // No operation specified — show help.
    eprintln!("parser: no operation specified. Use --help for usage.");
    std::process::exit(1);
}

// ── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    let mut cli = Cli::parse();

    // If a subcommand was given, dispatch to it.
    if let Some(cmd) = cli.command {
        match cmd {
            Commands::Parser(args) => {
                run_parser(args);
                return;
            }
        }
    }

    // No subcommand → run the wallpaper engine.
    setup_logging(&cli.log_level);

    if let Err(e) = validate_pkg_path(&cli.path) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }

    // The extraction progress bar overwrites stdout as it redraws, so
    // it would eat any debug/trace logs printed during scene load. Hide
    // it whenever those levels are active.
    let show_progress = !matches!(cli.log_level.as_str(), "verbose" | "debug");

    let fit_mode = match cli.fit_mode.as_str() {
        "cover" => FitMode::Cover,
        "contain" | "fit" => FitMode::Contain,
        "stretch" => FitMode::Stretch,
        _ => {
            eprintln!(
                "Unknown fit-mode '{}'. Valid: cover, contain, stretch",
                cli.fit_mode
            );
            std::process::exit(1);
        }
    };

    // --no-effects implies a static image: render once and stop,
    // unless the user explicitly set a different target_fps.
    if cli.no_effects && cli.target_fps.is_none() {
        cli.target_fps = Some(0);
    }

    let target_fps = cli.target_fps;

    // The engine reports the files of the wallpaper that could not be
    // used: they are printed, one per line, while the wallpaper keeps
    // running.  The adapter runs in the foreground, so the reports are
    // printed by a thread of their own.
    let (status_tx, status_rx) = mpsc::channel();
    let reporter = std::thread::spawn(move || {
        for status in status_rx {
            match status {
                EngineStatus::Running { problems, .. } => {
                    for problem in problems {
                        eprintln!("warning: {problem}");
                    }
                }
                EngineStatus::Failed(_) => {
                    // The failure is reported by the caller below, which
                    // has the error itself: printing it here would show it
                    // twice.
                }
                _ => {}
            }
        }
    });

    let config = AdapterConfig {
        // A folder works exactly like an archive: the shape of the source
        // is decided by the path itself.
        source: wallpaper_source(PathBuf::from(&cli.path)),
        fit_mode,
        no_effects: cli.no_effects,
        no_mdl: cli.no_mdl,
        assets_path: cli.assets_path,
        target_fps,
        show_progress,
        status: Some(status_tx.clone()),
    };

    let result = run_adapter(&cli.modes, config);

    // The adapter is done: the status channel is closed and the thread
    // that prints the problems ends.
    drop(status_tx);
    let _ = reporter.join();

    // A wallpaper that cannot be shown is fatal: the error is reported
    // with its whole chain of causes and the command line dies with it.
    if let Err(error) = result {
        eprintln!("error: {}", error.trace());
        std::process::exit(1);
    }
}

/// Runs the adapter of the display mode.
fn run_adapter(mode: &str, config: AdapterConfig) -> Result<(), EngineError> {
    match mode {
        "winit" => winit_adapter::start(config),
        "wlr" => wlr_app::start(config),
        "gnome" => gnome_app::start(config),
        "mac" => {
            #[cfg(target_os = "macos")]
            {
                mac_app::start(config)
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = config;
                eprintln!("The 'mac' display mode is only available on macOS");
                Ok(())
            }
        }
        _ => {
            eprintln!(
                "Unknown display mode '{}'. Valid: wlr, winit, gnome, mac",
                mode
            );
            std::process::exit(1);
        }
    }
}

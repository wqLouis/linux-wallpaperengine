mod scene;

use std::path::Path;

use clap::Parser;
use log::LevelFilter;

use crate::scene::adapters::FitMode;
use crate::scene::adapters::{winit_adapter, wlr_app};

// ── Root CLI ─────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    // wallpaper .pkg file path
    #[arg(short, default_value = "./scene.pkg", global = false)]
    path: String,

    // different display mode [wlr, winit]
    #[arg(short = 'm', default_value = "wlr")]
    modes: String,

    // How to fit wallpaper to output: cover, contain, or stretch
    #[arg(long, default_value = "cover")]
    fit_mode: String,

    // Bypass all post-process effects, render as static image
    #[arg(long, default_value_t = false)]
    no_effects: bool,

    // Log level: verbose (everything + wgpu/naga), debug, warning, errors
    #[arg(short = 'l', long, default_value = "warning")]
    log_level: String,

    /// Path to Wallpaper Engine assets directory for lazy-loading
    /// fallback (e.g. Steam/steamapps/common/wallpaper_engine/assets).
    #[arg(long)]
    assets_path: Option<String>,
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

// ── Constants ────────────────────────────────────────────────────────────────

pub const MAX_TEXTURE: u32 = 512;
pub const MAX_VERTEX: u32 = MAX_TEXTURE * 4;
pub const MAX_INDEX: u32 = MAX_TEXTURE * 6;

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

fn validate_pkg_path(path: &str) {
    let p = Path::new(path);
    if !p.exists() || p.extension().unwrap_or_default() != "pkg" {
        panic!("Path '{}' does not exist or is not a .pkg file", path);
    }
}

fn print_error_list(paths: &[String], msg: &str) {
    eprintln!("{}", msg);
    for p in paths {
        eprintln!("  {}", p);
    }
}

// ── Parser subcommand logic ──────────────────────────────────────────────────

fn run_parser(args: ParserArgs) {
    validate_pkg_path(&args.path);

    let pkg = pkg_parser::pkg_parser::parser::Pkg::new(Path::new(&args.path));

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
        pkg.save_pkg(target, args.dry_run, args.parse_tex, args.parse_video, args.parse_mdl);
        return;
    }

    // No operation specified — show help.
    eprintln!("parser: no operation specified. Use --help for usage.");
    std::process::exit(1);
}

// ── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();

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

    validate_pkg_path(&cli.path);

    let fit_mode = match cli.fit_mode.as_str() {
        "cover" => FitMode::Cover,
        "contain" | "fit" => FitMode::Contain,
        "stretch" => FitMode::Stretch,
        _ => {
            eprintln!(
                "Unknown fit-mode '{}'. Valid: cover, contain, stretch",
                cli.fit_mode
            );
            return;
        }
    };

    match cli.modes.as_str() {
        "winit" => winit_adapter::start(cli.path, cli.no_effects, cli.assets_path),
        "wlr" => wlr_app::start(cli.path, fit_mode, cli.no_effects, cli.assets_path),
        _ => {
            eprintln!("Unknown display mode '{}'. Valid: wlr, winit", cli.modes);
        }
    }
}

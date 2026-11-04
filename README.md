# Linux Wallpaper Engine

[English](README.md) | [中文繁體](README_zh_TW.md) | [中文简体](README_zh_CN.md)

---

[![Rust](https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Wgpu](https://img.shields.io/badge/Wgpu-FF5A03?style=for-the-badge&logo=wgpu&logoColor=white)](https://github.com/gfx-rs/wgpu)
[![Vulkan](https://img.shields.io/badge/Vulkan-AC162C?style=for-the-badge&logo=vulkan&logoColor=white)](https://www.vulkan.org/)

Wallpaper Engine for Linux — written in Rust, rendered with `wgpu`.

https://github.com/user-attachments/assets/16891f80-30ca-482c-9f25-17b0b8fdeca5

---

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos/wqLouis/linux-wallpaper-engine&type=Timeline)](https://star-history.com/#wqLouis/linux-wallpaper-engine&Timeline)

---

## Features

### Supported

- **wlr-layer-shell (Wayland)** — the wallpaper is rendered as a `Layer::Background` surface behind every window, with fractional scaling support.
- **GNOME** — GNOME Shell has no layer shell, so the frames are rendered off-screen and streamed to a companion [GNOME Shell extension](gnome-extension/README.md) that shows them as the desktop background.
- **Graphical interface** — a native tray application (built with [gpui-ce](https://crates.io/crates/gpui-ce)) that lists the wallpapers of a workshop folder and renders the one that is picked.
- **Effect shaders** — the GLSL post-processing shaders of Wallpaper Engine (bloom, water ripples, …) are translated to WGSL and compiled at runtime.
- **Puppet rendering** — `.mdl` models are parsed and rendered as animated, CPU-skinned meshes.
- **Audio** — the audio tracks of a scene are played in a loop.

### Not supported

- **Video playback** — `.mp4` / `.webm` textures show a still frame.
- **User scripts** — `script.json` and the user script API are ignored.
- **Text rendering** — text layers are not drawn.
- **Particle systems** — particles are not simulated.

---

## Installation

### Build dependencies

**Linux**

The command line interface needs a C toolchain, `pkg-config` and the ALSA
headers (the engine plays the audio of a scene with `rodio`):

```bash
# Debian / Ubuntu
sudo apt install build-essential pkg-config libasound2-dev

# Fedora
sudo dnf install gcc pkgconf-pkg-config alsa-lib-devel

# Arch
sudo pacman -S base-devel pkgconf alsa-lib
```

The graphical interface needs two more packages: the gpui-ce backend of Linux
links against **fontconfig** (the fonts of the interface) and **xkbcommon**
(the keyboard layout):

```bash
# Debian / Ubuntu
sudo apt install libfontconfig-dev libxkbcommon-dev

# Fedora
sudo dnf install fontconfig-devel libxkbcommon-devel

# Arch
sudo pacman -S fontconfig libxkbcommon
```

A Vulkan driver is needed as well (Mesa for AMD/Intel, the proprietary driver for
Nvidia).

**macOS**

The macOS backend is written in Swift, so the Xcode command line tools (they
include `swiftc`) are required:

```bash
xcode-select --install
```

### Build

```bash
git clone https://github.com/wqLouis/linux-wallpaperengine.git
cd linux-wallpaper-engine
git submodule update --init --recursive   # the package parser is a submodule
cargo build --release
```

The repository is a cargo workspace with three packages:

| Package | Directory | What it builds |
|---------|-----------|----------------|
| `linux-wallpaper-engine` | `.` | the library (`linux_wallpaper_engine`) |
| `linux-wallpaper-engine-cli` | `crates/lwe-cli` | the CLI (`lwe-cli`) |
| `linux-wallpaper-engine-gui` | `crates/lwe-gui` | the GUI (`lwe-gui`) |

```bash
cargo build                # the library + the CLI (the default members)
cargo build-cli            # the CLI only
cargo build-gui            # the GUI only
cargo build --workspace    # everything
cargo test                 # the tests of the library and of the CLI
```

`cargo build-cli` and `cargo build-gui` are aliases of
`cargo build -p linux-wallpaper-engine-cli` and
`cargo build -p linux-wallpaper-engine-gui`; the aliases live in
`.cargo/config.toml`.

### Install

```bash
# the command line interface
cargo install --path crates/lwe-cli --profile release

# the graphical interface
cargo install --path crates/lwe-gui --profile release
```

or copy the binaries into your `PATH` by hand:

```bash
sudo install -Dm755 target/release/lwe-cli /usr/local/bin/
sudo install -Dm755 target/release/lwe-gui /usr/local/bin/
```

The GNOME backend additionally needs the shell extension:

```bash
./gnome-extension/install.sh
```

---

## The graphical interface

```bash
lwe-gui
```

The GUI is a system tray application: it puts a **StatusNotifierItem** on the
D-Bus (the tray icon of the desktop) and stays in the background.  Clicking
the icon opens a picker with the wallpapers of the workshop folder that was
chosen; the wallpaper that is picked is rendered as the desktop background by
the engine — in the same process, on a worker thread, so the interface stays
responsive.

On the first start the GUI asks for the workshop folder (the folder of the
Steam workshop that holds the wallpapers, by default
`~/.steam/steam/steamapps/workshop/content/431960`).

---

## Usage

```bash
# wlr-layer-shell (default) — behind every window
linux-wallpaper-engine -p wallpaper.pkg

# GNOME Shell — needs the extension from gnome-extension/
linux-wallpaper-engine -p wallpaper.pkg -m gnome

# a normal window (X11 / Wayland), with cursor tracking
linux-wallpaper-engine -p wallpaper.pkg -m winit

# macOS (experimental)
linux-wallpaper-engine -p wallpaper.pkg -m mac
```

### Options

| Option | Description | Default |
|--------|-------------|---------|
| `-p <path>` | path to the `.pkg` file | `./scene.pkg` |
| `-m <mode>` | display mode: `wlr`, `winit`, `gnome` or `mac` | `wlr` |
| `--fit-mode <mode>` | fit the wallpaper: `cover`, `contain` or `stretch` | `cover` |
| `--no-effects` | skip the post-processing effects | `false` |
| `--no-mdl` | ignore the puppet models | `false` |
| `--target-fps <fps>` | limit the frame rate | unlimited |
| `-l <level>` | log level: `verbose`, `debug`, `warning`, `errors` | `warning` |
| `--assets-path <dir>` | fallback directory with the Wallpaper Engine assets | — |

### Unpacking a wallpaper

```bash
# list the files of the package
linux-wallpaper-engine parser -p wallpaper.pkg --list

# unpack it
linux-wallpaper-engine parser -p wallpaper.pkg -x output/

# unpack and convert the textures to PNG / parse the puppets
linux-wallpaper-engine parser -p wallpaper.pkg -x output/ --parse-tex --parse-mdl
```

---

## Use it as a crate

The engine is a library: the CLI and the GUI are thin wrappers around it, and
so can be anything else.

```toml
[dependencies]
linux-wallpaper-engine = { git = "https://github.com/wqLouis/linux-wallpaperengine.git" }
```

```rust
use std::path::Path;

use linux_wallpaper_engine::engine::{EngineConfig, WallpaperHandle, scan_wallpapers};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Find the wallpapers: a workshop folder holds one folder per wallpaper.
    let workshop = Path::new("/home/me/.steam/steam/steamapps/workshop/content/431960");
    let wallpapers = scan_wallpapers(workshop)?;

    // 2. Start one of them: the renderer runs on its own worker thread.
    //    The handle is the control of the wallpaper: it takes the commands
    //    (pause, resume, stop, ...) and it reports the status of the
    //    renderer.
    let _handle = WallpaperHandle::spawn(EngineConfig::default(), wallpapers[0].source.clone())?;

    // 3. Every failure is an `EngineError` that carries its cause, so the
    //    whole trace of the failure can be reported.
    Ok(())
}
```

The façade of the library is described in
[docs/modules/engine.md](docs/modules/engine.md).

---

## Contributing

Feel free to post issues and pull requests.

---

## Disclaimer

This project is not affiliated with or endorsed by Wallpaper Engine.
Please support the original software by purchasing it on
[Steam](https://store.steampowered.com/app/431960/Wallpaper_Engine/).

## License

GPLv3 — see [LICENSE](LICENSE) for details.

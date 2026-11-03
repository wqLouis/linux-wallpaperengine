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

```bash
# Debian / Ubuntu
sudo apt install build-essential pkg-config libasound2-dev

# Fedora
sudo dnf install gcc pkgconf-pkg-config alsa-lib-devel

# Arch
sudo pacman -S base-devel pkgconf alsa-lib
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
git clone https://github.com/wqLouis/linux-wallpaper-engine.git
cd linux-wallpaper-engine
cargo build --release
```

### Install

```bash
cargo install --path . --profile release
```

or copy the binary into your `PATH` by hand:

```bash
sudo install -Dm755 target/release/linux-wallpaper-engine /usr/local/bin/
```

The GNOME backend additionally needs the shell extension:

```bash
./gnome-extension/install.sh
```

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

## Contributing

Feel free to post issues and pull requests.

---

## Disclaimer

This project is not affiliated with or endorsed by Wallpaper Engine.
Please support the original software by purchasing it on
[Steam](https://store.steampowered.com/app/431960/Wallpaper_Engine/).

## License

GPLv3 — see [LICENSE](LICENSE) for details.

# Linux Wallpaper Engine

[English](README.md) | [中文繁体](README_zh_TW.md) | [中文简体](README_zh_CN.md)

---

[![Rust](https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Wgpu](https://img.shields.io/badge/Wgpu-FF5A03?style=for-the-badge&logo=wgpu&logoColor=white)](https://github.com/gfx-rs/wgpu)
[![Vulkan](https://img.shields.io/badge/Vulkan-AC162C?style=for-the-badge&logo=vulkan&logoColor=white)](https://www.vulkan.org/)

Linux 上的 Wallpaper Engine —— 使用 Rust 编写，通过 `wgpu` 渲染。

https://github.com/user-attachments/assets/16891f80-30ca-482c-9f25-17b0b8fdeca5

---

## 功能

### 已支持

- **wlr-layer-shell（Wayland）** —— 壁纸以 `Layer::Background` 表面渲染，置于所有窗口之后，并支持分数缩放。
- **GNOME** —— GNOME Shell 没有 layer shell，因此改为离屏渲染，再通过配套的 [GNOME Shell 扩展](gnome-extension/README.md) 将帧显示为桌面背景。
- **图形界面** —— 原生托盘应用（基于 [gpui-ce](https://crates.io/crates/gpui-ce)），列出创意工坊文件夹中的壁纸并渲染所选的那一个。
- **特效着色器** —— Wallpaper Engine 的 GLSL 后处理着色器（泛光、水波纹等）会被转译为 WGSL，并在运行时编译。
- **偶戏渲染** —— `.mdl` 模型会被解析并渲染为带骨骼动画的 CPU 蒙皮网格。
- **音频** —— 场景中的音轨会循环播放。

### 暂不支持

- **视频播放** —— `.mp4` / `.webm` 纹理只显示首帧。
- **用户脚本** —— 忽略 `script.json` 与用户脚本 API。
- **文字渲染** —— 不绘制文字图层。
- **粒子系统** —— 不模拟粒子。

---

## 安装

### 编译依赖

**Linux**

命令行界面需要 C 工具链、`pkg-config` 以及 ALSA 头文件（引擎用 `rodio` 播放场景音频）：

```bash
# Debian / Ubuntu
sudo apt install build-essential pkg-config libasound2-dev

# Fedora
sudo dnf install gcc pkgconf-pkg-config alsa-lib-devel

# Arch
sudo pacman -S base-devel pkgconf alsa-lib
```

图形界面还需要两个包：Linux 版 gpui-ce 后端链接 **fontconfig**（界面字体）与 **xkbcommon**（键盘布局）：

```bash
# Debian / Ubuntu
sudo apt install libfontconfig-dev libxkbcommon-dev

# Fedora
sudo dnf install fontconfig-devel libxkbcommon-devel

# Arch
sudo pacman -S fontconfig libxkbcommon
```

另外还需要 Vulkan 驱动（AMD/Intel 用 Mesa，Nvidia 用官方驱动）。

**macOS**

macOS 适配器使用 Swift 编写，因此需要 Xcode 命令行工具（内含 `swiftc`）：

```bash
xcode-select --install
```

### 编译

```bash
git clone https://github.com/wqLouis/linux-wallpaperengine.git
cd linux-wallpaper-engine
git submodule update --init --recursive   # 包解析器是 git 子模块
cargo build --release
```

仓库是一个 cargo workspace，包含三个包：

| 包 | 目录 | 产物 |
|---|---|---|
| `linux-wallpaper-engine` | `.` | 库（`linux_wallpaper_engine`） |
| `linux-wallpaper-engine-cli` | `crates/lwe-cli` | 命令行界面（`linux-wallpaper-engine`） |
| `linux-wallpaper-engine-gui` | `crates/lwe-gui` | 图形界面（`linux-wallpaper-engine-gui`） |

```bash
cargo build                # 库 + 命令行界面（默认成员）
cargo build-cli            # 仅命令行界面
cargo build-gui            # 仅图形界面
cargo build --workspace    # 全部
cargo test                 # 库与命令行界面的测试
```

`cargo build-cli` 与 `cargo build-gui` 是 `.cargo/config.toml` 中的别名，分别等价于
`cargo build -p linux-wallpaper-engine-cli` 与 `cargo build -p linux-wallpaper-engine-gui`。

### 安装

```bash
# 命令行界面
cargo install --path . --profile release

# 图形界面
cargo install --path crates/lwe-gui --profile release
```

也可以手动复制可执行文件：

```bash
sudo install -Dm755 target/release/linux-wallpaper-engine /usr/local/bin/
sudo install -Dm755 target/release/linux-wallpaper-engine-gui /usr/local/bin/
```

GNOME 后端还需要安装 Shell 扩展：

```bash
./gnome-extension/install.sh
```

---

## 图形界面

```bash
linux-wallpaper-engine-gui
```

图形界面是一个系统托盘应用：它在 D-Bus 上注册一个 **StatusNotifierItem**（也就是桌面的托盘图标），并常驻后台。点击图标会打开一个选择器，列出所选创意工坊文件夹中的壁纸；选中的壁纸由引擎渲染为桌面背景 —— 在同一进程内、于工作线程上完成，因此界面始终保持响应。

首次启动时，图形界面会询问创意工坊文件夹的位置（默认是
`~/.steam/steam/steamapps/workshop/content/431960`）。

---

## 使用

```bash
# wlr-layer-shell（默认）—— 位于所有窗口之后
linux-wallpaper-engine -p wallpaper.pkg

# GNOME Shell —— 需要 gnome-extension/ 中的扩展
linux-wallpaper-engine -p wallpaper.pkg -m gnome

# 普通窗口（X11 / Wayland），可追踪光标
linux-wallpaper-engine -p wallpaper.pkg -m winit

# macOS（实验性）
linux-wallpaper-engine -p wallpaper.pkg -m mac
```

### 参数

| 参数 | 说明 | 默认值 |
|--------|-------------|---------|
| `-p <path>` | `.pkg` 文件路径 | `./scene.pkg` |
| `-m <mode>` | 显示模式：`wlr`、`winit`、`gnome` 或 `mac` | `wlr` |
| `--fit-mode <mode>` | 壁纸适配：`cover`、`contain` 或 `stretch` | `cover` |
| `--no-effects` | 跳过后处理特效 | `false` |
| `--no-mdl` | 忽略偶戏模型 | `false` |
| `--target-fps <fps>` | 限制帧率 | 不限 |
| `-l <level>` | 日志级别：`verbose`、`debug`、`warning`、`errors` | `warning` |
| `--assets-path <dir>` | Wallpaper Engine 素材目录（回退用） | — |

### 解包壁纸

```bash
# 列出包内文件
linux-wallpaper-engine parser -p wallpaper.pkg --list

# 解包
linux-wallpaper-engine parser -p wallpaper.pkg -x output/

# 解包并将纹理转为 PNG / 解析偶戏模型
linux-wallpaper-engine parser -p wallpaper.pkg -x output/ --parse-tex --parse-mdl
```

---

## 作为库使用

引擎本身就是一个库：命令行界面与图形界面都只是它的薄封装，你也可以如此。

```toml
[dependencies]
linux-wallpaper-engine = { git = "https://github.com/wqLouis/linux-wallpaperengine.git" }
```

```rust
use std::path::Path;

use linux_wallpaper_engine::engine::{EngineConfig, WallpaperHandle, scan_wallpapers};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 找出壁纸：创意工坊文件夹里每个壁纸各占一个目录。
    let workshop = Path::new("/home/me/.steam/steam/steamapps/workshop/content/431960");
    let wallpapers = scan_wallpapers(workshop)?;

    // 2. 启动其中一个：渲染器运行在自己的工作线程上。
    //    句柄就是壁纸的控制器：它接收命令（暂停、继续、停止……），
    //    并报告渲染器的状态。
    let _handle = WallpaperHandle::spawn(wallpapers[0].source.clone(), EngineConfig::default())?;

    // 3. 所有失败都是带原因的 `EngineError`，因此可以报告完整的错误链。
    Ok(())
}
```

库的 façade 详见 [docs/modules/engine.md](docs/modules/engine.md)。

---

## 贡献

欢迎提交 issue 与 pull request。

---

## 免责声明

本项目与 Wallpaper Engine 无任何关联或背书。
请通过在 [Steam](https://store.steampowered.com/app/431960/Wallpaper_Engine/) 上购买来支持原始软件。

## 授权

本项目采用 GPLv3 授权条款 —— 详见 [LICENSE](LICENSE) 文件。

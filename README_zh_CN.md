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

```bash
# Debian / Ubuntu
sudo apt install build-essential pkg-config libasound2-dev

# Fedora
sudo dnf install gcc pkgconf-pkg-config alsa-lib-devel

# Arch
sudo pacman -S base-devel pkgconf alsa-lib
```

另外还需要 Vulkan 驱动（AMD/Intel 用 Mesa，Nvidia 用官方驱动）。

**macOS**

macOS 适配器使用 Swift 编写，因此需要 Xcode 命令行工具（内含 `swiftc`）：

```bash
xcode-select --install
```

### 编译

```bash
git clone https://github.com/wqLouis/linux-wallpaper-engine.git
cd linux-wallpaper-engine
cargo build --release
```

### 安装

```bash
cargo install --path . --profile release
```

也可以手动复制可执行文件：

```bash
sudo install -Dm755 target/release/linux-wallpaper-engine /usr/local/bin/
```

GNOME 后端还需要安装 Shell 扩展：

```bash
./gnome-extension/install.sh
```

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

## 贡献

欢迎提交 issue 与 pull request。

---

## 免责声明

本项目与 Wallpaper Engine 无任何关联或背书。
请通过在 [Steam](https://store.steampowered.com/app/431960/Wallpaper_Engine/) 上购买来支持原始软件。

## 授权

本项目采用 GPLv3 授权条款 —— 详见 [LICENSE](LICENSE) 文件。

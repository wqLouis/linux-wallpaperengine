# Linux Wallpaper Engine

[English](README.md) | [中文繁體](README_zh_TW.md) | [中文簡體](README_zh_CN.md)

---

[![Rust](https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Wgpu](https://img.shields.io/badge/Wgpu-FF5A03?style=for-the-badge&logo=wgpu&logoColor=white)](https://github.com/gfx-rs/wgpu)
[![Vulkan](https://img.shields.io/badge/Vulkan-AC162C?style=for-the-badge&logo=vulkan&logoColor=white)](https://www.vulkan.org/)

Linux 上的 Wallpaper Engine —— 以 Rust 撰寫，透過 `wgpu` 算繪。

https://github.com/user-attachments/assets/16891f80-30ca-482c-9f25-17b0b8fdeca5

---

## 功能

### 已支援

- **wlr-layer-shell（Wayland）** —— 桌布以 `Layer::Background` 表面算繪，置於所有視窗之後，並支援分數縮放。
- **GNOME** —— GNOME Shell 並無 layer shell，故改為離屏算繪，再透過配套的 [GNOME Shell 擴充功能](gnome-extension/README.md) 將影格顯示為桌面背景。
- **特效著色器** —— Wallpaper Engine 的 GLSL 後處理著色器（泛光、水波紋等）會被轉譯為 WGSL，並於執行時編譯。
- **偶戲算繪** —— `.mdl` 模型會被解析並算繪為具骨骼動畫的 CPU 蒙皮網格。
- **音訊** —— 場景中的音軌會循環播放。

### 暫不支援

- **影片播放** —— `.mp4` / `.webm` 紋理僅顯示首幀。
- **使用者腳本** —— 忽略 `script.json` 與使用者腳本 API。
- **文字算繪** —— 不繪製文字圖層。
- **粒子系統** —— 不模擬粒子。

---

## 安裝

### 編譯相依

**Linux**

```bash
# Debian / Ubuntu
sudo apt install build-essential pkg-config libasound2-dev

# Fedora
sudo dnf install gcc pkgconf-pkg-config alsa-lib-devel

# Arch
sudo pacman -S base-devel pkgconf alsa-lib
```

另需 Vulkan 驅動程式（AMD/Intel 用 Mesa，Nvidia 用官方驅動）。

**macOS**

macOS 適配器以 Swift 撰寫，故需要 Xcode 命令列工具（內含 `swiftc`）：

```bash
xcode-select --install
```

### 編譯

```bash
git clone https://github.com/wqLouis/linux-wallpaper-engine.git
cd linux-wallpaper-engine
cargo build --release
```

### 安裝

```bash
cargo install --path . --profile release
```

亦可手動複製執行檔：

```bash
sudo install -Dm755 target/release/linux-wallpaper-engine /usr/local/bin/
```

GNOME 後端尚需安裝 Shell 擴充功能：

```bash
./gnome-extension/install.sh
```

---

## 使用

```bash
# wlr-layer-shell（預設）—— 位於所有視窗之後
linux-wallpaper-engine -p wallpaper.pkg

# GNOME Shell —— 需要 gnome-extension/ 中的擴充功能
linux-wallpaper-engine -p wallpaper.pkg -m gnome

# 一般視窗（X11 / Wayland），可追蹤游標
linux-wallpaper-engine -p wallpaper.pkg -m winit

# macOS（實驗性）
linux-wallpaper-engine -p wallpaper.pkg -m mac
```

### 參數

| 參數 | 說明 | 預設值 |
|--------|-------------|---------|
| `-p <path>` | `.pkg` 檔案路徑 | `./scene.pkg` |
| `-m <mode>` | 顯示模式：`wlr`、`winit`、`gnome` 或 `mac` | `wlr` |
| `--fit-mode <mode>` | 桌布適配：`cover`、`contain` 或 `stretch` | `cover` |
| `--no-effects` | 略過後處理特效 | `false` |
| `--no-mdl` | 忽略偶戲模型 | `false` |
| `--target-fps <fps>` | 限制影格率 | 不限 |
| `-l <level>` | 日誌層級：`verbose`、`debug`、`warning`、`errors` | `warning` |
| `--assets-path <dir>` | Wallpaper Engine 素材目錄（備援用） | — |

### 解壓桌布

```bash
# 列出封裝內檔案
linux-wallpaper-engine parser -p wallpaper.pkg --list

# 解壓
linux-wallpaper-engine parser -p wallpaper.pkg -x output/

# 解壓並將紋理轉為 PNG / 解析偶戲模型
linux-wallpaper-engine parser -p wallpaper.pkg -x output/ --parse-tex --parse-mdl
```

---

## 貢獻

歡迎提交 issue 與 pull request。

---

## 免責聲明

本專案與 Wallpaper Engine 並無任何關聯或背書。
請透過於 [Steam](https://store.steampowered.com/app/431960/Wallpaper_Engine/) 購買以支持原始軟體。

## 授權

本專案採 GPLv3 授權 —— 詳見 [LICENSE](LICENSE)。

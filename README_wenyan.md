# 華牆引擎

> **文言版說明書**

[English](README.md) | [中文繁體](README_zh_TW.md) | [中文簡體](README_zh_CN.md)

---

## 緣起

華牆引擎者，Wallpaper Engine 之Linux譯本也。以Rust為基，借wgpu之力，令桌面生動如Windows之時。

https://github.com/user-attachments/assets/16891f80-30ca-482c-9f25-17b0b8fdeca5

---

## 功能

### 已支持

- **wlr-layer-shell（Wayland）** — 以`Layer::Background`渲染，居眾窗之底，支援分數縮放。
- **GNOME** — GNOME Shell無layer-shell，故離屏渲染，以[GNOME Shell擴充功能](gnome-extension/README.md)呈之於桌面。
- **特效著色器** — Wallpaper Engine之GLSL後處理著色器（泛光、水波諸效），譯為WGSL，運行時編譯。
- **偶戲算繪** — `.mdl`模型解析成帶骨骼動畫之CPU蒙皮網格。
- **音訊** — 場景音軌循環播放。

### 未支持

- **影片播放** — `.mp4`、`.webm`紋理僅顯首幀。
- **使用者腳本** — `script.json`及腳本API皆忽之。
- **文字算繪** — 不繪文字圖層。
- **粒子系統** — 不模擬粒子。

---

## 安裝

### 依賴

**Linux**

```bash
# Debian / Ubuntu
sudo apt install build-essential pkg-config libasound2-dev

# Fedora
sudo dnf install gcc pkgconf-pkg-config alsa-lib-devel

# Arch
sudo pacman -S base-devel pkgconf alsa-lib
```

復需Vulkan驅動（AMD、Intel用Mesa，Nvidia用專有驅動）。

**macOS**

macOS適配器以Swift寫之，故需Xcode命令列工具（含`swiftc`）：

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

亦可手置之於`PATH`：

```bash
sudo install -Dm755 target/release/linux-wallpaper-engine /usr/local/bin/
```

GNOME後端尚需置其擴充功能：

```bash
./gnome-extension/install.sh
```

---

## 使用

```bash
# wlr-layer-shell（預設）——居眾窗之底
linux-wallpaper-engine -p wallpaper.pkg

# GNOME Shell ——需gnome-extension/之擴充功能
linux-wallpaper-engine -p wallpaper.pkg -m gnome

# 常窗（X11、Wayland），可追蹤游標
linux-wallpaper-engine -p wallpaper.pkg -m winit

# macOS（實驗）
linux-wallpaper-engine -p wallpaper.pkg -m mac
```

### 參數

| 參數 | 說明 | 預設 |
|----------|-------------|--------|
| `-p` | `.pkg`路徑 | `./scene.pkg` |
| `-m` | 模式：`wlr`、`winit`、`gnome`、`mac` | `wlr` |
| `--fit-mode` | 適配：`cover`、`contain`、`stretch` | `cover` |
| `--no-effects` | 跳過後處理 | `false` |
| `--no-mdl` | 忽略偶戲模型 | `false` |
| `--target-fps` | 限幀率 | 不限 |
| `-l` | 日誌級別 | `warning` |
| `--assets-path` | Wallpaper Engine素材目錄 | — |

### 解壓

```bash
# 列出封裝內檔案
linux-wallpaper-engine parser -p wallpaper.pkg --list

# 解壓
linux-wallpaper-engine parser -p wallpaper.pkg -x output/

# 解壓並轉紋理為PNG、解析偶戲
linux-wallpaper-engine parser -p wallpaper.pkg -x output/ --parse-tex --parse-mdl
```

---

## 貢獻

Issue、pull request，皆可投之。

---

## 結語

本項目與Wallpaper Engine無涉。請支持正版，於Steam購之。

GPLv3 授權，詳見 [LICENSE](LICENSE)。

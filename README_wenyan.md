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
- **圖形介面** — 原生系統匣之應用也（以[gpui-ce](https://crates.io/crates/gpui-ce)為之），列工作坊資料夾中之桌布，而算繪所選者。
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

命令列者，需C工具鏈、`pkg-config`及ALSA標頭檔（引擎以`rodio`播場景之音）：

```bash
# Debian / Ubuntu
sudo apt install build-essential pkg-config libasound2-dev

# Fedora
sudo dnf install gcc pkgconf-pkg-config alsa-lib-devel

# Arch
sudo pacman -S base-devel pkgconf alsa-lib
```

圖形介面者，另需**fontconfig**（介面之字）與**xkbcommon**（鍵盤之制），蓋gpui-ce之Linux後端繫此二者：

```bash
# Debian / Ubuntu
sudo apt install libfontconfig-dev libxkbcommon-dev

# Fedora
sudo dnf install fontconfig-devel libxkbcommon-devel

# Arch
sudo pacman -S fontconfig libxkbcommon
```

復需Vulkan驅動（AMD、Intel用Mesa，Nvidia用專有驅動）。

**macOS**

macOS適配器以Swift寫之，故需Xcode命令列工具（含`swiftc`）：

```bash
xcode-select --install
```

### 編譯

```bash
git clone https://github.com/wqLouis/linux-wallpaperengine.git
cd linux-wallpaper-engine
git submodule update --init --recursive   # 封裝解析者，git子模組也
cargo build --release
```

此倉者，cargo workspace也，含三包：

| 包 | 目錄 | 所得 |
|---|---|---|
| `linux-wallpaper-engine` | `.` | 函式庫（`linux_wallpaper_engine`） |
| `linux-wallpaper-engine-cli` | `crates/lwe-cli` | 命令列（`linux-wallpaper-engine`） |
| `linux-wallpaper-engine-gui` | `crates/lwe-gui` | 圖形介面（`linux-wallpaper-engine-gui`） |

```bash
cargo build                # 函式庫與命令列（默認成員）
cargo build-cli            # 唯命令列
cargo build-gui            # 唯圖形介面
cargo build --workspace    # 悉皆
cargo test                 # 函式庫與命令列之試
```

`cargo build-cli`、`cargo build-gui`者，`.cargo/config.toml`中之別名也，即`cargo build -p linux-wallpaper-engine-cli`、`cargo build -p linux-wallpaper-engine-gui`是也。

### 安裝

```bash
# 命令列
cargo install --path . --profile release

# 圖形介面
cargo install --path crates/lwe-gui --profile release
```

亦可手置之於`PATH`：

```bash
sudo install -Dm755 target/release/linux-wallpaper-engine /usr/local/bin/
sudo install -Dm755 target/release/linux-wallpaper-engine-gui /usr/local/bin/
```

GNOME後端尚需置其擴充功能：

```bash
./gnome-extension/install.sh
```

---

## 圖形介面

```bash
linux-wallpaper-engine-gui
```

圖形介面者，系統匣之應用也：於D-Bus之上置一**StatusNotifierItem**（即桌面之匣圖示），而常駐於後。點其圖示，則開一選單，列所選工作坊資料夾中之桌布；所選者由引擎算繪為桌面之背景 —— 於同一行程之內、工作執行緒之上，故介面常保回應。

初啟之時，圖形介面問工作坊資料夾所在（默認為`~/.steam/steam/steamapps/workshop/content/431960`）。

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

## 作為函式庫

引擎者，函式庫也：命令列與圖形介面皆其薄衣，閣下亦可為之。

```toml
[dependencies]
linux-wallpaper-engine = { git = "https://github.com/wqLouis/linux-wallpaperengine.git" }
```

```rust
use std::path::Path;

use linux_wallpaper_engine::engine::{EngineConfig, WallpaperHandle, scan_wallpapers};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 一、尋桌布：工作坊資料夾中，每桌布各佔一目錄。
    let workshop = Path::new("/home/me/.steam/steam/steamapps/workshop/content/431960");
    let wallpapers = scan_wallpapers(workshop)?;

    // 二、啟其一：算繪器行於己之工作執行緒。
    //    握把者，桌布之控也：受命令（暫停、續、止……），而報其狀。
    let _handle = WallpaperHandle::spawn(wallpapers[0].source.clone(), EngineConfig::default())?;

    // 三、所有失誤皆為帶因之`EngineError`，故可陳其全鏈。
    Ok(())
}
```

函式庫之façade，見 [docs/modules/engine.md](docs/modules/engine.md)。

---

## 貢獻

Issue、pull request，皆可投之。

---

## 結語

本項目與Wallpaper Engine無涉。請支持正版，於Steam購之。

GPLv3 授權，詳見 [LICENSE](LICENSE)。

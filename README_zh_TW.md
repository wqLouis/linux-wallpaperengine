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
- **圖形介面** —— 原生系統匣應用程式（以 [gpui-ce](https://crates.io/crates/gpui-ce) 撰寫），列出工作坊資料夾中的桌布並算繪所選的那一個。
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

命令列介面需要 C 工具鏈、`pkg-config` 以及 ALSA 標頭檔（引擎以 `rodio` 播放場景音訊）：

```bash
# Debian / Ubuntu
sudo apt install build-essential pkg-config libasound2-dev

# Fedora
sudo dnf install gcc pkgconf-pkg-config alsa-lib-devel

# Arch
sudo pacman -S base-devel pkgconf alsa-lib
```

圖形介面另需兩個套件：Linux 版 gpui-ce 後端連結 **fontconfig**（介面字型）與 **xkbcommon**（鍵盤配置）：

```bash
# Debian / Ubuntu
sudo apt install libfontconfig-dev libxkbcommon-dev

# Fedora
sudo dnf install fontconfig-devel libxkbcommon-devel

# Arch
sudo pacman -S fontconfig libxkbcommon
```

另需 Vulkan 驅動程式（AMD/Intel 用 Mesa，Nvidia 用官方驅動）。

**macOS**

macOS 適配器以 Swift 撰寫，故需要 Xcode 命令列工具（內含 `swiftc`）：

```bash
xcode-select --install
```

### 編譯

```bash
git clone https://github.com/wqLouis/linux-wallpaperengine.git
cd linux-wallpaper-engine
git submodule update --init --recursive   # 封裝解析器為 git 子模組
cargo build --release
```

本倉庫是一個 cargo workspace，包含三個套件：

| 套件 | 目錄 | 產物 |
|---|---|---|
| `linux-wallpaper-engine` | `.` | 函式庫（`linux_wallpaper_engine`） |
| `linux-wallpaper-engine-cli` | `crates/lwe-cli` | 命令列介面（`lwe-cli`） |
| `linux-wallpaper-engine-gui` | `crates/lwe-gui` | 圖形介面（`lwe-gui`） |

```bash
cargo build                # 函式庫 + 命令列介面（預設成員）
cargo build-cli            # 僅命令列介面
cargo build-gui            # 僅圖形介面
cargo build --workspace    # 全部
cargo test                 # 函式庫與命令列介面的測試
```

`cargo build-cli` 與 `cargo build-gui` 是 `.cargo/config.toml` 中的別名，分別等同於
`cargo build -p linux-wallpaper-engine-cli` 與 `cargo build -p linux-wallpaper-engine-gui`。

### 安裝

```bash
# 命令列介面
cargo install --path . --profile release

# 圖形介面
cargo install --path crates/lwe-gui --profile release
```

亦可手動複製執行檔：

```bash
sudo install -Dm755 target/release/lwe-cli /usr/local/bin/
sudo install -Dm755 target/release/lwe-gui /usr/local/bin/
```

GNOME 後端尚需安裝 Shell 擴充功能：

```bash
./gnome-extension/install.sh
```

---

## 圖形介面

```bash
lwe-gui
```

圖形介面是一個系統匣應用程式：它於 D-Bus 上註冊一個 **StatusNotifierItem**（即桌面的系統匣圖示），並常駐背景。點選圖示會開啟一個選擇器，列出所選工作坊資料夾中的桌布；選中的桌布由引擎算繪為桌面背景 —— 於同一行程內、在工作執行緒上完成，故介面始終保持回應。

首次啟動時，圖形介面會詢問工作坊資料夾的位置（預設為
`~/.steam/steam/steamapps/workshop/content/431960`）。

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

## 作為函式庫使用

引擎本身即是一個函式庫：命令列介面與圖形介面皆為其薄殼層，閣下亦可如此。

```toml
[dependencies]
linux-wallpaper-engine = { git = "https://github.com/wqLouis/linux-wallpaperengine.git" }
```

```rust
use std::path::Path;

use linux_wallpaper_engine::engine::{EngineConfig, WallpaperHandle, scan_wallpapers};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 尋得桌布：工作坊資料夾中，每個桌布各佔一目錄。
    let workshop = Path::new("/home/me/.steam/steam/steamapps/workshop/content/431960");
    let wallpapers = scan_wallpapers(workshop)?;

    // 2. 啟動其中之一：算繪器運行於自身之工作執行緒。
    //    握把即為桌布之控制者：它接受命令（暫停、繼續、停止……），
    //    並回報算繪器之狀態。
    let _handle = WallpaperHandle::spawn(wallpapers[0].source.clone(), EngineConfig::default())?;

    // 3. 所有失敗皆為帶其因之 `EngineError`，故得以呈報完整之錯誤鏈。
    Ok(())
}
```

函式庫之 façade 詳見 [docs/modules/engine.md](docs/modules/engine.md)。

---

## 貢獻

歡迎提交 issue 與 pull request。

---

## 免責聲明

本專案與 Wallpaper Engine 並無任何關聯或背書。
請透過於 [Steam](https://store.steampowered.com/app/431960/Wallpaper_Engine/) 購買以支持原始軟體。

## 授權

本專案採 GPLv3 授權 —— 詳見 [LICENSE](LICENSE)。

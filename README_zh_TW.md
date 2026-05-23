# Linux Wallpaper Engine

[English](README.md) | [中文繁體](README_zh_TW.md) | [中文简体](README_zh_CN.md) | [文言](README_wenyan.md)

---

本專案旨在將 [Wallpaper Engine](https://www.wallpaperengine.io/en) 的相容性帶到 Linux（以及可能的 macOS）。它使用 Rust 編寫，並借助 `wgpu` 與 Vulkan（macOS 上為 Metal）來渲染互動式 3D 桌布。

> **狀態：** 軟體運作正常，但仍處於活躍開發階段。許多具有後處理效果的桌布都能正常運作，但部分功能（完整動畫、影片播放）仍未完成。

https://github.com/user-attachments/assets/16891f80-30ca-482c-9f25-17b0b8fdeca5

## 功能特色

### 渲染
- **硬體加速渲染** — 透過 `wgpu`（Vulkan/Metal 後端）
- **GLSL 著色器支援** — Wallpaper Engine 的 `.frag`/`.vert` 著色器會經由 GLSL→WGSL 預處理器轉譯，並在執行時動態編譯，用於後處理效果
- **正交攝影機** — 根據 scene.json 參數設定（look-at + 正交投影）
- **Alpha 混合** — 支援可配置的混合模式
- **後處理管線** — 採用來回多通道渲染（ping-pong multi-pass rendering）處理效果（泛光、水波紋等）
- **每幀統一變數**：`g_Time`、`g_ModelViewProjectionMatrix`、`g_Screen`、`g_ParallaxPosition` 以及命名材質常數
- **遮罩與噪聲紋理支援** — 後處理效果中可用
- **無效果模式**（`--no-effects`）— 以靜態桌布影像進行除錯

### 顯示適配器
- **wlr-layer-shell（Wayland）：** 以 `Layer::Background` 表面渲染，置於所有視窗底層。使用 `wlr_layer_shell` 協定。支援透過 `wp-fractional-scale-v1` 和 `wp-viewporter` 的分數縮放。
- **Winit（X11/Wayland）：** 建立一個置底的視窗，並追蹤游標位置以實現深度視差效果。

### 套件解析（`pkg_parser`）
- **`.pkg` 檔案解壓與解析** — 讀取打包的桌布檔案
- **`.tex` 紋理解析** — 支援 DXT1、DXT5、R8、RG88、PNG、JPEG 格式，自動偵測格式並支援 LZ4 解壓縮。可匯出為 PNG 或轉換為 RGBA 以供渲染
- **`.mdl` 偶戲模型解析** — 讀取 MDLV0023 格式，解析控制點、三角形、骨骼（MDLS）及動畫（MDLA）區段。可序列化為 JSON
- **影片/GIF 中繼資料解析** — 偵測 MP4、WebM、GIF 格式，並可擷取 GIF 幀

### 音頻
- **音訊播放** — 透過 `rodio` 支援循環播放場景音軌

### 命令列功能
- 兩種顯示模式：`wlr`（預設，Wayland 背景）和 `winit`（X11/Wayland 視窗）
- 桌布適配模式：`cover`、`contain`、`stretch`
- 解壓/解析模式（`-x`）：解壓並可選擇將 `.tex`→PNG、解析影片、解析 `.mdl` 模型為 JSON
- 可配置的日誌級別：`verbose`、`debug`、`warning`（預設）、`errors`

## 系統需求

* **Rust**（最新穩定版本；2024 版）
* **Vulkan 驅動程式：** 確保您的 GPU 驅動程式支援 Vulkan（AMD/Intel 使用 Mesa，Nvidia 使用專有驅動程式）
* **macOS 支援：** 未經測試（無 Mac 設備），但已包含 Metal 後端
* **Wallpaper Engine 資源：** 您必須合法持有 `.pkg` 檔案（例如透過 Steam 購買 Wallpaper Engine）

### 依賴項
- `wgpu` 28.0 啟用 `glsl` 功能（Naga GLSL 前端）
- `winit` 0.30 用於視窗適配器
- `smithay-client-toolkit` 0.20 + `wayland-client` 0.31 用於 wlr-layer-shell
- `glam` 0.31 用於線性代數
- `clap` 4.5 用於命令列參數解析
- `rodio` 0.21 用於音訊播放
- `serde` / `serde_json` 用於場景 JSON 解析

## 安裝

### 原始碼編譯

1.  複製倉庫：
    ```bash
    git clone https://github.com/wqLouis/linux-wallpaper-engine.git
    cd linux-wallpaper-engine
    ```

2.  編譯專案：
    ```bash
    cargo build --profile=release
    ```

3.  安裝：
    ```bash
    cargo install --path . --profile=release
    ```

### Arch Linux 用戶

```bash
paru -S linux-wallpaper-engine-git
```

## 使用方式

```bash
# 執行桌布（預設 wlr 模式）
linux-wallpaper-engine -p path/to/wallpaper.pkg

# 使用 winit 適配器執行（視窗模式，支援游標追蹤以實現視差效果）
linux-wallpaper-engine -p path/to/wallpaper.pkg -m winit

# 解壓並解析 .pkg 檔案
linux-wallpaper-engine -p path/to/wallpaper.pkg -x [output_dir]

# 解壓並將紋理轉換為 PNG
linux-wallpaper-engine -p path/to/wallpaper.pkg -x --parse-tex

# 解壓並解析影片/GIF 中繼資料
linux-wallpaper-engine -p path/to/wallpaper.pkg -x --parse-video

# 解壓並將 MDL 偶戲模型匯出為 JSON
linux-wallpaper-engine -p path/to/wallpaper.pkg -x --parse-mdl

# 預覽模式（不寫入檔案，只顯示將要解壓的內容）
linux-wallpaper-engine -p path/to/wallpaper.pkg -x --dry-run

# 無效果模式（除錯用，渲染靜態影像）
linux-wallpaper-engine -p path/to/wallpaper.pkg --no-effects

# 變更桌布適配模式
linux-wallpaper-engine -p path/to/wallpaper.pkg --fit-mode contain

# 詳細日誌
linux-wallpaper-engine -p path/to/wallpaper.pkg -l verbose
```

### 命令列參數
| 參數 | 說明 | 預設值 |
|----------|-------------|--------|
| `-p` / `<path>` | `.pkg` 檔案路徑 | `./scene.pkg` |
| `-m` / `<modes>` | 顯示模式：`wlr` 或 `winit` | `wlr` |
| `--fit-mode` | 桌布適配：`cover`、`contain`、`stretch` | `cover` |
| `--no-effects` | 略過後處理，渲染靜態影像 | `false` |
| `-l` / `--log-level` | `verbose`、`debug`、`warning`、`errors` | `warning` |
| `-x` / `[output]` | 解壓模式（可選擇指定輸出目錄）| 停用 |
| `--parse-tex` | 將 `.tex` 紋理轉換為 PNG（解壓模式）| `false` |
| `--parse-video` | 解析影片/GIF 中繼資料（解壓模式）| `false` |
| `--parse-mdl` | 將 `.mdl` 偶戲模型解析為 JSON（解壓模式）| `false` |
| `--dry-run` | 顯示將解壓的檔案，但不寫入 | `false` |

## 專案結構

```
src/
├── main.rs                       # 命令列入口點，使用 clap 解析參數
├── pkg_parser/                   # （獨立 crate）.pkg 檔案解析器
│   └── src/pkg_parser/
│       ├── parser.rs             # .pkg 檔案格式讀取與解壓
│       ├── tex_parser.rs         # .tex 紋理載入（LZ4、DXT、PNG、JPEG）
│       ├── video_parser.rs       # 影片/GIF 中繼資料解析與幀擷取
│       └── mdl_parser.rs         # MDL 偶戲模型解析與 JSON 匯出
└── scene/
    ├── mod.rs                    # 模組宣告
    ├── loader/
    │   ├── scene.rs              # scene.json 結構型別（Root、Camera、General 等）
    │   ├── scene_loader.rs       # 從 .pkg 載入場景（平行紋理解析）
    │   ├── object.rs             # 物件 JSON 結構，包含所有 WP Engine 屬性
    │   ├── object_loader.rs      # 物件對應建構（紋理/音訊/節點層級）
    │   └── model.rs              # 模型 JSON 結構
    ├── renderer/
    │   ├── app.rs                # WgpuApp：主要 GPU 狀態與渲染循環
    │   ├── surface.rs            # 表面抽象層（原始控制代碼 + winit）
    │   ├── load.rs               # 資源載入與管線建立
    │   ├── buffer.rs             # 頂點/索引/投影 GPU 緩衝區
    │   ├── draw.rs               # DrawQueue 與 DrawObject 建構
    │   ├── vertex.rs             # 頂點型別與 NDC 頂點
    │   ├── projection.rs         # 正交攝影機投影
    │   ├── render_pass.rs        # 最終渲染通道與統一變數寫入
    │   ├── intermediate_pass.rs  # 來回效果渲染通道
    │   ├── effect_bindgroup.rs   # 效果綁定群組建構
    │   ├── ping_pong.rs          # 來回紋理對管理
    │   ├── post_process.rs       # 後處理取樣器與佈局
    │   └── post_processor/       # GLSL→WGSL 著色器預處理
    │       ├── shader_header.rs  # 著色器包含標頭（common.h 等）
    │       ├── shader_headers/   # GLSL 包含檔案（*.h）
    │       ├── effect_param.rs   # 統一變數佈局與每幀參數填充
    │       ├── pipeline_handler.rs # 效果管線建立與快取
    │       ├── pipeline_helpers.rs # 定義收集與綁定群組佈局
    │       └── transform/        # 著色器原始碼轉換
    │           ├── layout.rs     # 從 GLSL 產生 EffectLayout
    │           ├── mod.rs        # 預處理管線（標頭注入、可變/屬性翻譯）
    │           └── replace.rs    # GLSL→GLSL 修正（mul、saturate、texSample2D 等）
    └── adapters/
        ├── mod.rs               # FitMode 列舉
        ├── winit_adapter.rs     # Winit 視窗適配器（置底視窗、游標追蹤）
        └── wlr_app/
            ├── mod.rs           # wlr-layer-shell Wayland 適配器（Background 圖層）
            └── scale.rs         # 分數縮放狀態管理
```

## 已知問題與限制

* **影片播放：** 影片紋理（.tex 檔案內的 mp4/webm）會被偵測但不會在執行時解碼，將顯示為靜態幀。GIF 紋理可能部分可用。
* **動畫：** `.mdl` 偶戲模型的骨骼動畫已解析但未播放。
* **著色器相容性：** 部分 Wallpaper Engine 著色器結構可能無法正確轉譯。GLSL→WGSL 預處理管線處理常見情況，但仍存在邊緣案例。
* **Wayland 上的游標追蹤：** wlr-layer-shell 適配器無法在 `Layer::Background` 表面上接收指標事件，因為 Wayland 的安全模型。深度視差效果在 wlr 模式下不可用。
* **macOS：** 未經測試。

## 開發路線圖

- [x] 提升穩定性與錯誤處理
- [x] 實作音訊支援
- [x] .mdl 檔案解析
- [x] 著色器預處理與效果管線
- [x] wlr-layer-shell Wayland 適配器
- [x] 多種桌布適配模式
- [x] .pkg 解壓與檔案解析工具
- [x] 紋理格式偵測與轉換（DXT、R8、RG88、PNG、JPEG）
- [x] 後處理效果管線（來回多通道）
- [ ] 影片紋理播放
- [ ] 偶戲模型動畫
- [ ] 設定檔支援
- [ ] 多螢幕支援

## 貢獻

歡迎提交貢獻！

## 授權

本專案採用 GPLv3 授權條款 — 詳見 LICENSE 檔案。

## 免責聲明

本專案與 Wallpaper Engine 無任何關聯或背書。請透過在 Steam 上購買來支持原始軟體。

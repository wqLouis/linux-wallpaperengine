# 華牆引擎

> **文言版說明書**

---

## 緣起

華牆引擎者，Wallpaper Engine 之Linux譯本也。昔有Wallpaper Engine 者，Windows之良品，可令桌面生動，活潑若此。今有好事者，以Rust為基，借wgpu之力， Vulkan之功，復原其效於Linux之上，亦可至於macOS焉。

> **現況：** 此器可用，然尚在增益之中。多數後處理之效已可運行，而視訊播放、骨骼動畫尚未完成。

https://github.com/user-attachments/assets/16891f80-30ca-482c-9f25-17b0b8fdeca5

## 功能

### 渲染

- **硬件加速** — 借`wgpu`之力，Vulkan、Metal皆可馭之
- **GLSL著色器** — Wallpaper Engine之`.frag`/`.vert`著色器，經GLSL→WGSL預處理器譯之，運行時編譯，以成後處理之效
- **正交攝影** — 依scene.json參數而立，look-at加正交投影
- **Alpha混合** — 多種混合模式可選
- **後處理管線** — 來回多通道渲染（ping-pong），以成泛光、水波諸效
- **每幀統一變量**：`g_Time`、`g_ModelViewProjectionMatrix`、`g_Screen`、`g_ParallaxPosition`及材質常數
- **遮罩噪聲紋理** — 後處理中可用
- **無效模式**（`--no-effects`）— 以靜圖調試

### 顯示適配

- **wlr-layer-shell（Wayland）:** 以`Layer::Background`渲染，居眾窗之底。用`wlr_layer_shell`協議，支援分數縮放。
- **Winit（X11/Wayland）:** 建置底視窗，追蹤游標，以成視差之效。

### 套件解析（pkg_parser）

- **`.pkg`解壓與解析** — 讀取打包之牆紙文件
- **`.tex`紋理解析** — 支援DXT1、DXT5、R8、RG88、PNG、JPEG，自動識別，LZ4解壓
- **`.mdl`偶戲模型解析** — MDLV0023格式，讀取控制點、三角形、骨骼及動畫，可序列化為JSON
- **視訊/GIF解析** — 識別MP4、WebM、GIF，可提取GIF幀

### 音頻

- **音訊播放** — 借`rodio`之力，支援場景音軌循環

### 命令列

- 雙顯示模式：`wlr`（預設）與`winit`
- 牆紙適配：`cover`、`contain`、`stretch`
- 解壓模式（`-x`）：解壓`.pkg`，可選轉`.tex`為PNG、解析視訊、解析`.mdl`為JSON
- 日誌級別：`verbose`、`debug`、`warning`（預設）、`errors`

---

## 系統需求

- **Rust** — 最新穩定版
- **Vulkan驅動** — AMD/Intel用Mesa，Nvidia用專有驅動
- **macOS** — 未測試，已有Metal後端
- **華牆引擎素材** — 須合法持有`.pkg`文件

### 依賴

`wgpu` · `winit` · `smithay-client-toolkit` · `wayland-client` · `glam` · `clap` · `rodio` · `serde`

---

## 安裝

### 源碼編譯

```bash
git clone https://github.com/wqLouis/linux-wallpaper-engine.git
cd linux-wallpaper-engine
cargo build --profile=release
cargo install --path . --profile=release
```

### Arch用戶

```bash
paru -S linux-wallpaper-engine-git
```

---

## 使用

```bash
# 運行牆紙（預設wlr模式）
linux-wallpaper-engine -p path/to/wallpaper.pkg

# winit模式（視窗+游標追蹤）
linux-wallpaper-engine -p path/to/wallpaper.pkg -m winit

# 解壓.pkg
linux-wallpaper-engine -p path/to/wallpaper.pkg -x [output_dir]

# 轉紋理為PNG
linux-wallpaper-engine -p path/to/wallpaper.pkg -x --parse-tex

# 預覽模式
linux-wallpaper-engine -p path/to/wallpaper.pkg -x --dry-run

# 無效模式（調試）
linux-wallpaper-engine -p path/to/wallpaper.pkg --no-effects

# 詳細日誌
linux-wallpaper-engine -p path/to/wallpaper.pkg -l verbose
```

### 參數表

| 參數 | 說明 | 預設 |
|----------|-------------|--------|
| `-p` | `.pkg`路徑 | `./scene.pkg` |
| `-m` | 模式：`wlr`或`winit` | `wlr` |
| `--fit-mode` | 適配：`cover`、`contain`、`stretch` | `cover` |
| `--no-effects` | 跳過後處理 | `false` |
| `-l` | 日誌級別 | `warning` |
| `-x` | 解壓模式 | 停用 |
| `--parse-tex` | 轉`.tex`為PNG | `false` |
| `--parse-video` | 解析視訊 | `false` |
| `--parse-mdl` | 解析`.mdl`為JSON | `false` |
| `--dry-run` | 預覽不解壓 | `false` |

---

## 專案結構

```
src/
├── main.rs              # 命令列入口
├── pkg_parser/          # .pkg解析器
│   ├── parser.rs        # .pkg讀取
│   ├── tex_parser.rs    # .tex紋理
│   ├── video_parser.rs  # 視訊解析
│   └── mdl_parser.rs    # .mdl模型
└── scene/
    ├── loader/          # 場景載入
    ├── renderer/        # 渲染器
    │   ├── app.rs       # 主渲染
    │   ├── load.rs      # 資源載入
    │   ├── draw.rs      # 繪製
    │   └── post_processor/  # 著色器預處理
    └── adapters/        # 顯示適配
        ├── winit_adapter.rs
        └── wlr_app/     # Wayland適配
```

---

## 已知之限

- **視訊播放** — 視訊紋理可識別但不解碼，僅顯首幀
- **動畫** — `.mdl`骨骼動畫已解析但未播放
- **著色器兼容** — 少數著色器結構或不能譯
- **Wayland游標追蹤** — `Layer::Background`不可接收指標事件
- **macOS** — 未測試

---

## 開發藍圖

- [x] 穩定性與錯誤處理
- [x] 音訊支援
- [x] `.mdl`解析
- [x] 著色器預處理
- [x] Wayland適配
- [x] 多種適配模式
- [x] `.pkg`解壓工具
- [x] 後處理管線
- [ ] 視訊紋理播放
- [ ] 偶戲模型動畫
- [ ] 配置文件支援
- [ ] 多螢幕支援

---

## 結語

本項目與Wallpaper Engine無涉，請支持正版，在Steam購之。

GPLv3 授權

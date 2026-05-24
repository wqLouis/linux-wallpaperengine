# Linux Wallpaper Engine

[English](README.md) | [中文繁体](README_zh_TW.md) | [中文简体](README_zh_CN.md)

---

本项目旨在将 [Wallpaper Engine](https://www.wallpaperengine.io/en) 的兼容性带到 Linux（以及可能的 macOS）。它使用 Rust 编写，并借助 `wgpu` 与 Vulkan（macOS 上为 Metal）来渲染互动式 3D 壁纸。

> **状态：** 软件运作正常，但仍处于活跃开发阶段。许多具有后处理效果的壁纸都能正常运行，但部分功能（完整动画、视频播放）仍未完成。

https://github.com/user-attachments/assets/16891f80-30ca-482c-9f25-17b0b8fdeca5

## 功能特色

### 渲染
- **硬件加速渲染** — 通过 `wgpu`（Vulkan/Metal 后端）
- **GLSL 着色器支持** — Wallpaper Engine 的 `.frag`/`.vert` 着色器会通过 GLSL→WGSL 预处理器转译，并在运行时动态编译，用于后处理效果
- **正交摄像机** — 根据 scene.json 参数设定（look-at + 正交投影）
- **Alpha 混合** — 支持可配置的混合模式
- **后处理管线** — 采用来回多通道渲染（ping-pong multi-pass rendering）处理效果（泛光、水波纹等）
- **每帧统一变量**：`g_Time`、`g_ModelViewProjectionMatrix`、`g_Screen`、`g_ParallaxPosition` 以及命名材质常数
- **遮罩与噪声纹理支持** — 后处理效果中可用
- **无效果模式**（`--no-effects`）— 以静态壁纸图像进行调试

### 显示适配器
- **wlr-layer-shell（Wayland）：** 以 `Layer::Background` 表面渲染，置于所有窗口底层。使用 `wlr_layer_shell` 协议。支持通过 `wp-fractional-scale-v1` 和 `wp-viewporter` 的分数缩放。
- **Winit（X11/Wayland）：** 建立一个置底的窗口，并追踪光标位置以实现深度视差效果。

### 套件解析（`pkg_parser`）
- **`.pkg` 文件解压与解析** — 读取打包的壁纸文件
- **`.tex` 纹理解析** — 支持 DXT1、DXT5、R8、RG88、PNG、JPEG 格式，自动侦测格式并支持 LZ4 解压缩。可导出为 PNG 或转换为 RGBA 以供渲染
- **`.mdl` 偶戏模型解析** — 读取 MDLV0023 格式，解析控制点、三角形、骨骼（MDLS）及动画（MDLA）区段。可序列化为 JSON
- **视频/GIF 元数据解析** — 侦测 MP4、WebM、GIF 格式，并可撷取 GIF 帧

### 音频
- **音频播放** — 通过 `rodio` 支持循环播放场景音轨

### 命令行功能
- 两种显示模式：`wlr`（默认，Wayland 背景）和 `winit`（X11/Wayland 窗口）
- 壁纸适配模式：`cover`、`contain`、`stretch`
- 解压/解析模式（`-x`）：解压并可选择将 `.tex`→PNG、解析视频、解析 `.mdl` 模型为 JSON
- 可配置的日志级别：`verbose`、`debug`、`warning`（默认）、`errors`

## 系统需求

* **Rust**（最新稳定版本；2024 版）
* **Vulkan 驱动程序：** 确保您的 GPU 驱动程序支持 Vulkan（AMD/Intel 使用 Mesa，Nvidia 使用专有驱动程序）
* **macOS 支持：** 未经测试（无 Mac 设备），但已包含 Metal 后端
* **Wallpaper Engine 资源：** 您必须合法持有 `.pkg` 文件（例如通过 Steam 购买 Wallpaper Engine）

### 依赖项
- `wgpu` 28.0 启用 `glsl` 功能（Naga GLSL 前端）
- `winit` 0.30 用于窗口适配器
- `smithay-client-toolkit` 0.20 + `wayland-client` 0.31 用于 wlr-layer-shell
- `glam` 0.31 用于线性代数
- `clap` 4.5 用于命令行参数解析
- `rodio` 0.21 用于音频播放
- `serde` / `serde_json` 用于场景 JSON 解析

## 安装

### 源代码编译

1.  复制仓库：
    ```bash
    git clone https://github.com/wqLouis/linux-wallpaper-engine.git
    cd linux-wallpaper-engine
    ```

2.  编译项目：
    ```bash
    cargo build --profile=release
    ```

3.  安装：
    ```bash
    cargo install --path . --profile=release
    ```

### Arch Linux 用户

```bash
paru -S linux-wallpaper-engine-git
```

## 使用方式

```bash
# 运行壁纸（默认 wlr 模式）
linux-wallpaper-engine -p path/to/wallpaper.pkg

# 使用 winit 适配器运行（窗口模式，支持光标追踪以实现视差效果）
linux-wallpaper-engine -p path/to/wallpaper.pkg -m winit

# 解压并解析 .pkg 文件
linux-wallpaper-engine -p path/to/wallpaper.pkg -x [output_dir]

# 解压并将纹理转换为 PNG
linux-wallpaper-engine -p path/to/wallpaper.pkg -x --parse-tex

# 解压并解析视频/GIF 元数据
linux-wallpaper-engine -p path/to/wallpaper.pkg -x --parse-video

# 解压并将 MDL 偶戏模型导出为 JSON
linux-wallpaper-engine -p path/to/wallpaper.pkg -x --parse-mdl

# 预览模式（不写入文件，只显示将要解压的内容）
linux-wallpaper-engine -p path/to/wallpaper.pkg -x --dry-run

# 无效果模式（调试用，渲染静态图像）
linux-wallpaper-engine -p path/to/wallpaper.pkg --no-effects

# 更改壁纸适配模式
linux-wallpaper-engine -p path/to/wallpaper.pkg --fit-mode contain

# 详细日志
linux-wallpaper-engine -p path/to/wallpaper.pkg -l verbose
```

### 命令行参数
| 参数 | 说明 | 默认值 |
|----------|-------------|--------|
| `-p` / `<path>` | `.pkg` 文件路径 | `./scene.pkg` |
| `-m` / `<modes>` | 显示模式：`wlr` 或 `winit` | `wlr` |
| `--fit-mode` | 壁纸适配：`cover`、`contain`、`stretch` | `cover` |
| `--no-effects` | 跳过后处理，渲染静态图像 | `false` |
| `-l` / `--log-level` | `verbose`、`debug`、`warning`、`errors` | `warning` |
| `-x` / `[output]` | 解压模式（可选择指定输出目录）| 停用 |
| `--parse-tex` | 将 `.tex` 纹理转换为 PNG（解压模式）| `false` |
| `--parse-video` | 解析视频/GIF 元数据（解压模式）| `false` |
| `--parse-mdl` | 将 `.mdl` 偶戏模型解析为 JSON（解压模式）| `false` |
| `--dry-run` | 显示将解压的文件，但不写入 | `false` |

## 项目结构

```
src/
├── main.rs                       # 命令行入口点，使用 clap 解析参数
├── pkg_parser/                   # （独立 crate）.pkg 文件解析器
│   └── src/pkg_parser/
│       ├── parser.rs             # .pkg 文件格式读取与解压
│       ├── tex_parser.rs         # .tex 纹理加载（LZ4、DXT、PNG、JPEG）
│       ├── video_parser.rs       # 视频/GIF 元数据解析与帧撷取
│       └── mdl_parser.rs         # MDL 偶戏模型解析与 JSON 导出
└── scene/
    ├── mod.rs                    # 模块声明
    ├── loader/
    │   ├── scene.rs              # scene.json 结构类型（Root、Camera、General 等）
    │   ├── scene_loader.rs       # 从 .pkg 加载场景（平行纹理解析）
    │   ├── object.rs             # 对象 JSON 结构，包含所有 WP Engine 属性
    │   ├── object_loader.rs      # 对象映射构建（纹理/音频/节点层级）
    │   └── model.rs              # 模型 JSON 结构
    ├── renderer/
    │   ├── app.rs                # WgpuApp：主要 GPU 状态与渲染循环
    │   ├── surface.rs            # 表面抽象层（原始句柄 + winit）
    │   ├── load.rs               # 资源加载与管线创建
    │   ├── buffer.rs             # 顶点/索引/投影 GPU 缓冲区
    │   ├── draw.rs               # DrawQueue 与 DrawObject 构建
    │   ├── vertex.rs             # 顶点类型与 NDC 顶点
    │   ├── projection.rs         # 正交摄像机投影
    │   ├── render_pass.rs        # 最终渲染通道与统一变量写入
    │   ├── intermediate_pass.rs  # 来回效果渲染通道
    │   ├── effect_bindgroup.rs   # 效果绑定群组构建
    │   ├── ping_pong.rs          # 来回纹理对管理
    │   ├── post_process.rs       # 后处理采样器与布局
    │   └── post_processor/       # GLSL→WGSL 着色器预处理
    │       ├── shader_header.rs  # 着色器包含头文件（common.h 等）
    │       ├── shader_headers/   # GLSL 包含文件（*.h）
    │       ├── effect_param.rs   # 统一变量布局与每帧参数填充
    │       ├── pipeline_handler.rs # 效果管线创建与缓存
    │       ├── pipeline_helpers.rs # 定义收集与绑定群组布局
    │       └── transform/        # 着色器源代码转换
    │           ├── layout.rs     # 从 GLSL 产生 EffectLayout
    │           ├── mod.rs        # 预处理管线（头文件注入、可变/属性翻译）
    │           └── replace.rs    # GLSL→GLSL 修正（mul、saturate、texSample2D 等）
    └── adapters/
        ├── mod.rs               # FitMode 枚举
        ├── winit_adapter.rs     # Winit 窗口适配器（置底窗口、光标追踪）
        └── wlr_app/
            ├── mod.rs           # wlr-layer-shell Wayland 适配器（Background 图层）
            └── scale.rs         # 分数缩放状态管理
```

## 已知问题与限制

* **视频播放：** 视频纹理（.tex 文件内的 mp4/webm）会被侦测但不会在运行时解码，将显示为静态帧。GIF 纹理可能部分可用。
* **动画：** `.mdl` 偶戏模型的骨骼动画已解析但未播放。
* **着色器兼容性：** 部分 Wallpaper Engine 着色器结构可能无法正确转译。GLSL→WGSL 预处理管线处理常见情况，但仍存在边缘案例。
* **Wayland 上的光标追踪：** wlr-layer-shell 适配器无法在 `Layer::Background` 表面上接收指针事件，因为 Wayland 的安全模型。深度视差效果在 wlr 模式下不可用。
* **macOS：** 未经测试。

## 开发路线图

- [x] 提升稳定性与错误处理
- [x] 实作音频支持
- [x] .mdl 文件解析
- [x] 着色器预处理与效果管线
- [x] wlr-layer-shell Wayland 适配器
- [x] 多种壁纸适配模式
- [x] .pkg 解压与文件解析工具
- [x] 纹理格式侦测与转换（DXT、R8、RG88、PNG、JPEG）
- [x] 后处理效果管线（来回多通道）
- [ ] 视频纹理播放
- [ ] 偶戏模型动画
- [ ] 配置文件支持
- [ ] 多屏幕支持

## 贡献

欢迎提交贡献！

## 授权

本项目采用 GPLv3 授权条款 — 详见 LICENSE 文件。

## 免责声明

本项目与 Wallpaper Engine 无任何关联或背书。请通过在 Steam 上购买来支持原始软件。

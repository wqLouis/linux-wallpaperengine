# Linux Wallpaper Engine

[![Rust](https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Wgpu](https://img.shields.io/badge/Wgpu-FF5A03?style=for-the-badge&logoColor=white)](https://github.com/gfx-rs/wgpu)
[![Vulkan](https://img.shields.io/badge/Vulkan-AC162C?style=for-the-badge&logo=vulkan&logoColor=white)](https://www.vulkan.org/)

> **⚠️ EXPERIMENTAL SOFTWARE**

This project is an attempt to bring [Wallpaper Engine](https://www.wallpaperengine.io/en) compatibility to Linux. It is written in Rust and utilizes `wgpu` with the Vulkan backend to render scenes.

Currently, this software is **highly unstable**, under heavy development, and almost certainly not working.

## Features


https://github.com/user-attachments/assets/3a2a6ed8-ddf5-4f0a-8c0d-80f8fcfc4af0


*   **Scene Parsing:** Reads and parses `.scene.pkg` files from Wallpaper Engine.
*   **Rendering:** Hardware-accelerated rendering using **wgpu** (Vulkan backend).
*   **Texture Support:** Handles texture loading with automatic alignment and format conversion.

## Requirements

*   **Rust** (Latest stable version)
*   **Vulkan Drivers:** Ensure your GPU drivers support Vulkan (Mesa for AMD/Intel, proprietary drivers for Nvidia).
*   **Wallpaper Engine Assets:** You must have legal access to the `.pkg` files (e.g., via a purchased copy of Wallpaper Engine on Steam).

## Installation

1.  Clone the repository:
    ```bash
    git clone https://github.com/wqLouis/linux-wallpaper-engine.git
    cd linux-wallpaper-engine
    ```

2.  Install the project:
    ```bash
    cargo install --path . --profile=release
    ```

## Usage

```bash
linux-wallpaper-engine {path to pkg file}
```

## Known Issues & Limitations

*   **Compatibility:** In some wallpapers, the software might fail to parse the texture files

## Roadmap

- [x] Improve stability and error handling
- [ ] Add support for more shader types
- [x] Implement audio support
- [ ] Animation support
- [ ] .mdl file loading

## Contributing

Contributions are welcome! However, please keep in mind that this is an experimental project.

## License

This project is licensed under the GPLv3 License - see the LICENSE file for details.

## Disclaimer

This project is not affiliated with or endorsed by Wallpaper Engine. Please support the original software by purchasing it on Steam.

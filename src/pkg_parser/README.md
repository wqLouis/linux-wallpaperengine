# Wallpaper Engine Pkg Parser

A high-performance Rust library and command-line tool for parsing Wallpaper Engine `.pkg` archives and `.tex` texture files.

## ✨ Features

* **No Runtime Dependencies**: Pure Rust implementation with static linking—no external runtimes required.
* **Compact Binaries**: About 6.6MB.
* **Robust Extraction**: Seamlessly extracts files from `.pkg` archives while preserving the original directory structure.
* **Texture Conversion**: Converts proprietary `.tex` texture files to standard `.png` format.
  * *Note: Texture conversion is very **Work In Progress (WIP)** it breaks idk *

## Installation

Build and install locally:

```bash
git clone https://github.com/wqLouis/depkg.git
cd wallpaper-engine-pkg-parser
cargo install --path .
```

## Acknowledgments & Inspiration

This project was inspired by the incredible work done by the open-source community. Special thanks to:

* **[notscuffed/repkg](https://github.com/notscuffed/repkg)**
* **[AzPepoze/linux-wallpaperengine](https://github.com/AzPepoze/linux-wallpaperengine)**

---

## ⚠️ Disclaimer

**Please respect the copyright of wallpaper creators.**

This tool is intended primarily to assist in bringing Wallpaper Engine content to Linux platforms and for educational/research purposes. We strictly condemn the use of this tool for piracy or unauthorized distribution of paid content.

# Wallpaper Engine `.pkg` File Format Documentation

This document describes the binary structure of the `.pkg` files used by Wallpaper Engine to package scene assets (models, textures, scripts, etc.).

## Overview

The `.pkg` format is a simple, index-based archive. It consists of a **Header**, a **File Table** (Directory), and a **Data Blob**.

*   **Endianness:** Little Endian
*   **String Encoding:** UTF-8
*   **Integer Sizes:** 32-bit (`u32`)
*   **Note:** Because offsets are `u32`, the theoretical maximum size of a `.pkg` file is 4 GB.

---

## File Structure

The file is laid out sequentially in three distinct sections.

```text
+------------------+ 
|  HEADER          |  <-- Starts at Byte 0
+------------------+
|  FILE TABLE      |  <-- Starts after Header
|  (Entry 1)       |
|  (Entry 2)       |
|  ...             |
+------------------+
|  DATA BLOB       |  <-- Starts at arbitrary offsets
|  [File Data]     |
|  [File Data]     |
+------------------+
```

---

## 1. Header

The header contains metadata required to parse the file table.

| Offset | Type | Size | Description |
|--------|------|------|-------------|
| `0x00` | `u32` | 4 | **Version String Length**. The number of bytes to read for the version string. |
| `0x04` | `char[]` | Variable | **Version String**. Typically `PKGV0022` (or similar). |
| `0x04+len` | `u32` | 4 | **File Count**. The total number of file entries in the table. |

**Example:**
If the version string is `PKGV0022` (8 bytes):
1. Read `u32` -> `8`
2. Read 8 bytes -> `PKGV0022`
3. Read `u32` -> Total file count (e.g., `167`)

---

## 2. File Table

The File Table is a flat array of entries. It contains no directory hierarchy; directories are implied by the file paths (e.g., `models/box.json`).

The table repeats the following structure for every file defined in **File Count**:

### Entry Structure

| Field | Type | Size | Description |
|-------|------|------|-------------|
| **Path Length** | `u32` | 4 | Length of the file path string. |
| **Path** | `char[]` | Variable | The relative path of the file within the package (e.g., `scene.json`, `materials/texture.tex`). |
| **Offset** | `u32` | 4 | The absolute byte offset in the file where the file's data begins. |
| **Size** | `u32` | 4 | The size of the file data in bytes. |

---

## 3. Data Blob

The data section contains the raw content of the files. There are no separators or padding bytes between files.

To read a file:
1. Locate its entry in the **File Table**.
2. Seek to the **Offset**.
3. Read **Size** bytes.

# `.tex` File Structure Specification

This document outlines the binary layout of the texture file format as defined by the parser logic in `parse()`.

## 1. Global Header
The file begins with a fixed-size header containing version information, format identifiers, and global image dimensions.

| Offset | Size (Bytes) | Type | Variable Name | Description |
| :--- | :--- | :--- | :--- | :--- |
| `0x00` | 8 | `[u8; 8]` | `texv` | Version Magic String (e.g., `TEXV0005`) |
| `0x08` | 1 | - | - | Separator / Padding |
| `0x09` | 8 | `[u8; 8]` | `texi` | Info Magic String |
| `0x11` | 1 | - | - | Separator / Padding |
| `0x12` | 4 | `u32` (LE) | `format` | Format ID (See [Format Table](#format-ids)) |
| `0x16` | 4 | - | - | Skip / Padding |
| `0x1A` | 4 | `u32` (LE) | `dimension[0]` | Target Image Width |
| `0x1E` | 4 | `u32` (LE) | `dimension[1]` | Target Image Height |
| `0x22` | 12 | - | - | Skip / Padding |
| `0x2E` | 8 | `[u8; 8]` | `texb` | Block Magic String (e.g., `TEXB0003`) |
| `0x36` | 1 | - | - | Separator / Padding |
| `0x37` | 4 | `u32` (LE) | `image_count` | Number of images in container |
| `0x3B` | 8 | - | - | Skip / Padding |
| `0x43` | 4 | `u32` (LE) | `mipmap_count` | Number of mipmaps |

---

## 2. Format IDs
The `format` field at offset `0x12` determines how the payload data is interpreted.

| ID Value | Name | Description |
| :--- | :--- | :--- |
| `0` | `raw` | Uncompressed embedded image (PNG/JPG). |
| `4` | `dxt1` | Compressed (DXT1). |
| `6` | `dxt5` | Compressed (DXT5). |
| `7` | `dxt1` | Compressed (DXT1 variant). |
| `8` | `rg88` | Uncompressed (RG88). |
| `9` | `r8` | Uncompressed (Grayscale/Mask). |

---

## 3. Payload Structure
The structure of the data following the header depends on the `format` ID.

### Case A: Format 0 (`raw`)
If the format is `0`, the parser treats the data as a standard image file (PNG or JPG) embedded directly.

| Offset | Size (Bytes) | Type | Description |
| :--- | :--- | :--- | :--- |
| `0x47` | 16 | - | Skip / Padding |
| `0x57` | 4 | `u32` (LE) | `size` | Size of the embedded image data |
| `0x5B` | `size` | `u8[]` | `payload` | Raw bytes of the PNG or JPG file |

**Notes:**
*   The parser determines the file type (PNG vs JPG) by inspecting the first few bytes of the `payload`.
*   It skips 16 bytes after `mipmap_count` before reading the data size.

### Case B: Other Formats (DXT, R8, RG88)
For formats `4`, `6`, `8`, and `9`, the payload is either raw pixel data or LZ4 compressed data.

| Offset | Size (Bytes) | Type       | Description                                          |                                                |
| :----- | :----------- | :--------- | :--------------------------------------------------- | ---------------------------------------------- |
| `0x47` | 8            | -          | Skip / Padding (Likely specific mipmap width/height) |                                                |
| `0x4F` | 4            | `u32` (LE) | `lz4`                                                | Compression Flag (`1` = Compressed, `0` = Raw) |
| `0x53` | 4            | `u32` (LE) | `dncompressed_size`                                  | Size of data after decompression               |
| `0x57` | 4            | `u32` (LE) | `size`                                               | Size of the data chunk in the file             |
| `0x5B` | `size`       | `u8[]`     | `payload`                                            | The pixel data (compressed or raw)             |

**Processing Logic:**
1.  Read `size` bytes from `0x5B`.
2.  If `lz4` == `1`: Decompress the data using LZ4 block decompression. The output buffer size must match `uncompressed_size`.
3.  If `lz4` == `0`: The data is already raw.
4.  Interpret the resulting bytes based on the Format ID (e.g., for `r8`, expand single bytes to RGBA pixels).
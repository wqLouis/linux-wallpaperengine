# Linux Wallpaper Engine — GNOME Shell extension

This extension displays the wallpapers rendered by the
[`linux-wallpaper-engine`](../../README.md) app as the GNOME desktop
background.

GNOME Shell does not implement the `wlr-layer-shell` protocol, so the app
cannot render into a compositor surface.  Instead the app renders the scene
into an off-screen texture, copies every frame into a shared file in
`/dev/shm` and hands the path of that file over to this extension, which
shows it in the background group of the shell.

---

## Requirements

* GNOME Shell 45 or newer (the extension uses ES modules)
* `linux-wallpaper-engine` built from this repository

## Installation

```bash
./install.sh
```

The script copies the extension into
`~/.local/share/gnome-shell/extensions/linux-wallpaper-engine@wqLouis.github.io`
and enables it.  Log out and back in afterwards (on X11 you can also press
<kbd>Alt</kbd>+<kbd>F2</kbd> and enter `r` to restart the shell).

Check that it is running:

```bash
gnome-extensions info linux-wallpaper-engine@wqLouis.github.io
```

## Usage

```bash
linux-wallpaper-engine -m gnome -p /path/to/wallpaper.pkg
```

The app waits for the extension to show up on the session bus.  If it is not
installed (or not enabled) it prints an error and exits.

Useful options:

| Option              | Description                                          |
|---------------------|------------------------------------------------------|
| `--fit-mode`       | `cover`, `contain` or `stretch`                    |
| `--target-fps`     | limit the frame rate (the shell throttles itself too) |
| `--no-effects`     | render a static image                                |

## Building with GNOME Builder

The folder is a ready to use GNOME Builder project:

```bash
gnome-builder gnome-extension/linux-wallpaper-engine.project
```

`src/extension.js` is the extension itself, `src/metadata.json` holds its
metadata.  GNOME Builder can also install the extension from the *Run*
dialog.

## How it works

| Step | Description |
|------|-------------|
| 1.   | The app connects to the session bus and waits for the extension to own `com.github.wqlouis.linux_wallpaper_engine`. |
| 2.   | The app calls `GetOutput` to learn the logical size and the scale factor of the primary monitor. |
| 3.   | The scene is rendered into an off-screen texture with the physical resolution of the monitor. |
| 4.   | The frame is copied back to the CPU, written into `/dev/shm/lwe-frame-<pid>-<n>.rgba` and pushed to the extension with `PushFrame`. |
| 5.   | The extension loads the image and displays it in `Main.layoutManager.backgroundGroup`, scaled to the monitor. |

### D-Bus interface

The extension owns the bus name
`com.github.wqlouis.linux_wallpaper_engine` and exports the interface
`com.github.wqlouis.linux_wallpaper_engine` on the object
`/com/github/wqlouis/linux_wallpaper_engine`.

| Member          | Direction | Description                                       |
|-----------------|-----------|---------------------------------------------------|
| `GetOutput`     | app → ext | returns `(width, height, scale)` of the output     |
| `PushFrame`     | app → ext | `(path, width, height)` of a shared RGBA frame     |
| `SetVisible`    | app → ext | show or hide the background                        |
| `OutputChanged` | ext → app | the output geometry changed                        |
| `CursorMoved`   | ext → app | the pointer position, normalised to `[0, 1]`         |

The frames are transferred through files instead of the bus, so the D-Bus
traffic stays small even for 4K frames.

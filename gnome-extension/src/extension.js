// Linux Wallpaper Engine — GNOME Shell extension.
//
// Displays the frames rendered by the "linux-wallpaper-engine" app as the
// desktop background.
//
// The app renders the scene off-screen (GNOME Shell does not implement the
// wlr-layer-shell protocol) and writes every frame into a shared file in
// /dev/shm.  It then calls the PushFrame method of this extension with the
// path of that file; the extension loads the image and displays it in the
// background group of the shell.
//
// Run the app with:  linux-wallpaper-engine -m gnome -p wallpaper.pkg

import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import St from 'gi://St';
import Clutter from 'gi://Clutter';

import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';

const BUS_NAME = 'com.github.wqlouis.linux_wallpaper_engine';
const OBJECT_PATH = '/com/github/wqlouis/linux_wallpaper_engine';
const INTERFACE_NAME = 'com.github.wqlouis.linux_wallpaper_engine';

const IFACE_XML = `
<node>
  <interface name="${INTERFACE_NAME}">
    <method name="GetOutput">
      <arg type="u" name="width" direction="out"/>
      <arg type="u" name="height" direction="out"/>
      <arg type="u" name="scale" direction="out"/>
    </method>
    <method name="PushFrame">
      <arg type="s" name="path" direction="in"/>
      <arg type="u" name="width" direction="in"/>
      <arg type="u" name="height" direction="in"/>
    </method>
    <method name="SetVisible">
      <arg type="b" name="visible" direction="in"/>
    </method>
    <signal name="OutputChanged">
      <arg type="u" name="width"/>
      <arg type="u" name="height"/>
      <arg type="u" name="scale"/>
    </signal>
    <signal name="CursorMoved">
      <arg type="d" name="x"/>
      <arg type="d" name="y"/>
    </signal>
  </interface>
</node>`;

const logError = typeof globalThis.logError === 'function'
    ? globalThis.logError
    : (error, message) => console.error(message, error);

export default class LinuxWallpaperEngineExtension extends Extension {
    enable() {
        this._actors = [];
        this._textures = [];
        this._textureApi = this._detectTextureApi();
        this._frameWidth = 0;
        this._frameHeight = 0;
        this._lastCursor = [-1, -1];
        this._lastFrameTime = 0;

        if (!this._textureApi) {
            logError(null, 'linux-wallpaper-engine: no usable texture API found');
            return;
        }

        // Export the D-Bus interface the app talks to.
        this._dbus = Gio.DBusExportedObject.wrapJSObject(IFACE_XML, this);
        this._dbus.export(Gio.DBus.session, OBJECT_PATH);

        // Own the well known name so the app can find us.
        this._nameId = Gio.bus_own_name(
            Gio.BusType.SESSION,
            BUS_NAME,
            Gio.BusNameOwnerFlags.NONE,
            () => {},
            null,
            (connection, name) => {
                logError(null, `linux-wallpaper-engine: cannot own the bus name ${name}`);
            });

        this._monitorsId = Main.layoutManager.connect('monitors-changed', () => {
            this._onMonitorsChanged();
        });

        // The pointer position is polled and sent to the app, which uses it
        // for the depth parallax effects.
        this._pointerId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 50, () => {
            this._pollPointer();
            return GLib.SOURCE_CONTINUE;
        });

        this._syncActors();
    }

    disable() {
        if (this._pointerId) {
            GLib.source_remove(this._pointerId);
            this._pointerId = 0;
        }
        if (this._monitorsId) {
            Main.layoutManager.disconnect(this._monitorsId);
            this._monitorsId = 0;
        }

        for (const actor of this._actors)
            actor.destroy();
        this._actors = [];
        this._textures = [];

        if (this._dbus) {
            this._dbus.unexport();
            this._dbus = null;
        }
        if (this._nameId) {
            Gio.bus_unown_name(this._nameId);
            this._nameId = 0;
        }
    }

    // ── D-Bus methods (called by the app) ────────────────────────────

    GetOutputAsync(params, invocation) {
        const [width, height, scale] = this._outputGeometry();
        invocation.return_value(new GLib.Variant('(uuu)', [width, height, scale]));
    }

    PushFrameAsync(params, invocation) {
        const [path, width, height] = params;

        try {
            this._loadFrame(path, width, height);
            invocation.return_value(null);
        } catch (error) {
            logError(error, 'linux-wallpaper-engine: cannot load the frame');
            invocation.return_dbus_error(
                `${INTERFACE_NAME}.Error`,
                `${error.message ?? error}`);
        }
    }

    SetVisibleAsync(params, invocation) {
        const [visible] = params;

        for (const actor of this._actors)
            actor.visible = visible;
        invocation.return_value(null);
    }

    // ── Frame handling ───────────────────────────────────────────────

    _loadFrame(path, width, height) {
        const file = Gio.File.new_for_path(path);
        const [ok, contents] = file.load_contents(null);
        if (!ok)
            throw new Error(`cannot read the frame '${path}'`);

        const bytes = new Uint8Array(contents);

        if (!this._actors.length)
            this._syncActors();
        if (!this._actors.length)
            throw new Error('no monitor found');

        for (const texture of this._textures)
            this._setTextureData(texture, bytes, width, height);

        this._frameWidth = width;
        this._frameHeight = height;
        this._lastFrameTime = GLib.get_monotonic_time();
        this._layout();
    }

    /**
     * Hand the raw RGBA frame to a texture.
     *
     * `St.Texture` is preferred, `Clutter.Texture` is the fallback for
     * shells that do not expose `st_texture_set_data` anymore.
     */
    _setTextureData(texture, bytes, width, height) {
        try {
            if (typeof texture.set_data === 'function') {
                texture.set_data(bytes, width, height, width * 4);
                return;
            }
            if (typeof texture.set_bytes === 'function') {
                texture.set_bytes(bytes, width, height);
                return;
            }
        } catch (error) {
            logError(error, 'linux-wallpaper-engine: cannot update the texture');
        }

        throw new Error('no usable texture API found');
    }

    // ── Actors ───────────────────────────────────────────────────────

    /** Create one texture actor per monitor. */
    _syncActors() {
        const group = Main.layoutManager.backgroundGroup;
        if (!group) {
            logError(null, 'linux-wallpaper-engine: no background group found');
            return;
        }

        const monitors = Main.layoutManager.monitors;

        while (this._actors.length > monitors.length) {
            const actor = this._actors.pop();
            const index = this._textures.indexOf(actor);
            if (index >= 0)
                this._textures.splice(index, 1);
            actor.destroy();
        }

        while (this._actors.length < monitors.length) {
            const actor = this._createTextureActor();
            group.add_child(actor);
            // Keep the actor on top of the regular wallpaper.
            group.set_child_above_sibling(actor, null);
            this._actors.push(actor);
        }

        this._layout();
    }

    _createTextureActor() {
        let texture;

        if (this._textureApi === 'st') {
            texture = new St.Texture({
                reactive: false,
                track_hover: false,
                can_focus: false,
            });
        } else {
            texture = new Clutter.Texture();
            texture.reactive = false;
            texture.track_hover = false;
        }

        this._textures.push(texture);
        return texture;
    }

    /** Place the actors on their monitors. */
    _layout() {
        const monitors = Main.layoutManager.monitors;

        for (let i = 0; i < this._actors.length; i++) {
            const actor = this._actors[i];
            const monitor = monitors[i];
            if (!monitor)
                continue;

            actor.set_position(monitor.x, monitor.y);

            // The frame is rendered with the physical resolution of the
            // monitor, so it has to be scaled down to the logical size.
            if (this._frameWidth > 0 && this._frameHeight > 0) {
                actor.set_scale(
                    monitor.width / this._frameWidth,
                    monitor.height / this._frameHeight);
            }
        }
    }

    // ── Output geometry ──────────────────────────────────────────────

    _outputGeometry() {
        const monitors = Main.layoutManager.monitors;
        const index = Main.layoutManager.primaryIndex ?? 0;
        const monitor = monitors[index] ?? monitors[0];

        if (!monitor)
            return [1920, 1080, 1];

        return [
            Math.max(1, monitor.width),
            Math.max(1, monitor.height),
            this._monitorScale(monitor),
        ];
    }

    _monitorScale(monitor) {
        let scale = monitor.geometryScale ?? monitor.geometry_scale;

        if (!scale) {
            const context = global.context;
            scale = context && context.get_scale_factor
                ? context.get_scale_factor()
                : 1;
        }

        return Math.max(1, scale);
    }

    _onMonitorsChanged() {
        this._syncActors();

        const [width, height, scale] = this._outputGeometry();
        this._dbus?.emit_signal('OutputChanged', new GLib.Variant('(uuu)', [width, height, scale]));
    }

    // ── Pointer ──────────────────────────────────────────────────────

    _pollPointer() {
        // The pointer is only interesting while the app is pushing frames.
        if (GLib.get_monotonic_time() - this._lastFrameTime > 2 * GLib.SECOND)
            return;

        const pointer = global.get_pointer();
        if (!pointer)
            return;

        const [x, y] = pointer;
        const monitors = Main.layoutManager.monitors;

        let monitor = null;
        for (const candidate of monitors) {
            if (x >= candidate.x && x < candidate.x + candidate.width &&
                y >= candidate.y && y < candidate.y + candidate.height) {
                monitor = candidate;
                break;
            }
        }
        monitor = monitor ?? monitors[Main.layoutManager.primaryIndex ?? 0];
        if (!monitor)
            return;

        const nx = (x - monitor.x) / monitor.width;
        const ny = (y - monitor.y) / monitor.height;

        if (Math.abs(nx - this._lastCursor[0]) < 0.001 &&
            Math.abs(ny - this._lastCursor[1]) < 0.001)
            return;

        this._lastCursor = [nx, ny];
        this._dbus?.emit_signal('CursorMoved', new GLib.Variant('(dd)', [nx, ny]));
    }

    // ── Helpers ──────────────────────────────────────────────────────

    /** Find a texture implementation that can display raw RGBA frames. */
    _detectTextureApi() {
        if (typeof St?.Texture === 'function' &&
            typeof St.Texture.prototype.set_data === 'function')
            return 'st';

        if (typeof Clutter?.Texture === 'function' &&
            typeof Clutter.Texture.prototype.set_bytes === 'function')
            return 'clutter';

        return null;
    }
}

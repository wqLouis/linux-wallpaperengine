// Linux Wallpaper Engine — macOS desktop window.
//
// Creates a borderless AppKit window that lives in the desktop layer of the
// window server and hosts a CAMetalLayer.  The window is placed above the
// desktop picture and below the desktop icons, so it behaves like a live
// wallpaper.
//
// The file is compiled by `build.rs` and called from Rust through a small
// C ABI (see `src/scene/adapters/mac_app/mod.rs`).

import AppKit
import Metal
import QuartzCore

/// The window that shows the wallpaper.
private var desktopWindow: NSWindow?

/// The view that hosts the Metal layer.
///
/// wgpu renders into the layer of the view, so the view has to be backed by a
/// `CAMetalLayer`.
private final class MetalHostView: NSView {
    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)

        wantsLayer = true

        if let device = MTLCreateSystemDefaultDevice() {
            let layer = CAMetalLayer()
            layer.device = device
            layer.pixelFormat = .bgra8Unorm
            layer.framebufferOnly = true
            self.layer = layer
        }
    }

    required init?(coder: NSCoder) {
        fatalError("MacDesktop does not support NSCoding")
    }
}

/// The size of the primary screen (in points) and its scale factor.
@_cdecl("lwe_mac_screen_size")
public func lwe_mac_screen_size(_ width: UnsafeMutablePointer<Double>,
                                _ height: UnsafeMutablePointer<Double>,
                                _ scale: UnsafeMutablePointer<Double>) {
    let screen = NSScreen.screens.first

    width.pointee = Double(screen?.frame.width ?? 0.0)
    height.pointee = Double(screen?.frame.height ?? 0.0)
    scale.pointee = Double(screen?.backingScaleFactor ?? 1.0)
}

/// Create the desktop window and return the view that hosts the Metal layer.
///
/// Must be called from the main thread.
@_cdecl("lwe_mac_create_window")
public func lwe_mac_create_window(_ width: Double, _ height: Double) -> UnsafeMutableRawPointer? {
    // A wallpaper has no user interface of its own: no Dock icon, no menu bar.
    let app = NSApplication.shared
    app.setActivationPolicy(.accessory)
    app.finishLaunching()

    let screen = NSScreen.screens.first
    let frame = screen?.frame ?? NSRect(x: 0, y: 0, width: width, height: height)

    let view = MetalHostView(frame: NSRect(origin: .zero, size: frame.size))

    let window = NSWindow(contentRect: frame,
                          styleMask: [.borderless],
                          backing: .buffered,
                          defer: false)
    window.contentView = view
    window.isOpaque = true
    window.backgroundColor = .black
    window.hasShadow = false
    // The wallpaper must never steal the pointer.
    window.ignoresMouseEvents = true
    // kCGDesktopWindowLevel (20): above the desktop picture, below the
    // desktop icons (kCGDesktopIconWindowLevel, 25) and the Dock.
    window.level = 20
    window.collectionBehavior = [.canJoinAllSpaces,
                                 .stationary,
                                 .ignoresCycle,
                                 .fullScreenAuxiliary]
    window.setFrame(frame, display: true)

    // The layer draws in device pixels, so it has to know the scale factor.
    view.layer?.contentsScale = screen?.backingScaleFactor ?? 1.0

    // Send the window to the back of its level, so the Dock and the desktop
    // icons stay in front of it.
    window.orderBack(nil)

    desktopWindow = window

    // The window keeps the view alive.
    return Unmanaged.passUnretained(view).toOpaque()
}

/// Process the pending AppKit events.
///
/// The Rust render loop is not a `NSApplication` run loop, so the events
/// have to be pumped by hand.
@_cdecl("lwe_mac_pump_events")
public func lwe_mac_pump_events() {
    let app = NSApplication.shared

    while let event = app.nextEvent(matching: .any,
                                    until: Date(),
                                    inMode: .default,
                                    dequeue: true) {
        app.sendEvent(event)
    }
}

/// Close the desktop window again.
@_cdecl("lwe_mac_destroy_window")
public func lwe_mac_destroy_window() {
    desktopWindow?.orderOut(nil)
    desktopWindow?.contentView = nil
    desktopWindow = nil
}

//! The system tray icon.
//!
//! The GUI lives in the system tray: the icon is a StatusNotifierItem, a
//! D-Bus service that the desktop shows next to the clock.  The
//! implementation is [ksni], a pure-Rust one, so no GTK (and no GLib main
//! loop) has to be integrated with gpui.
//!
//! The tray runs on a thread of its own, with a runtime of its own: gpui
//! owns the main thread, and the two never share a runtime.  The tray
//! thread never touches a gpui object: it sends the actions of the user to
//! the application over a channel, and the application sends the state of
//! the wallpaper back the other way around.

use async_channel::{Receiver, Sender};
use ksni::TrayMethods;

/// What the tray asks the application to do.
#[derive(Clone, Debug)]
pub enum TrayAction {
    /// The user clicked the icon: the picker is opened.
    Activate {
        /// The XDG activation token of the click.
        ///
        /// Wayland only lets a window be raised when the request carries a
        /// token, so the token is taken while the click is being handled.
        token: Option<String>,
    },
    /// The user chose "Wallpapers…".
    OpenPicker,
    /// The user chose "Pause" or "Resume".
    TogglePause,
    /// The user chose "Quit".
    Quit,
}

/// What the application tells the tray.
#[derive(Clone, Debug)]
pub enum TrayCommand {
    /// The state of the wallpaper changed: the menu is refreshed.
    State(TrayState),
}

/// What the tray shows of the wallpaper.
#[derive(Clone, Debug, Default)]
pub struct TrayState {
    /// The title of the wallpaper that is running.
    pub title: Option<String>,
    /// Whether the wallpaper is paused.
    pub paused: bool,
}

/// The tray icon itself.
///
/// The tray is the only place that owns the D-Bus service: the
/// application reaches it through the channel, never directly.
struct WallpaperTray {
    /// The channel to the application.
    actions: Sender<TrayAction>,
    /// What the tray shows of the wallpaper.
    state: TrayState,
}

impl ksni::Tray for WallpaperTray {
    fn id(&self) -> String {
        // The id has to be the same in every session: the desktop uses it
        // to remember the position of the icon.
        "lwe-gui".into()
    }

    fn title(&self) -> String {
        "Linux Wallpaper Engine".into()
    }

    /// A themed icon: the desktop looks the name up in its icon theme, so
    /// no picture has to be shipped with the application.
    fn icon_name(&self) -> String {
        "preferences-desktop-wallpaper".into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Linux Wallpaper Engine".into(),
            description: self.status_line(),
            ..Default::default()
        }
    }

    /// The user clicked the icon.
    fn activate(&mut self, _x: i32, _y: i32) {
        // The activation token is read from the event that is being
        // handled right now: it cannot be asked for later.
        let token = ksni::current_xdg_activation_token();

        // `send` is asynchronous: dropping the future would throw the
        // message away.  `try_send` is the one that delivers it.
        let _ = self.actions.try_send(TrayAction::Activate { token });
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;

        vec![
            StandardItem {
                label: self.status_line(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Wallpapers…".into(),
                icon_name: "folder-open".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.actions.try_send(TrayAction::OpenPicker);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: if self.state.paused { "Resume" } else { "Pause" }.into(),
                icon_name: if self.state.paused {
                    "media-playback-start"
                } else {
                    "media-playback-pause"
                }
                .into(),
                // There is nothing to pause when no wallpaper is running.
                enabled: self.state.title.is_some(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.actions.try_send(TrayAction::TogglePause);
                }),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.actions.try_send(TrayAction::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

impl WallpaperTray {
    /// The line of the menu that tells what the application is doing.
    fn status_line(&self) -> String {
        match (&self.state.title, self.state.paused) {
            (Some(title), true) => format!("{title} (paused)"),
            (Some(title), false) => title.clone(),
            (None, _) => "No wallpaper".into(),
        }
    }
}

/// Runs the tray on a thread of its own.
///
/// The tray is a D-Bus service and needs a runtime of its own: the gpui
/// application owns the main thread, and the two runtimes never meet.
///
/// The function returns when the channel to the application is closed.
pub fn run(actions: Sender<TrayAction>, commands: Receiver<TrayCommand>) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            log::error!("the tray has no runtime: {error}");
            return;
        }
    };

    runtime.block_on(async move {
        let tray = WallpaperTray {
            actions,
            state: TrayState::default(),
        };

        // The item is registered even when the desktop has no tray yet:
        // the watcher may show up later, and a tray application that
        // gives up is an application nobody can reach.
        let handle = match tray.assume_sni_available(true).spawn().await {
            Ok(handle) => {
                log::info!("the tray icon is registered");
                handle
            }
            Err(error) => {
                log::error!("the tray icon could not be registered: {error}");
                return;
            }
        };

        // The application owns the state of the wallpaper: every change
        // of it is applied here, and the menu is published again.
        while let Ok(command) = commands.recv().await {
            match command {
                TrayCommand::State(state) => {
                    handle
                        .update(|tray: &mut WallpaperTray| tray.state = state)
                        .await;
                }
            }
        }

        log::info!("the tray is closed");
    });
}

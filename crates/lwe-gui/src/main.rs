//! The native GUI of the Linux Wallpaper Engine.
//!
//! The GUI is a tray application: an icon in the system tray opens a
//! picker that lists the wallpapers of the workshop folder, and the
//! wallpaper the user picks is rendered by the engine — in this process,
//! on a worker thread of its own.
//!
//! The threads of the application:
//!
//! ```text
//!  main thread                          background threads
//!  -----------------                    -------------------
//!  gpui_platform::application()          "tray": the StatusNotifierItem
//!    ├─ the picker window                   └─ a tokio runtime of its own
//!    ├─ the handle of the wallpaper
//!    └─ the tasks that wait for the tray,
//!       for the scanner and for the
//!       statuses of the wallpaper
//! ```
//!
//! Nothing is swallowed: a failure of the engine is shown to the user in
//! a modal dialog, and the problems of a wallpaper — the files that could
//! not be used — are shown as well.

mod config;
mod design;
mod picker;
mod thumbs;
mod tray;

use std::path::PathBuf;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use gpui::{AnyWindowHandle, App, AppContext, AsyncApp, BorrowAppContext, Global};
use linux_wallpaper_engine::engine::{
    EngineConfig, EngineStatus, WallpaperHandle, WallpaperSource, scan_wallpapers,
};

use config::{FitMode, GuiConfig, MAX_FPS};
use picker::{Picker, PickerState, WallpaperState};

/// The state the whole application shares.
///
/// The state is a global of the gpui application: every part of the GUI
/// reaches it, and the parts of it that need a redraw are notified.
pub struct AppState {
    /// The settings of the user.
    pub config: GuiConfig,
    /// What the picker shows.
    pub picker: PickerState,
    /// The view of the picker: the window shows it.
    pub view: gpui::Entity<Picker>,
    /// The wallpaper that is running.
    wallpaper: Option<WallpaperHandle>,
    /// The window of the picker.
    window: Option<AnyWindowHandle>,
    /// The channel to the tray thread.
    tray: Sender<tray::TrayCommand>,
}

impl Global for AppState {}

impl AppState {
    /// The configuration the engine is started with.
    fn engine_config(&self) -> EngineConfig {
        EngineConfig {
            fit_mode: self.config.fit_mode.into(),
            target_fps: self.config.target_fps,
            no_effects: !self.config.effects,
            no_mdl: !self.config.puppets,
            assets_path: self.config.assets_path.clone(),
            show_progress: false,
        }
    }

    /// Writes the settings of the user.
    fn save_config(&self) {
        if let Err(error) = self.config.save() {
            log::warn!("the settings could not be written: {error}");
        }
    }

    /// Shows a wallpaper: the first one is started, the next ones replace
    /// it.
    pub fn choose(&mut self, row: picker::Row, cx: &mut App) {
        // A wallpaper that is already running is replaced by a command:
        // the renderer loads the new one, the window does not wait for it.
        if let Some(handle) = &self.wallpaper {
            match handle.switch(row.source.clone()) {
                Ok(()) => {
                    self.picker.select(&row);
                    self.picker.set_running(WallpaperState::Running);
                    self.sync_tray();
                    cx.notify(self.view.entity_id());
                    return;
                }
                Err(error) => {
                    // The renderer is gone: the wallpaper is stopped and
                    // a new one is started below.
                    log::warn!("the wallpaper could not be replaced: {}", error.trace());
                    self.wallpaper = None;
                }
            }
        }

        // The first wallpaper takes a while to come up: it is started on
        // a thread of its own, so the window stays responsive.
        self.picker.select(&row);
        self.start_wallpaper(row.source.clone(), cx);
    }

    /// Starts the wallpaper with the configuration of the user.
    ///
    /// The engine starts the renderer on a worker thread of its own and
    /// waits for the wallpaper to come up, so the window stays
    /// responsive.
    fn start_wallpaper(&mut self, source: WallpaperSource, cx: &mut App) {
        self.picker.set_running(WallpaperState::Starting);
        self.sync_tray();
        cx.notify(self.view.entity_id());

        let config = self.engine_config();
        let (sender, receiver) = async_channel::bounded(1);

        let thread = std::thread::Builder::new()
            .name("wallpaper-start".into())
            .spawn(move || {
                let _ = sender.try_send(WallpaperHandle::spawn(config, source));
            });

        if let Err(error) = thread {
            log::error!("the wallpaper could not be started: {error}");
            self.picker.set_running(WallpaperState::Stopped);
            cx.notify(self.view.entity_id());
            return;
        }

        cx.spawn(async move |cx| {
            match receiver.recv().await {
                Ok(Ok(handle)) => {
                    cx.update_global(|state: &mut AppState, cx| {
                        state.wallpaper = Some(handle);
                        state.picker.set_running(WallpaperState::Running);
                        state.sync_tray();
                        cx.notify(state.view.entity_id());
                    });
                }
                Ok(Err(error)) => {
                    let trace = error.trace();
                    cx.update_global(|state: &mut AppState, cx| {
                        state.picker.set_running(WallpaperState::Stopped);
                        cx.notify(state.view.entity_id());
                    });
                    show_failure(cx, "The wallpaper could not be loaded", &trace).await;
                }
                // The thread that starts the wallpaper is gone.
                Err(_) => {}
            }
        })
        .detach();
    }

    /// The wallpaper that is running is started again with the
    /// configuration of the user.
    ///
    /// The settings that the renderer reads when the wallpaper comes up
    /// need a renderer that is started again: the same wallpaper is shown
    /// again, not another one.
    fn restart_wallpaper(&mut self, cx: &mut App) {
        let Some(source) = self.current_source() else {
            return;
        };

        // The handle stops the renderer and waits for it: the wallpaper
        // does not outlive the application.
        self.wallpaper = None;

        self.start_wallpaper(source, cx);
    }

    /// Where the wallpaper that is running lives.
    fn current_source(&self) -> Option<WallpaperSource> {
        let current = self.picker.current.as_ref()?;
        self.picker
            .rows
            .iter()
            .find(|row| &row.id == current)
            .map(|row| row.source.clone())
    }

    /// The way the wallpaper is fitted to the screen changed.
    pub fn set_fit_mode(&mut self, mode: FitMode, cx: &mut App) {
        if self.config.fit_mode == mode {
            return;
        }

        self.config.fit_mode = mode;
        self.save_config();

        self.restart_wallpaper(cx);
    }

    /// The frame rate changed: the engine takes it as a command, so it is
    /// applied at once.
    pub fn set_fps(&mut self, value: u32, cx: &mut App) {
        let fps = if value >= MAX_FPS { None } else { Some(value) };

        if self.config.target_fps == fps {
            return;
        }

        self.config.target_fps = fps;
        self.save_config();

        if let Some(handle) = &self.wallpaper
            && let Err(error) = handle.set_fps(fps)
        {
            log::warn!("the frame rate could not be changed: {}", error.trace());
        }

        cx.notify(self.view.entity_id());
    }

    /// The effects of the scene are rendered, or not.
    pub fn set_effects(&mut self, on: bool, cx: &mut App) {
        if self.config.effects == on {
            return;
        }

        self.config.effects = on;
        self.save_config();

        self.restart_wallpaper(cx);
    }

    /// The puppet models of the scene are rendered, or not.
    pub fn set_puppets(&mut self, on: bool, cx: &mut App) {
        if self.config.puppets == on {
            return;
        }

        self.config.puppets = on;
        self.save_config();

        self.restart_wallpaper(cx);
    }

    /// Pauses the wallpaper, or renders it again.
    pub fn toggle_pause(&mut self, cx: &mut App) {
        let Some(handle) = &self.wallpaper else {
            // Nothing is running: there is nothing to pause.
            return;
        };

        let paused = self.picker.running == WallpaperState::Paused;
        let result = if paused {
            handle.resume()
        } else {
            handle.pause()
        };

        match result {
            Ok(()) => {
                self.picker.set_running(if paused {
                    WallpaperState::Running
                } else {
                    WallpaperState::Paused
                });
                self.sync_tray();
                cx.notify(self.view.entity_id());
            }
            Err(error) => {
                let trace = error.trace();
                cx.spawn(async move |cx| {
                    show_failure(cx, "The wallpaper could not be paused", &trace).await;
                })
                .detach();
            }
        }
    }

    /// Stops the wallpaper.
    fn stop(&mut self) {
        // The handle stops the renderer and waits for it: the wallpaper
        // does not outlive the application.
        self.wallpaper = None;
    }

    /// Tells the tray what the wallpaper is doing.
    fn sync_tray(&self) {
        let state = tray::TrayState {
            title: self.picker.title.clone(),
            paused: self.picker.running == WallpaperState::Paused,
        };

        // `try_send` is the one that delivers: `send` is asynchronous and
        // would throw the message away.
        let _ = self.tray.try_send(tray::TrayCommand::State(state));
    }

    /// The statuses of the wallpaper, if there are any.
    fn poll(&self) -> Vec<EngineStatus> {
        let mut statuses = Vec::new();

        let Some(handle) = &self.wallpaper else {
            return statuses;
        };

        while let Some(status) = handle.poll_status() {
            statuses.push(status);
        }

        statuses
    }
}

/// The entry point of the application.
fn main() {
    // The log is for the developer: the user is told about the failures
    // in a dialog.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // The channels between the tray thread and the gpui thread.
    let (tray_actions, tray_actions_rx) = async_channel::unbounded::<tray::TrayAction>();
    let (tray_commands, tray_commands_rx) = async_channel::unbounded::<tray::TrayCommand>();

    // The tray lives on a thread of its own: gpui owns the main thread,
    // and the two runtimes never meet.
    if let Err(error) = std::thread::Builder::new()
        .name("tray".into())
        .spawn(move || tray::run(tray_actions, tray_commands_rx))
    {
        // A tray that does not start is not a reason to refuse to start:
        // the application still runs, it just cannot be reached from the
        // tray.
        log::error!("the tray thread could not be started: {error}");
    }

    // The gpui application owns the main thread.
    gpui_platform::application()
        // The picker window may be closed: the tray keeps the application
        // alive.
        .with_quit_mode(gpui::QuitMode::Explicit)
        .run(|cx: &mut App| start(cx, tray_actions_rx, tray_commands));
}

/// Starts the application.
fn start(cx: &mut App, actions: Receiver<tray::TrayAction>, commands: Sender<tray::TrayCommand>) {
    // The picker is a view of the state: the state keeps it alive, so the
    // window can be closed and opened again.
    let view = cx.new(|_cx| Picker::default());

    let config = GuiConfig::load();
    let folder = config.workshop_dir.clone();

    cx.set_global(AppState {
        config,
        picker: PickerState::default(),
        view,
        wallpaper: None,
        window: None,
        tray: commands,
    });

    // The actions of the tray are handled on the gpui thread.
    cx.spawn(async move |cx| {
        while let Ok(action) = actions.recv().await {
            handle_action(cx, action).await;
        }
    })
    .detach();

    // The statuses of the wallpaper are read every now and then: the
    // engine reports them as they come.
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;

            let statuses = cx.read_global::<AppState, _>(|state, _| state.poll());
            for status in statuses {
                handle_status(cx, status).await;
            }
        }
    })
    .detach();

    // The first start asks for the workshop folder; afterwards the folder
    // of the user is scanned right away.
    //
    // The application is borrowed while it starts, so nothing here may
    // borrow it again: the helpers take the application itself.
    match folder {
        Some(folder) => scan(cx, folder),
        None => {
            open_picker(cx);
        }
    }
}

/// Handles an action of the tray.
async fn handle_action(cx: &mut AsyncApp, action: tray::TrayAction) {
    match action {
        tray::TrayAction::Activate { token } => {
            // The window is raised with the token of the click: on Wayland
            // nothing happens without one.
            let Some(window) = cx.update(open_picker) else {
                return;
            };

            let _ = window.update(cx, |_view, window, _cx| {
                let raised = match token.as_deref() {
                    Some(token) => window.activate_with_token(token),
                    None => false,
                };

                if !raised {
                    window.activate();
                }
            });
        }
        tray::TrayAction::OpenPicker => {
            cx.update(|cx| {
                open_picker(cx);
            });
        }
        tray::TrayAction::TogglePause => {
            cx.update_global(|state: &mut AppState, cx| state.toggle_pause(cx));
        }
        tray::TrayAction::Quit => {
            cx.update(quit);
        }
    }
}

/// Handles a status of the wallpaper.
async fn handle_status(cx: &mut AsyncApp, status: EngineStatus) {
    match status {
        EngineStatus::Starting => {}
        EngineStatus::Running { title, problems } => {
            let problems: Vec<String> =
                problems.iter().map(|problem| problem.to_string()).collect();

            cx.update_global(|state: &mut AppState, cx| {
                if let Some(title) = title {
                    state.picker.title = Some(title);
                }
                state.picker.set_running(WallpaperState::Running);
                state.picker.add_problems(problems.clone());
                state.sync_tray();
                cx.notify(state.view.entity_id());
            });

            // The files that could not be used are shown to the user: a
            // wallpaper that plays with a broken file is not a wallpaper
            // the user asked for.
            if !problems.is_empty() {
                let details = problems.join("\n");
                // The dialog is shown in a task of its own: the statuses
                // of the wallpaper are read while it is open.
                cx.spawn(async move |cx| {
                    show_failure(
                        cx,
                        "Some files of the wallpaper could not be used",
                        &details,
                    )
                    .await;
                })
                .detach();
            }
        }
        EngineStatus::Failed(error) => {
            // The wallpaper that was running keeps running when the
            // wallpaper that was to replace it could not be loaded: only
            // the failure is reported.
            let trace = error.trace();
            // The dialog is shown in a task of its own: the statuses of
            // the wallpaper are read while it is open.
            cx.spawn(async move |cx| {
                show_failure(cx, "The wallpaper could not be loaded", &trace).await;
            })
            .detach();
        }
        EngineStatus::Stopped => {
            cx.update_global(|state: &mut AppState, cx| {
                state.picker.set_running(WallpaperState::Stopped);
                state.sync_tray();
                cx.notify(state.view.entity_id());
            });
        }
    }
}

/// Remembers the folder of the user and scans it.
pub fn choose_folder(cx: &mut App, folder: PathBuf) {
    cx.update_global(|state: &mut AppState, cx| {
        state.config.workshop_dir = Some(folder.clone());
        state.save_config();

        cx.notify(state.view.entity_id());
    });

    scan(cx, folder);
}

/// Remembers the folder of the assets of Wallpaper Engine.
///
/// The renderer has to be started again: it reads the folder when the
/// wallpaper comes up.
pub fn choose_assets(cx: &mut App, folder: PathBuf) {
    cx.update_global(|state: &mut AppState, cx| {
        state.config.assets_path = Some(folder);
        state.save_config();

        cx.notify(state.view.entity_id());
        state.restart_wallpaper(cx);
    });
}

/// Scans a folder and shows the wallpapers it holds.
///
/// The scan reads the manifest of every wallpaper of the folder: it
/// happens on a background thread, so the window stays responsive.
pub fn scan(cx: &mut App, folder: PathBuf) {
    cx.update_global(|state: &mut AppState, cx| {
        state.picker.begin_scan(folder.clone());
        cx.notify(state.view.entity_id());
    });

    let scan = cx.background_spawn(async move {
        let wallpapers = scan_wallpapers(&folder);

        wallpapers
            .iter()
            .map(|wallpaper| picker::Row {
                id: wallpaper.id.clone(),
                title: wallpaper.title.clone(),
                kind: wallpaper.kind.clone(),
                // The preview is written to the cache: gpui shows an image
                // from a file.
                preview: wallpaper.thumbnail.as_ref().and_then(|thumbnail| {
                    thumbs::write_thumbnail(&wallpaper.id, wallpaper.source.path(), thumbnail)
                }),
                source: wallpaper.source.clone(),
            })
            .collect::<Vec<_>>()
    });

    cx.spawn(async move |cx| {
        let rows = scan.await;

        cx.update_global(|state: &mut AppState, cx| {
            state.picker.finish_scan(rows);
            cx.notify(state.view.entity_id());
        });
    })
    .detach();
}

/// Opens the picker window.
///
/// The window that is already there is only shown again; a window that was
/// closed is opened anew.
pub fn open_picker(cx: &mut App) -> Option<AnyWindowHandle> {
    let existing = cx.read_global::<AppState, _>(|state, _| state.window);

    if let Some(window) = existing {
        // The window may have been closed since the last time: it is
        // opened again then.
        if window
            .update(cx, |_view, window, _cx| {
                window.set_visible(true);
            })
            .is_ok()
        {
            return Some(window);
        }

        cx.update_global(|state: &mut AppState, _cx| {
            state.window = None;
        });
    }

    let bounds = gpui::Bounds::centered(None, gpui::size(gpui::px(900.), gpui::px(620.)), cx);
    let view = cx.read_global::<AppState, _>(|state, _| state.view.clone());

    let window = cx.open_window(
        gpui::WindowOptions::default()
            .window_bounds(gpui::WindowBounds::Windowed(bounds))
            .app_id(Some("lwe-gui".to_string()))
            .titlebar(Some(gpui::TitlebarOptions {
                title: Some("Wallpaper Engine".into()),
                ..Default::default()
            })),
        move |_window, _cx| view.clone(),
    );

    match window {
        Ok(window) => {
            let handle: AnyWindowHandle = window.into();
            cx.update_global(|state: &mut AppState, _cx| {
                state.window = Some(handle);
            });
            Some(handle)
        }
        Err(error) => {
            log::error!("the picker window could not be opened: {error}");
            None
        }
    }
}

/// Shows a failure to the user.
///
/// The failure is shown in a modal dialog of the picker window: the
/// window is opened when it is not there, because a failure the user
/// cannot see is a failure that is swallowed.
pub async fn show_failure(cx: &mut AsyncApp, title: &str, detail: &str) {
    let Some(window) = cx.update(open_picker) else {
        log::error!("{title}: {detail}");
        return;
    };

    let receiver = window.update(cx, |_view, window, cx| {
        window.prompt(
            gpui::PromptLevel::Critical,
            title,
            Some(detail),
            &["Close"],
            cx,
        )
    });

    match receiver {
        Ok(receiver) => {
            // The answer of the user is not needed, but the dialog is
            // awaited: it stays open until it is closed.
            let _ = receiver.await;
        }
        Err(error) => log::error!("the failure could not be shown: {error}"),
    }
}

/// Stops the wallpaper and quits the application.
fn quit(cx: &mut App) {
    // The wallpaper is stopped before the application goes away.
    cx.update_global(|state: &mut AppState, _cx| state.stop());

    cx.quit();

    // The tray thread and the name of the tray on the bus have to go away
    // as well.
    std::process::exit(0);
}

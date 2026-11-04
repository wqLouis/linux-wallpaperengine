//! The picker: the window that lists the wallpapers.
//!
//! The picker shows the wallpapers of the workshop folder with their
//! preview, their title and their kind.  Clicking one of them shows it as
//! the desktop background.
//!
//! The window is a *view* of the state of the application: the list of
//! the wallpapers lives in the state, so closing the window and opening
//! it again does not scan the folder a second time.
//!
//! The state of the picker is a plain data structure without any gpui in
//! it, so its transitions are tested without a display.

use std::path::PathBuf;
use std::rc::Rc;

use gpui::colors::Colors;
use gpui::prelude::*;
use gpui::{
    AppContext, Context, ObjectFit, PathPromptOptions, Render, Window, div, img, px, uniform_list,
};
use linux_wallpaper_engine::engine::WallpaperSource;
use linux_wallpaper_engine::scene::loader::project::WallpaperKind;

use crate::AppState;

/// What the wallpaper is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WallpaperState {
    /// No wallpaper is running.
    #[default]
    Stopped,
    /// The wallpaper is starting: it takes a while to come up.
    Starting,
    /// The wallpaper is running.
    Running,
    /// The wallpaper is paused.
    Paused,
}

/// One wallpaper of the list.
#[derive(Clone, Debug)]
pub struct Row {
    /// The id of the wallpaper.
    pub id: String,
    /// The title of the wallpaper.
    pub title: String,
    /// The kind of the wallpaper.
    pub kind: WallpaperKind,
    /// The preview of the wallpaper, when it has one.
    pub preview: Option<PathBuf>,
    /// Where the wallpaper lives.
    pub source: WallpaperSource,
}

/// What the picker shows.
///
/// The state is a plain data structure: the transitions of the picker are
/// tested without a display.
#[derive(Clone, Debug, Default)]
pub struct PickerState {
    /// The folder that is scanned.
    pub folder: Option<PathBuf>,
    /// The wallpapers of the folder.
    pub rows: Rc<Vec<Row>>,
    /// The id of the wallpaper that is running.
    pub current: Option<String>,
    /// The title of the wallpaper that is running.
    pub title: Option<String>,
    /// What the wallpaper is doing.
    pub running: WallpaperState,
    /// The problems the wallpaper reported.
    pub problems: Vec<String>,
    /// Whether the folder is being scanned.
    pub scanning: bool,
}

impl PickerState {
    /// The scan of a folder begins.
    ///
    /// The old list is dropped: the wallpapers of another folder are not
    /// the wallpapers of this one.
    pub fn begin_scan(&mut self, folder: PathBuf) {
        self.folder = Some(folder);
        self.scanning = true;
        self.rows = Rc::new(Vec::new());
    }

    /// The scan of the folder is over.
    pub fn finish_scan(&mut self, rows: Vec<Row>) {
        self.scanning = false;
        self.rows = Rc::new(rows);
    }

    /// A wallpaper is picked: it is the one that is running from now on.
    pub fn select(&mut self, row: &Row) {
        self.current = Some(row.id.clone());
        self.title = Some(row.title.clone());
    }

    /// What the wallpaper is doing changed.
    pub fn set_running(&mut self, running: WallpaperState) {
        self.running = running;
    }

    /// The wallpaper reported problems: they are shown to the user.
    pub fn add_problems(&mut self, problems: Vec<String>) {
        self.problems.extend(problems);
    }

    /// The line that tells what the picker is doing.
    pub fn status(&self) -> String {
        let mut parts = Vec::new();

        if self.scanning {
            parts.push("scanning…".to_string());
        } else {
            match self.folder.as_ref() {
                Some(folder) => parts.push(format!(
                    "{} wallpapers in {}",
                    self.rows.len(),
                    folder.display()
                )),
                None => parts.push("no workshop folder has been chosen yet".to_string()),
            }
        }

        let title = self.title.as_deref().unwrap_or("the wallpaper");
        parts.push(match self.running {
            WallpaperState::Stopped => "no wallpaper is running".to_string(),
            WallpaperState::Starting => "the wallpaper is starting…".to_string(),
            WallpaperState::Running => format!("{title} is running"),
            WallpaperState::Paused => format!("{title} is paused"),
        });

        if !self.problems.is_empty() {
            parts.push(format!("{} problems", self.problems.len()));
        }

        parts.join(" · ")
    }
}

/// The picker: the window that lists the wallpapers.
pub struct Picker;

impl Render for Picker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::for_appearance(window);
        let state = cx.read_global::<AppState, _>(|state, _| state.picker.clone());
        let rows = state.rows.clone();
        let current = state.current.clone();
        let view = cx.entity();

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.background)
            .text_color(colors.text)
            .child(header(&colors, &state, cx))
            .child(status_line(&colors, &state, cx))
            .child(
                uniform_list("wallpapers", rows.len(), move |range, _window, _cx| {
                    range
                        .map(|index| {
                            let row = &rows[index];
                            let view = view.clone();
                            let id = row.id.clone();
                            let selected = current.as_deref() == Some(row.id.as_str());

                            div()
                                .id(format!("wallpaper-{id}"))
                                .h(px(88.))
                                .flex()
                                .items_center()
                                .gap_3()
                                .px_4()
                                .border_b_1()
                                .border_color(colors.separator)
                                .bg(if selected {
                                    colors.selected
                                } else {
                                    colors.background
                                })
                                .text_color(if selected {
                                    colors.selected_text
                                } else {
                                    colors.text
                                })
                                .on_click(move |_event, _window, cx| {
                                    view.update(cx, |_view, cx| select(cx, id.clone()));
                                })
                                .child(preview(row, &colors))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(div().text_lg().child(row.title.clone()))
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(if selected {
                                                    colors.selected_text
                                                } else {
                                                    colors.disabled
                                                })
                                                .child(kind_label(&row.kind)),
                                        ),
                                )
                        })
                        .collect::<Vec<_>>()
                })
                .flex_1(),
            )
    }
}

/// The header of the window: the folder and the button that changes it.
fn header(colors: &Colors, state: &PickerState, cx: &mut Context<Picker>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .px_4()
        .py_3()
        .border_b_1()
        .border_color(colors.separator)
        .child(
            div()
                .flex()
                .flex_col()
                .child(div().text_lg().child("Wallpapers"))
                .child(div().text_sm().text_color(colors.disabled).child(
                    match state.folder.as_ref() {
                        Some(folder) => folder.display().to_string(),
                        None => "no folder has been chosen yet".to_string(),
                    },
                )),
        )
        .child(
            div()
                .id("choose-folder")
                .cursor_pointer()
                .px_3()
                .py_1()
                .rounded_md()
                .bg(colors.container)
                .border_1()
                .border_color(colors.border)
                .child("Choose folder…")
                .on_click(cx.listener(|_picker, _event, _window, cx| {
                    choose_folder(cx);
                })),
        )
}

/// The line that tells what the picker is doing.
///
/// The problems of the wallpaper are counted here; clicking the line
/// shows what they are.
fn status_line(colors: &Colors, state: &PickerState, cx: &mut Context<Picker>) -> impl IntoElement {
    let line = div()
        .id("status")
        .px_4()
        .py_2()
        .text_sm()
        .text_color(colors.disabled)
        .child(state.status());

    if state.problems.is_empty() {
        return line.into_any_element();
    }

    line.cursor_pointer()
        .on_click(cx.listener(|_picker, _event, _window, cx| {
            let details =
                cx.read_global::<AppState, _>(|state, _| state.picker.problems.join("\n"));

            cx.spawn(async move |_picker, cx| {
                crate::show_failure(
                    cx,
                    "Some files of the wallpaper could not be used",
                    &details,
                )
                .await;
            })
            .detach();
        }))
        .into_any_element()
}

/// The preview of a wallpaper.
fn preview(row: &Row, colors: &Colors) -> impl IntoElement {
    match row.preview.as_ref() {
        Some(path) => img(path.clone())
            .w(px(120.))
            .h(px(72.))
            .rounded_md()
            .object_fit(ObjectFit::Contain)
            .into_any_element(),
        None => div()
            .w(px(120.))
            .h(px(72.))
            .rounded_md()
            .bg(colors.container)
            .flex()
            .items_center()
            .justify_center()
            .text_sm()
            .text_color(colors.disabled)
            .child("no preview")
            .into_any_element(),
    }
}

/// The name of the kind of a wallpaper.
fn kind_label(kind: &WallpaperKind) -> String {
    match kind {
        WallpaperKind::Scene => "Scene".into(),
        WallpaperKind::Video => "Video".into(),
        WallpaperKind::Web => "Web".into(),
        WallpaperKind::Application => "Application".into(),
        WallpaperKind::Unknown(name) => name.clone(),
    }
}

/// The user asked for another folder.
///
/// The file picker of the desktop is used: the GUI does not write a
/// dialog of its own.
fn choose_folder(cx: &mut Context<Picker>) {
    let receiver = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Select the Wallpaper Engine workshop folder".into()),
    });

    cx.spawn(async move |_picker, cx| {
        match receiver.await {
            Ok(Ok(Some(folders))) => {
                if let Some(folder) = folders.into_iter().next() {
                    cx.update(|cx| crate::choose_folder(cx, folder));
                }
            }
            // The user did not choose anything: nothing happens.
            Ok(Ok(None)) => {}
            Ok(Err(error)) => {
                let detail = format!("the folder could not be chosen: {error}");
                crate::show_failure(cx, "The folder could not be chosen", &detail).await;
            }
            // The dialog was closed without an answer.
            Err(_) => {}
        }
    })
    .detach();
}

/// The user picked a wallpaper: it is shown as the desktop background.
fn select(cx: &mut Context<Picker>, id: String) {
    let row = cx.read_global::<AppState, _>(|state, _| {
        state.picker.rows.iter().find(|row| row.id == id).cloned()
    });

    let Some(row) = row else {
        return;
    };

    // The wallpaper is started by the application: the renderer lives in
    // the engine, not in the window.
    cx.update_global(|state: &mut AppState, cx| state.choose(row, cx));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row of the list, for the tests.
    fn row(id: &str, title: &str) -> Row {
        Row {
            id: id.into(),
            title: title.into(),
            kind: WallpaperKind::Scene,
            preview: None,
            source: WallpaperSource::Folder(PathBuf::from(id)),
        }
    }

    /// A fresh picker holds nothing.
    #[test]
    fn a_fresh_picker_holds_nothing() {
        let state = PickerState::default();

        assert!(state.rows.is_empty());
        assert!(state.current.is_none());
        assert!(!state.scanning);
        assert_eq!(state.running, WallpaperState::Stopped);
        assert!(state.status().contains("no workshop folder"));
    }

    /// The scan of a folder fills the list.
    #[test]
    fn the_scan_fills_the_list() {
        let mut state = PickerState::default();

        state.begin_scan(PathBuf::from("/workshop"));
        assert!(state.scanning);
        assert!(state.status().contains("scanning"));

        state.finish_scan(vec![row("1", "Aurora"), row("2", "Borealis")]);

        assert!(!state.scanning);
        assert_eq!(state.rows.len(), 2);
        assert!(state.status().contains("2 wallpapers"));
    }

    /// A scan of another folder drops the old list.
    #[test]
    fn a_scan_of_another_folder_drops_the_old_list() {
        let mut state = PickerState::default();
        state.finish_scan(vec![row("1", "Aurora")]);

        state.begin_scan(PathBuf::from("/elsewhere"));

        assert!(state.rows.is_empty());
    }

    /// The picked wallpaper is the one that is running.
    #[test]
    fn the_picked_wallpaper_is_the_one_that_runs() {
        let mut state = PickerState::default();
        state.finish_scan(vec![row("1", "Aurora"), row("2", "Borealis")]);

        state.select(&row("2", "Borealis"));

        assert_eq!(state.current.as_deref(), Some("2"));
        assert_eq!(state.title.as_deref(), Some("Borealis"));
    }

    /// The state of the wallpaper is shown.
    #[test]
    fn the_state_of_the_wallpaper_is_shown() {
        let mut state = PickerState::default();
        state.finish_scan(vec![row("1", "Aurora")]);
        state.select(&row("1", "Aurora"));

        state.set_running(WallpaperState::Starting);
        assert!(state.status().contains("starting"));

        state.set_running(WallpaperState::Running);
        assert!(state.status().contains("Aurora is running"));

        state.set_running(WallpaperState::Paused);
        assert!(state.status().contains("Aurora is paused"));
    }

    /// The problems of the wallpaper are counted.
    #[test]
    fn the_problems_of_the_wallpaper_are_counted() {
        let mut state = PickerState::default();

        state.add_problems(vec!["a broken file".into()]);
        state.add_problems(vec!["another broken file".into(), "a third one".into()]);

        assert_eq!(state.problems.len(), 3);
        assert!(state.status().contains("3 problems"));
    }
}

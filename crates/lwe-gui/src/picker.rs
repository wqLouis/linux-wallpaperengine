//! The picker: the window that shows the wallpapers and the settings.
//!
//! The picker has two pages: the wallpapers of the workshop folder and
//! the settings of the user.  Clicking one of the wallpapers shows it as
//! the desktop background.
//!
//! The window is a *view* of the state of the application: the list of
//! the wallpapers and the page that is shown live in the state, so closing
//! the window and opening it again does not scan the folder a second time.
//!
//! The state of the picker is a plain data structure without any gpui in
//! it, so its transitions are tested without a display.

use std::path::PathBuf;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, ClickEvent, Context, DispatchPhase, Entity, Fill, MouseMoveEvent,
    ObjectFit, Overflow, PathPromptOptions, Pixels, Render, Window, div, img, linear_color_stop,
    linear_gradient, px, svg, uniform_list,
};
use linux_wallpaper_engine::engine::WallpaperSource;
use linux_wallpaper_engine::scene::loader::project::WallpaperKind;

use crate::config::{FitMode, GuiConfig, MAX_FPS};
use crate::design::{
    CARD_GAP, CONTENT_RADIUS, Depth, Space, Theme, Type, card_radius, inner_radius,
};

use crate::AppState;

/// The gear that opens the settings.
const GEAR: &[u8] = include_bytes!("../assets/icons/gear.svg");

/// The folder of the wallpapers.
const FOLDER: &[u8] = include_bytes!("../assets/icons/folder.svg");

/// The check of a switch that is on.
const CHECK: &[u8] = include_bytes!("../assets/icons/check.svg");

/// The chevron that goes back to the wallpapers.
const CHEVRON_LEFT: &[u8] = include_bytes!("../assets/icons/chevron-left.svg");

/// The smallest width of a card of the grid.
const MIN_CARD: Pixels = px(220.);

/// The page the picker shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    /// The wallpapers of the workshop folder.
    #[default]
    Wallpapers,
    /// The settings of the user.
    Settings,
}

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
    /// The page that is shown.
    pub page: Page,
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

    /// The page that is shown changes.
    pub fn set_page(&mut self, page: Page) {
        self.page = page;
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

/// The picker: the window that shows the wallpapers and the settings.
#[derive(Default)]
pub struct Picker {
    /// Whether the frame rate is being dragged.
    dragging: bool,
    /// The track of the frame rate while it is dragged.
    track: Option<Bounds<Pixels>>,
}

impl Render for Picker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::new(window);
        let state = cx.read_global::<AppState, _>(|state, _| state.picker.clone());
        let config = cx.read_global::<AppState, _>(|state, _| state.config.clone());

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.palette.background)
            .text_color(theme.palette.text)
            .child(header(&theme, &state, cx))
            .child(match state.page {
                Page::Wallpapers => wallpapers_page(&theme, &state, window, cx),
                Page::Settings => settings_page(&theme, &config, cx),
            })
            .child(status_bar(&theme, &state, cx))
    }
}

/// The header of the window: the title of the page and the button that
/// leaves it.
fn header(theme: &Theme, state: &PickerState, cx: &mut Context<Picker>) -> AnyElement {
    let (title, icon, id, page) = match state.page {
        Page::Wallpapers => ("Wallpapers", GEAR, "open-settings", Page::Settings),
        Page::Settings => ("Settings", CHEVRON_LEFT, "close-settings", Page::Wallpapers),
    };

    let button = icon_button(
        theme,
        id,
        icon,
        cx.listener(move |_picker, _event, _window, cx| set_page(cx, page)),
    );

    div()
        .flex()
        .items_center()
        .justify_between()
        .h(px(56.))
        .px(Space::Xl.size())
        .border_b_1()
        .border_color(theme.palette.border)
        .child(
            div()
                .text_size(Type::Title.size())
                .font_weight(Type::Title.weight())
                .child(title),
        )
        .child(button)
        .into_any_element()
}

/// A round button with an icon in it.
///
/// The button is a raised surface with a hairline border; when it is
/// pressed it becomes a groove and drops by one pixel.
fn icon_button(
    theme: &Theme,
    id: &'static str,
    icon: &'static [u8],
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let hover = theme.palette.hover;
    let groove_dark = theme.palette.groove_dark;
    let groove_light = theme.palette.groove_light;

    div()
        .id(id)
        .size(px(32.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(theme.palette.surface)
        .border_1()
        .border_color(theme.palette.border)
        .shadow(Depth::Raised.shadows(&theme.palette))
        .hover(move |style| style.bg(hover))
        .active(move |mut style| {
            style.margin.top = Some(px(1.).into());
            style.background = Some(Fill::Color(linear_gradient(
                180.,
                linear_color_stop(groove_dark, 0.),
                linear_color_stop(groove_light, 1.),
            )));
            style
        })
        .on_click(on_click)
        .child(
            svg()
                .data(icon)
                .size(px(16.))
                .text_color(theme.palette.text),
        )
}

/// The page of the wallpapers: the wallpapers as cards in a grid.
fn wallpapers_page(
    theme: &Theme,
    state: &PickerState,
    window: &mut Window,
    cx: &mut Context<Picker>,
) -> AnyElement {
    let padding = Space::Xl.size();
    let gap = Space::Lg.size();
    let inner = window.viewport_size().width - padding * 2.;

    if state.rows.is_empty() {
        return empty_page(theme, state);
    }

    // The cards are as wide as they can be: the grid has as many columns
    // as fit into the window.
    let columns = (((inner + gap) / (MIN_CARD + gap)).floor() as usize).max(1);
    let card_width = (inner - gap * (columns as f32 - 1.)) / columns as f32;
    let preview_height = (card_width - CARD_GAP * 2.) * 9. / 16.;
    let card_height = preview_height + px(62.);

    let items = state.rows.clone();
    let current = state.current.clone();
    let view = cx.entity();
    let theme = *theme;
    let rows = items.len().div_ceil(columns);

    uniform_list("wallpapers", rows, move |range, _window, _cx| {
        range
            .map(|row| {
                let start = row * columns;
                let end = (start + columns).min(items.len());

                let cards = (start..end).map(|index| {
                    let item = &items[index];
                    let selected = current.as_deref() == Some(item.id.as_str());
                    wallpaper_card(
                        &theme,
                        item,
                        selected,
                        card_width,
                        preview_height,
                        &view,
                        index,
                    )
                });

                div().flex().gap(gap).h(card_height).children(cards)
            })
            .collect()
    })
    .flex_1()
    .px(padding)
    .pt(padding)
    .pb(padding)
    .into_any_element()
}

/// A card of the grid: the preview of a wallpaper, its name and its kind.
///
/// The card that is picked is raised a little more and has a ring of two
/// pixels in the colour of the text.
fn wallpaper_card(
    theme: &Theme,
    row: &Row,
    selected: bool,
    width: Pixels,
    preview_height: Pixels,
    view: &Entity<Picker>,
    index: usize,
) -> AnyElement {
    let id = row.id.clone();
    let border_strong = theme.palette.border_strong;

    let card = theme.raised(
        div().w(width).flex().flex_col().p(CARD_GAP),
        card_radius(),
        if selected { Depth::Picked } else { Depth::Card },
    );

    let card = card
        .id(("wallpaper", index))
        .cursor_pointer()
        .when(selected, |card| {
            card.ring_2().ring_color(theme.palette.text)
        })
        .hover(move |style| style.border_color(border_strong))
        .on_click({
            let view = view.clone();
            move |_event, _window, cx| {
                view.update(cx, |_view, cx| select(cx, id.clone()));
            }
        })
        .child(preview(theme, row, preview_height))
        .child(
            div()
                .mt(Space::Sm.size())
                .text_size(Type::Heading.size())
                .font_weight(Type::Heading.weight())
                .truncate()
                .child(row.title.clone()),
        )
        .child(
            div()
                .mt(px(2.))
                .text_size(Type::Small.size())
                .text_color(theme.palette.muted)
                .child(kind_label(&row.kind)),
        );

    card.into_any_element()
}

/// The preview of a wallpaper.
fn preview(theme: &Theme, row: &Row, height: Pixels) -> AnyElement {
    match row.preview.as_ref() {
        Some(path) => img(path.clone())
            .w_full()
            .h(height)
            .rounded(CONTENT_RADIUS)
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        None => div()
            .w_full()
            .h(height)
            .rounded(CONTENT_RADIUS)
            .bg(theme.palette.groove_light)
            .into_any_element(),
    }
}

/// The page of the settings: the groups of the settings of the user.
fn settings_page(theme: &Theme, config: &GuiConfig, cx: &mut Context<Picker>) -> AnyElement {
    let card = theme.surface(
        div()
            .flex()
            .flex_col()
            .child(folder_row(
                theme,
                "Wallpapers folder",
                "Where the wallpapers are read from.",
                config.workshop_dir.as_deref(),
                "choose-workshop",
                cx,
            ))
            .child(theme.hairline())
            .child(fit_row(theme, config.fit_mode, cx))
            .child(theme.hairline())
            .child(fps_row(theme, config.target_fps, cx))
            .child(theme.hairline())
            .child(toggle_row(
                theme,
                "Effects",
                "The effects of the scene.",
                config.effects,
                "effects",
                cx,
            ))
            .child(theme.hairline())
            .child(toggle_row(
                theme,
                "Puppet models",
                "The puppet models of the scene.",
                config.puppets,
                "puppets",
                cx,
            ))
            .child(theme.hairline())
            .child(folder_row(
                theme,
                "Wallpaper Engine assets",
                "Where the assets of Wallpaper Engine are.",
                config.assets_path.as_deref(),
                "choose-assets",
                cx,
            )),
        card_radius(),
        Depth::Card,
    );

    let mut page = div()
        .flex_1()
        .px(Space::Xl.size())
        .py(Space::Lg.size())
        .child(card);

    // The page scrolls when the settings do not fit into the window.
    page.style().overflow.y = Some(Overflow::Scroll);

    page.into_any_element()
}

/// A row of the settings: a label with a description, and a control.
fn setting_row(
    theme: &Theme,
    label: &str,
    description: &str,
    control: impl IntoElement,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(Space::Xl.size())
        .px(Space::Lg.size())
        .py(Space::Md.size())
        .child(
            div()
                .flex()
                .flex_col()
                .gap(Space::Xs.size())
                .w(px(300.))
                .flex_shrink(1.)
                .min_w(px(150.))
                .child(
                    div()
                        .text_size(Type::Heading.size())
                        .font_weight(Type::Heading.weight())
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .text_size(Type::Small.size())
                        .text_color(theme.palette.muted)
                        .child(description.to_string()),
                ),
        )
        .child(control)
        .into_any_element()
}

/// The row of the folder of the wallpapers or of the assets.
fn folder_row(
    theme: &Theme,
    label: &str,
    description: &str,
    folder: Option<&std::path::Path>,
    id: &'static str,
    cx: &mut Context<Picker>,
) -> AnyElement {
    let path = match folder {
        Some(folder) => folder.display().to_string(),
        None => "no folder has been chosen yet".to_string(),
    };

    let button = icon_button(
        theme,
        id,
        FOLDER,
        cx.listener(move |_picker, _event, _window, cx| choose_folder(cx, id)),
    );

    setting_row(
        theme,
        label,
        description,
        div()
            .flex()
            .items_center()
            .gap(Space::Sm.size())
            .child(
                div()
                    .max_w(px(240.))
                    .text_size(Type::Small.size())
                    .text_color(theme.palette.muted)
                    .truncate()
                    .child(path),
            )
            .child(button),
    )
}

/// The row of the way the wallpaper is fitted to the screen.
fn fit_row(theme: &Theme, current: FitMode, cx: &mut Context<Picker>) -> AnyElement {
    let segments = div()
        .flex()
        .flex_1()
        .children(FitMode::ALL.into_iter().enumerate().map(|(index, mode)| {
            let selected = mode == current;
            let text = if selected {
                theme.palette.text
            } else {
                theme.palette.muted
            };

            div()
                .id(("fit-mode", index))
                .flex_1()
                .h(px(26.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(inner_radius(CONTENT_RADIUS, px(2.)))
                .text_size(Type::Small.size())
                .text_color(text)
                .cursor_pointer()
                .when(selected, |segment| {
                    segment
                        .bg(theme.palette.surface)
                        .shadow(Depth::Raised.shadows(&theme.palette))
                })
                .on_click(cx.listener(move |_picker, _event, _window, cx| {
                    set_fit_mode(cx, mode);
                }))
                .child(mode.label())
        }));

    let control = theme
        .groove(
            div().flex().w(px(240.)).p(px(2.)).rounded(CONTENT_RADIUS),
            CONTENT_RADIUS,
        )
        .child(segments);

    setting_row(
        theme,
        "Fit mode",
        "How the wallpaper fills the screen.",
        control,
    )
}

/// The row of the frame rate.
fn fps_row(theme: &Theme, target_fps: Option<u32>, cx: &mut Context<Picker>) -> AnyElement {
    let value = target_fps.unwrap_or(MAX_FPS);
    let label = fps_label(value);

    let control = div()
        .flex()
        .items_center()
        .gap(Space::Md.size())
        .child(slider(theme, value, cx))
        .child(
            div()
                .w(px(64.))
                .text_size(Type::Small.size())
                .text_color(theme.palette.muted)
                .child(label),
        );

    setting_row(
        theme,
        "Frame rate",
        "How many frames are drawn in a second.",
        control,
    )
}

/// The slider of the frame rate.
fn slider(theme: &Theme, value: u32, cx: &mut Context<Picker>) -> AnyElement {
    let width = px(160.);
    let knob = px(16.);
    let fraction = value as f32 / MAX_FPS as f32;
    let offset = px(fraction * f32::from(width - knob));

    let view_down = cx.entity();
    let view_up = cx.entity();

    div()
        .id("fps-slider")
        .relative()
        .w(width)
        .h(px(20.))
        .flex()
        .items_center()
        .on_mouse_down_all(move |event, phase, hitbox, _window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }

            let bounds = hitbox.bounds;
            let value = value_at(event.position.x, bounds);

            view_down.update(cx, |picker, cx| {
                picker.dragging = true;
                picker.track = Some(bounds);
                set_fps(cx, value);
            });
        })
        .on_mouse_move(cx.listener(|picker, event: &MouseMoveEvent, _window, cx| {
            if !picker.dragging {
                return;
            }

            let Some(bounds) = picker.track else {
                return;
            };

            let value = value_at(event.position.x, bounds);
            set_fps(cx, value);
        }))
        .on_mouse_up_all(move |_event, _phase, _hitbox, _window, cx| {
            view_up.update(cx, |picker, _cx| {
                picker.dragging = false;
                picker.track = None;
            });
        })
        .child(theme.groove(div().w(width).h(px(8.)).rounded_full(), px(4.)))
        .child(
            div()
                .absolute()
                .left(offset)
                .size(knob)
                .rounded_full()
                .bg(theme.palette.surface)
                .border_1()
                .border_color(theme.palette.border_strong)
                .shadow(Depth::Raised.shadows(&theme.palette)),
        )
        .into_any_element()
}

/// The value of the frame rate at a place of the slider.
fn value_at(x: Pixels, bounds: Bounds<Pixels>) -> u32 {
    let width = bounds.size.width;
    if width <= px(0.) {
        return 0;
    }

    let ratio = ((x - bounds.origin.x) / width).clamp(0., 1.);
    (ratio * MAX_FPS as f32).round() as u32
}

/// The name of a frame rate, as the user sees it.
fn fps_label(value: u32) -> String {
    match value {
        0 => "still image".to_string(),
        value if value >= MAX_FPS => "unlimited".to_string(),
        value => format!("{value} fps"),
    }
}

/// The row of a switch.
fn toggle_row(
    theme: &Theme,
    label: &str,
    description: &str,
    on: bool,
    id: &'static str,
    cx: &mut Context<Picker>,
) -> AnyElement {
    let track = div()
        .w(px(36.))
        .h(px(20.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_start()
        .when(on, |track| {
            track
                .justify_end()
                .bg(theme.palette.text)
                .border_1()
                .border_color(theme.palette.text)
        })
        .when(!on, |track| {
            theme
                .groove(track, px(10.))
                .border_1()
                .border_color(theme.palette.border_strong)
        })
        .p(px(2.))
        .child(
            div()
                .size(px(16.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.palette.surface)
                .shadow(Depth::Raised.shadows(&theme.palette))
                .when(on, |knob| {
                    knob.child(
                        svg()
                            .data(CHECK)
                            .size(px(10.))
                            .text_color(theme.palette.text),
                    )
                }),
        );

    let control = div()
        .id(id)
        .cursor_pointer()
        .hover(move |style| style.opacity(0.85))
        .on_click(cx.listener(move |_picker, _event, _window, cx| {
            toggle_setting(cx, id, !on);
        }))
        .child(track);

    setting_row(theme, label, description, control)
}

/// The line at the bottom of the window that tells what the picker is
/// doing.
///
/// The problems of the wallpaper are counted here; clicking the line
/// shows what they are.
fn status_bar(theme: &Theme, state: &PickerState, cx: &mut Context<Picker>) -> AnyElement {
    let running = match state.running {
        WallpaperState::Stopped => "no wallpaper is running".to_string(),
        WallpaperState::Starting => "the wallpaper is starting…".to_string(),
        WallpaperState::Running => {
            let title = state.title.as_deref().unwrap_or("the wallpaper");
            format!("{title} is running")
        }
        WallpaperState::Paused => {
            let title = state.title.as_deref().unwrap_or("the wallpaper");
            format!("{title} is paused")
        }
    };

    let count = match state.folder.as_ref() {
        Some(_) if state.scanning => "scanning…".to_string(),
        Some(_) => format!("{} wallpapers", state.rows.len()),
        None => "no folder has been chosen yet".to_string(),
    };

    let problems = if state.problems.is_empty() {
        None
    } else {
        Some(format!("{} problems", state.problems.len()))
    };

    let right = div()
        .flex()
        .items_center()
        .gap(Space::Md.size())
        .child(div().child(count));

    let right = match problems {
        Some(problems) => right.child(
            div()
                .id("problems")
                .cursor_pointer()
                .hover(|style| style.opacity(0.7))
                .child(problems),
        ),
        None => right,
    };

    div()
        .flex()
        .items_center()
        .justify_between()
        .h(px(36.))
        .px(Space::Xl.size())
        .border_t_1()
        .border_color(theme.palette.border)
        .text_size(Type::Small.size())
        .text_color(theme.palette.muted)
        .child(div().child(running))
        .child(right)
        .id("status")
        .on_click(cx.listener(|_picker, _event, _window, cx| {
            let details =
                cx.read_global::<AppState, _>(|state, _| state.picker.problems.join("\n"));

            if details.is_empty() {
                return;
            }

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

/// The page that is shown when there is no wallpaper to show.
fn empty_page(theme: &Theme, state: &PickerState) -> AnyElement {
    let (title, description) = if state.scanning {
        ("Scanning…", "The folder of the wallpapers is being read.")
    } else if state.folder.is_some() {
        (
            "No wallpapers",
            "The folder of the wallpapers holds none. Another folder can be chosen in the settings.",
        )
    } else {
        (
            "No workshop folder",
            "The folder the wallpapers are read from has not been chosen yet. It can be chosen in the settings.",
        )
    };

    div()
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(Space::Sm.size())
        .child(
            div()
                .text_size(Type::Heading.size())
                .font_weight(Type::Heading.weight())
                .child(title),
        )
        .child(
            div()
                .text_size(Type::Small.size())
                .text_color(theme.palette.muted)
                .child(description),
        )
        .into_any_element()
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

/// The page that is shown changes.
fn set_page(cx: &mut Context<Picker>, page: Page) {
    cx.update_global(|state: &mut AppState, cx| {
        state.picker.set_page(page);
        cx.notify();
    });
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

/// The way the wallpaper is fitted to the screen changed.
fn set_fit_mode(cx: &mut Context<Picker>, mode: FitMode) {
    cx.update_global(|state: &mut AppState, cx| state.set_fit_mode(mode, cx));
}

/// The frame rate changed.
fn set_fps(cx: &mut Context<Picker>, value: u32) {
    cx.update_global(|state: &mut AppState, cx| state.set_fps(value, cx));
}

/// A switch was flipped.
fn toggle_setting(cx: &mut Context<Picker>, id: &str, on: bool) {
    cx.update_global(|state: &mut AppState, cx| match id {
        "effects" => state.set_effects(on, cx),
        "puppets" => state.set_puppets(on, cx),
        _ => {}
    });
}

/// The user asked for another folder.
///
/// The file picker of the desktop is used: the GUI does not write a
/// dialog of its own.
fn choose_folder(cx: &mut Context<Picker>, id: &'static str) {
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
                    cx.update(|cx| {
                        if id == "choose-assets" {
                            crate::choose_assets(cx, folder);
                        } else {
                            crate::choose_folder(cx, folder);
                        }
                    });
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
        assert_eq!(state.page, Page::Wallpapers);
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

    /// The page that is shown is remembered.
    #[test]
    fn the_page_that_is_shown_is_remembered() {
        let mut state = PickerState::default();

        assert_eq!(state.page, Page::Wallpapers);

        state.set_page(Page::Settings);
        assert_eq!(state.page, Page::Settings);

        state.set_page(Page::Wallpapers);
        assert_eq!(state.page, Page::Wallpapers);
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

//! The design language of the interface.
//!
//! Three rules hold everywhere:
//!
//! * **One scale.**  Every padding, margin, gap and size comes from the
//!   five numbers of the levels.  Nothing is measured by eye.
//! * **Depth, not borders.**  A surface has the colour of the background:
//!   what makes it visible are the two shadows of its elevation - the
//!   light one that falls from the upper left and the dark one that falls
//!   from the lower right.  A surface that is pressed into the background
//!   has the two shadows the other way round.
//! * **Concentric corners.**  The radius of an element is the radius of the
//!   element around it plus the gap that separates the two, so the corners
//!   follow each other instead of crossing.

use gpui::prelude::*;
use gpui::{
    div, hsla, point, px, Background, BoxShadow, Div, Hsla, Pixels, Window, WindowAppearance,
};

/// The levels of the interface.
///
/// A level says what an element is.  The number of the level is the only
/// number the interface uses: it is the padding of the element, the margin
/// around it and the gap that separates it from the element around it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    /// A control: a button, a chip, the icon of a card.
    Inline,
    /// A card: a wallpaper of the list.
    Card,
    /// A panel: the header, the status bar.
    Panel,
    /// The window: the surface of the application.
    Window,
    /// The largest element of the interface.
    Hero,
}

impl Level {
    /// The size of the level.
    pub fn size(self) -> Pixels {
        match self {
            Level::Inline => px(16.),
            Level::Card => px(24.),
            Level::Panel => px(32.),
            Level::Window => px(48.),
            Level::Hero => px(64.),
        }
    }
}

/// The radius of the corners of an element that sits `gap` pixels inside a
/// parent whose corners have the radius `outer`.
///
/// The corners of the two elements are concentric: the inner element is
/// rounded by the gap that separates it from the outer one, so the two
/// follow each other instead of crossing.
///
/// A radius bigger than half of the element would turn it into a circle;
/// the window clamps the radius of a corner to the size of the element, so
/// a small element becomes a pill and not a disc.
pub fn nested_radius(outer: Pixels, gap: Pixels) -> Pixels {
    outer + gap
}

/// The colours of the interface.
///
/// A neumorphic surface has the colour of its background: what makes it
/// visible are the shadows, not the colour.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// The colour of the background and of the surfaces.
    pub background: Hsla,
    /// The colour of the text.
    pub text: Hsla,
    /// The colour of the text that says less.
    pub muted: Hsla,
    /// The colour of the accent: the wallpaper that runs.
    pub accent: Hsla,
    /// The shadow of the light that falls from the upper left.
    pub light: Hsla,
    /// The shadow of the darkness that falls from the lower right.
    pub dark: Hsla,
}

impl Palette {
    /// The colours of the light appearance.
    pub fn light() -> Self {
        Self {
            background: hsla(214., 32., 91., 1.),
            text: hsla(218., 28., 22., 1.),
            muted: hsla(216., 16., 52., 1.),
            accent: hsla(226., 84., 62., 1.),
            light: hsla(0., 0., 100., 0.85),
            dark: hsla(213., 32., 66., 0.55),
        }
    }

    /// The colours of the dark appearance.
    pub fn dark() -> Self {
        Self {
            background: hsla(216., 16., 19., 1.),
            text: hsla(214., 26., 90., 1.),
            muted: hsla(215., 12., 62., 1.),
            accent: hsla(226., 90., 74., 1.),
            light: hsla(0., 0., 100., 0.06),
            dark: hsla(0., 0., 0., 0.45),
        }
    }

    /// The colours of the appearance of the window.
    pub fn for_appearance(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
        }
    }
}

/// How deep a surface sits above the background.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Elevation {
    /// The background itself: no shadow.
    Flat,
    /// A control: a button, a chip.
    Raised,
    /// A card: a wallpaper of the list.
    Card,
    /// A floating element: the wallpaper that runs, the hero.
    Floating,
    /// A surface that is pressed into the background.
    Inset,
}

impl Elevation {
    /// The shadows of the elevation.
    ///
    /// The light shadow falls from the upper left and the dark one from the
    /// lower right; an inset surface has them the other way round, so it
    /// looks pressed into the background.
    pub fn shadows(self, palette: &Palette) -> Vec<BoxShadow> {
        let (offset, blur) = match self {
            Elevation::Flat => return Vec::new(),
            Elevation::Raised => (2., 8.),
            Elevation::Card => (4., 12.),
            Elevation::Floating => (8., 24.),
            Elevation::Inset => (2., 8.),
        };

        let offset = px(offset);
        let blur = px(blur);
        let light: Background = palette.light.into();
        let dark: Background = palette.dark.into();

        match self {
            Elevation::Inset => vec![
                BoxShadow {
                    color: light,
                    offset: point(offset, offset),
                    blur_radius: blur,
                    spread_radius: px(0.),
                    inset: true,
                },
                BoxShadow {
                    color: dark,
                    offset: point(-offset, -offset),
                    blur_radius: blur,
                    spread_radius: px(0.),
                    inset: true,
                },
            ],
            _ => vec![
                BoxShadow {
                    color: light,
                    offset: point(-offset, -offset),
                    blur_radius: blur,
                    spread_radius: px(0.),
                    inset: false,
                },
                BoxShadow {
                    color: dark,
                    offset: point(offset, offset),
                    blur_radius: blur,
                    spread_radius: px(0.),
                    inset: false,
                },
            ],
        }
    }
}

/// The theme of the interface: the colours and the ways to draw with them.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    /// The colours of the theme.
    pub palette: Palette,
}

impl Theme {
    /// The theme of the window.
    pub fn new(window: &Window) -> Self {
        Self {
            palette: Palette::for_appearance(window.appearance()),
        }
    }

    /// The surface of an element: the colour of the background and the
    /// shadows of the elevation.
    ///
    /// The radius is the one of the element: it is calculated with
    /// nested_radius from the radius of the element around it.
    pub fn surface(&self, element: Div, elevation: Elevation, radius: Pixels) -> Div {
        let mut element = element.bg(self.palette.background).rounded(radius);
        element.style().box_shadow = Some(elevation.shadows(&self.palette));
        element
    }

    /// A button: a raised surface with a label in the middle.
    ///
    /// The button sinks into the background when it is pressed; the
    /// pressed state is drawn by the caller with the inset elevation.
    pub fn button(&self, radius: Pixels, label: impl Into<String>) -> Div {
        let button = self.surface(
            div()
                .flex()
                .items_center()
                .justify_center()
                .px(Level::Inline.size())
                .py(px(8.))
                .text_sm()
                .text_color(self.palette.text),
            Elevation::Raised,
            radius,
        );
        button.child(label.into())
    }

    /// A chip: a small raised surface with a label.
    pub fn chip(&self, radius: Pixels, label: impl Into<String>) -> Div {
        let chip = self.surface(
            div()
                .flex()
                .items_center()
                .px(Level::Inline.size())
                .py(px(4.))
                .text_xs()
                .text_color(self.palette.muted),
            Elevation::Raised,
            radius,
        );
        chip.child(label.into())
    }
}
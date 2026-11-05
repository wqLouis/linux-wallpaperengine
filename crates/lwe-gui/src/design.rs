//! The design language of the interface.
//!
//! The interface is quiet: greys, hairlines and soft shadows, nothing that
//! does not carry information.  Three rules hold everywhere:
//!
//! * **One scale.**  Every padding, margin, gap and size comes from the
//!   steps of the scale or of the type scale.  Nothing is measured by
//!   eye.
//! * **Depth, not decoration.**  A surface has a colour of its own, a
//!   hairline border of one pixel and two soft shadows: a tight one and a
//!   soft one, both under the element.  A surface that is pressed into
//!   the background is a *groove*: a step towards the background with a
//!   gradient that is dark at the top and light at the bottom.
//! * **Concentric corners.**  The radius of an element is the radius of
//!   the element around it plus the gap that separates the two: the
//!   corners of nested rectangles are arcs of the same circle.  The
//!   radii live here, so they cannot drift apart.

use gpui::prelude::*;
use gpui::{
    Background, BoxShadow, Div, FontWeight, Hsla, Pixels, Window, WindowAppearance, div, hsla,
    linear_color_stop, linear_gradient, point, px,
};

/// The steps of the scale of the interface.
///
/// Every distance of the interface is one of them: the padding of an
/// element, the margin around it and the gap that separates it from the
/// element around it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Space {
    /// Four pixels.
    Xs,
    /// Eight pixels.
    Sm,
    /// Twelve pixels.
    Md,
    /// Sixteen pixels.
    Lg,
    /// Twenty-four pixels.
    Xl,
}

impl Space {
    /// The size of the step.
    pub fn size(self) -> Pixels {
        match self {
            Space::Xs => px(4.),
            Space::Sm => px(8.),
            Space::Md => px(12.),
            Space::Lg => px(16.),
            Space::Xl => px(24.),
        }
    }
}

/// The steps of the type scale.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Type {
    /// The title of a page.
    Title,
    /// The name of a wallpaper or of a setting.
    Heading,
    /// The text that says less.
    Small,
}

impl Type {
    /// The size of the text.
    pub fn size(self) -> Pixels {
        match self {
            Type::Title => px(15.),
            Type::Heading => px(13.),
            Type::Small => px(12.),
        }
    }

    /// The weight of the text.
    pub fn weight(self) -> FontWeight {
        match self {
            Type::Title => FontWeight::SEMIBOLD,
            Type::Heading => FontWeight::MEDIUM,
            Type::Small => FontWeight::NORMAL,
        }
    }
}

/// The radius of the corners of the content: the previews and the controls.
///
/// Everything that is not a card has this radius.
pub const CONTENT_RADIUS: Pixels = px(6.);

/// The gap between the content of a card and the edge of the card.
///
/// The corners of the card are the corners of the content plus this gap:
/// the two are arcs of the same circle.
pub const CARD_GAP: Pixels = px(8.);

/// The radius of the corners of a card.
///
/// It is derived from the radius of the content and from the gap that
/// separates them, so the corners of a card and the corners of what is
/// inside it follow each other.
pub fn card_radius() -> Pixels {
    CONTENT_RADIUS + CARD_GAP
}

/// The radius of the corners of an element that lies `gap` pixels inside
/// a parent whose corners have the radius `outer`.
///
/// The inner element is rounded by the gap that separates it from the
/// outer one, so the two follow each other instead of crossing.
pub fn inner_radius(outer: Pixels, gap: Pixels) -> Pixels {
    if outer > gap { outer - gap } else { px(0.) }
}

/// The greys of the interface.
///
/// There is no hue anywhere: the interface is grey, and the pictures of
/// the wallpapers are the only colour in it.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// The colour of the window.
    pub background: Hsla,
    /// The colour of a surface that lies on the background.
    pub surface: Hsla,
    /// The colour of the top of a groove: a step towards the background.
    pub groove_dark: Hsla,
    /// The colour of the bottom of a groove.
    pub groove_light: Hsla,
    /// The hairline between two surfaces.
    pub border: Hsla,
    /// A hairline that has to be seen.
    pub border_strong: Hsla,
    /// The colour of a surface that is under the pointer.
    pub hover: Hsla,
    /// The colour of the text.
    pub text: Hsla,
    /// The colour of the text that says less.
    pub muted: Hsla,
    /// The shadow that stays close to the surface.
    pub shadow_tight: Hsla,
    /// The shadow that falls away from the surface.
    pub shadow_soft: Hsla,
    /// The light that falls on the top edge of a raised surface.
    pub highlight: Hsla,
}

impl Palette {
    /// The greys of the light appearance.
    pub fn light() -> Self {
        Self {
            background: hsla(0., 0., 0.97, 1.),
            surface: hsla(0., 0., 1., 1.),
            groove_dark: hsla(0., 0., 0.89, 1.),
            groove_light: hsla(0., 0., 0.94, 1.),
            border: hsla(0., 0., 0.89, 1.),
            border_strong: hsla(0., 0., 0.83, 1.),
            hover: hsla(0., 0., 0.95, 1.),
            text: hsla(0., 0., 0.09, 1.),
            muted: hsla(0., 0., 0.45, 1.),
            shadow_tight: hsla(0., 0., 0., 0.08),
            shadow_soft: hsla(0., 0., 0., 0.06),
            highlight: hsla(0., 0., 1., 0.7),
        }
    }

    /// The greys of the dark appearance.
    pub fn dark() -> Self {
        Self {
            background: hsla(0., 0., 0.06, 1.),
            surface: hsla(0., 0., 0.10, 1.),
            groove_dark: hsla(0., 0., 0.03, 1.),
            groove_light: hsla(0., 0., 0.07, 1.),
            border: hsla(0., 0., 0.18, 1.),
            border_strong: hsla(0., 0., 0.26, 1.),
            hover: hsla(0., 0., 0.14, 1.),
            text: hsla(0., 0., 0.96, 1.),
            muted: hsla(0., 0., 0.62, 1.),
            shadow_tight: hsla(0., 0., 0., 0.5),
            shadow_soft: hsla(0., 0., 0., 0.45),
            highlight: hsla(0., 0., 1., 0.05),
        }
    }

    /// The greys of the appearance of the window.
    pub fn for_appearance(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
        }
    }
}

/// How deep a surface sits above the background.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Depth {
    /// A control: a button, a segment.
    Raised,
    /// A card: a wallpaper of the grid, a group of settings.
    Card,
    /// A card that is picked: it is raised a little more.
    Picked,
}

impl Depth {
    /// The shadows of the depth.
    ///
    /// A tight shadow and a soft one, both under the element: the first
    /// gives the surface its distance from the background, the second
    /// spreads the shadow around it.
    pub fn shadows(self, palette: &Palette) -> Vec<BoxShadow> {
        let (tight, soft) = match self {
            Depth::Raised => (1., 4.),
            Depth::Card => (1., 8.),
            Depth::Picked => (2., 14.),
        };

        vec![
            BoxShadow {
                color: Background::from(palette.shadow_tight),
                offset: point(px(0.), px(tight)),
                blur_radius: px(2.),
                spread_radius: px(0.),
                inset: false,
            },
            BoxShadow {
                color: Background::from(palette.shadow_soft),
                offset: point(px(0.), px(soft / 2.)),
                blur_radius: px(soft),
                spread_radius: px(0.),
                inset: false,
            },
        ]
    }
}

/// The theme of the interface: the greys and the ways to draw with them.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    /// The greys of the theme.
    pub palette: Palette,
}

impl Theme {
    /// The theme of the window.
    pub fn new(window: &Window) -> Self {
        Self {
            palette: Palette::for_appearance(window.appearance()),
        }
    }

    /// A surface that lies on the background: a card, a group of settings.
    ///
    /// The surface has a colour of its own, a hairline border of one
    /// pixel and the shadows of its depth.
    pub fn surface(&self, element: Div, radius: Pixels, depth: Depth) -> Div {
        let mut element = element
            .bg(self.palette.surface)
            .border_1()
            .border_color(self.palette.border)
            .rounded(radius);
        element.style().box_shadow = Some(depth.shadows(&self.palette));
        element
    }

    /// A surface with a little light on its top edge.
    ///
    /// The light is the only gradient of the interface: it says that the
    /// surface is turned towards the light, and nothing more.
    pub fn raised(&self, element: Div, radius: Pixels, depth: Depth) -> Div {
        let mut element = element
            .bg(linear_gradient(
                180.,
                linear_color_stop(self.palette.highlight, 0.),
                linear_color_stop(self.palette.surface, 1.),
            ))
            .border_1()
            .border_color(self.palette.border)
            .rounded(radius);
        element.style().box_shadow = Some(depth.shadows(&self.palette));
        element
    }

    /// A groove: a surface that is pressed into the background.
    ///
    /// gpui has no shadow inside an element, so the groove is a step
    /// towards the background with a gradient that is dark at the top and
    /// light at the bottom.
    pub fn groove(&self, element: Div, radius: Pixels) -> Div {
        element
            .bg(linear_gradient(
                180.,
                linear_color_stop(self.palette.groove_dark, 0.),
                linear_color_stop(self.palette.groove_light, 1.),
            ))
            .rounded(radius)
    }

    /// The hairline that separates two things.
    pub fn hairline(&self) -> Div {
        div().h(px(1.)).bg(self.palette.border)
    }
}

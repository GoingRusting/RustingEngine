//! CSS-like values shared by editor GUI elements.

/// A width or height used by an editor element.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Length {
    /// Lets the element choose a size from its content.
    #[default]
    Auto,
    /// Uses an exact number of egui points, similar to CSS pixels.
    Px(f32),
    /// Uses part of the available size. `1.0` means 100 percent.
    Percent(f32),
    /// Uses all space that is currently available.
    Fill,
}

impl Length {
    /// Turns a style length into a usable size for the current panel.
    pub(crate) fn resolve(self, available: f32, automatic: f32) -> f32 {
        match self {
            Self::Auto => automatic,
            Self::Px(points) => points,
            Self::Percent(part) => available * part.clamp(0.0, 1.0),
            Self::Fill => available,
        }
        .max(0.0)
    }
}

/// Space on the four sides of an element.
///
/// The field names follow CSS order and make asymmetric layouts explicit.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Edges {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Edges {
    /// Uses the same space on every side.
    #[must_use]
    pub const fn all(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    /// Uses one value vertically and another horizontally.
    #[must_use]
    pub const fn symmetric(vertical: f32, horizontal: f32) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    pub(crate) fn horizontal(self) -> f32 {
        self.left + self.right
    }

    pub(crate) fn vertical(self) -> f32 {
        self.top + self.bottom
    }
}

/// Border around an editor element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderStyle {
    pub width: f32,
    pub color: egui::Color32,
    pub radius: f32,
}

/// Soft shadow behind an element, similar to CSS `box-shadow`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShadowStyle {
    pub offset: [i8; 2],
    pub blur: u8,
    pub spread: u8,
    pub color: egui::Color32,
}

impl ShadowStyle {
    pub(crate) fn as_egui(self) -> egui::epaint::Shadow {
        egui::epaint::Shadow {
            offset: self.offset,
            blur: self.blur,
            spread: self.spread,
            color: self.color,
        }
    }
}

impl Default for BorderStyle {
    fn default() -> Self {
        Self {
            width: 1.0,
            color: egui::Color32::TRANSPARENT,
            radius: 2.0,
        }
    }
}

/// Reusable visual and layout values for editor elements.
///
/// Public fields keep styling close to a small CSS declaration. Start with
/// `ElementStyle::default()` and change only the values an element needs.
#[derive(Clone, Debug, PartialEq)]
pub struct ElementStyle {
    pub width: Length,
    pub height: Length,
    pub min_width: f32,
    pub min_height: f32,
    pub max_width: f32,
    pub max_height: f32,
    pub margin: Edges,
    pub padding: Edges,
    pub gap: f32,
    pub background: Option<egui::Color32>,
    pub hover_background: Option<egui::Color32>,
    pub active_background: Option<egui::Color32>,
    pub text_color: Option<egui::Color32>,
    pub hover_text_color: Option<egui::Color32>,
    pub active_text_color: Option<egui::Color32>,
    pub font_size: Option<f32>,
    /// Places text inside the content box, similar to CSS text alignment.
    pub text_align: egui::Align2,
    pub border: BorderStyle,
    pub shadow: Option<ShadowStyle>,
}

impl Default for ElementStyle {
    fn default() -> Self {
        Self {
            width: Length::Auto,
            height: Length::Auto,
            min_width: 0.0,
            min_height: 0.0,
            max_width: f32::INFINITY,
            max_height: f32::INFINITY,
            margin: Edges::default(),
            padding: Edges::symmetric(3.0, 6.0),
            gap: 6.0,
            background: None,
            hover_background: None,
            active_background: None,
            text_color: None,
            hover_text_color: None,
            active_text_color: None,
            font_size: None,
            text_align: egui::Align2::CENTER_CENTER,
            border: BorderStyle::default(),
            shadow: None,
        }
    }
}

impl ElementStyle {
    /// Creates a style with an exact content size.
    #[must_use]
    pub fn fixed(width: f32, height: f32) -> Self {
        Self {
            width: Length::Px(width),
            height: Length::Px(height),
            ..Self::default()
        }
    }

    pub(crate) fn clamp_width(&self, width: f32) -> f32 {
        width.clamp(self.min_width, self.max_width.max(self.min_width))
    }

    pub(crate) fn clamp_height(&self, height: f32) -> f32 {
        height.clamp(self.min_height, self.max_height.max(self.min_height))
    }
}

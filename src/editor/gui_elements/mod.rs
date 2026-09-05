//! Reusable editor GUI elements with CSS-like styling.
//!
//! Egui controls layout immediately, so this module translates familiar style
//! values into exact egui rectangles. Editor panels should prefer these
//! elements over repeating width, padding, margin, and responsive calculations.

mod button;
mod combo_box;
mod forms;
mod icon_button;
mod style;
mod theme;

pub(crate) use button::button_with_sense;
pub use button::{button, ButtonProps};
pub use combo_box::{combo_box, ComboBoxProps, ComboBoxResponse};
pub use forms::{text_action, TextActionProps, TextActionResponse};
pub use icon_button::{icon_button, icon_button_sized};
pub use style::{BorderStyle, Edges, ElementStyle, Length, ShadowStyle};
pub use theme::EditorTheme;

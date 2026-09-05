//! Compact theme-aware button for code-drawn editor icons.

use super::super::icons::{paint_editor_icon, EditorIcon};
use super::EditorTheme;

pub fn icon_button(
    ui: &mut egui::Ui,
    icon: EditorIcon,
    selected: bool,
    enabled: bool,
) -> egui::Response {
    icon_button_sized(ui, icon, selected, enabled, egui::vec2(28.0, 28.0))
}

/// Draws an icon button at a caller-provided size.
pub fn icon_button_sized(
    ui: &mut egui::Ui,
    icon: EditorIcon,
    selected: bool,
    enabled: bool,
    size: egui::Vec2,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        size,
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    let response = if enabled {
        response
    } else {
        response.on_disabled_hover_text("Select an object first")
    };
    let visuals = ui.style().interact(&response);
    let fill = if selected {
        EditorTheme::ACCENT
    } else if response.hovered() {
        EditorTheme::BUTTON_HOVER
    } else {
        EditorTheme::BUTTON
    };
    ui.painter().rect(
        rect,
        4.0,
        fill,
        egui::Stroke::new(
            1.0_f32,
            if selected {
                EditorTheme::ACCENT_HOVER
            } else {
                EditorTheme::BORDER
            },
        ),
        egui::StrokeKind::Inside,
    );
    paint_editor_icon(
        ui.painter(),
        icon,
        rect.shrink(3.0),
        if enabled {
            visuals.text_color()
        } else {
            EditorTheme::TEXT_MUTED
        },
    );
    response
}

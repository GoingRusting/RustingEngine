//! Godot-style property rows: a muted label column on the left and one
//! full-width widget on the right. Every Inspector field goes through these so
//! all panels share one look.

use egui::emath::Numeric;
use egui::{DragValue, Ui};

use crate::editor::gui_elements::{icon_button_sized, EditorTheme};
use crate::editor::icons::paint_editor_icon;
use crate::editor::EditorIcon;

/// Draws one labeled row and returns what `add` returned.
pub fn property_row<R>(
    ui: &mut Ui,
    label: &str,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    let label_width = (ui.available_width() * 0.4).clamp(64.0, 150.0);
    ui.horizontal(|ui| {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(label_width, EditorTheme::ROW_HEIGHT),
            egui::Sense::hover(),
        );
        // Long labels are clipped to their column; the tooltip shows all.
        ui.painter_at(rect.shrink2(egui::vec2(4.0, 0.0))).text(
            rect.left_center() + egui::vec2(4.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::TextStyle::Body.resolve(ui.style()),
            EditorTheme::TEXT_MUTED,
        );
        response.on_hover_text(label);
        add(ui)
    })
    .inner
}

fn fill(ui: &mut Ui, widget: impl egui::Widget) -> egui::Response {
    ui.add_sized([ui.available_width(), EditorTheme::ROW_HEIGHT], widget)
}

/// Number field; build `value` with its range, speed, prefix, or suffix.
pub fn drag(ui: &mut Ui, label: &str, value: DragValue<'_>) -> bool {
    property_row(ui, label, |ui| fill(ui, value).changed())
}

/// Angle stored in radians and shown in degrees.
pub fn angle(
    ui: &mut Ui,
    label: &str,
    radians: &mut f32,
    degrees: std::ops::RangeInclusive<f32>,
) -> bool {
    let mut value = radians.to_degrees();
    let changed = drag(
        ui,
        label,
        DragValue::new(&mut value)
            .range(degrees)
            .speed(0.5)
            .suffix("°"),
    );
    if changed {
        *radians = value.to_radians();
    }
    changed
}

/// Three numbers with X/Y/Z stripes in the axis colors.
pub fn vec3<N: Numeric>(
    ui: &mut Ui,
    label: &str,
    values: &mut [N; 3],
    speed: f64,
) -> bool {
    property_row(ui, label, |ui| {
        let gap = 2.0;
        ui.spacing_mut().item_spacing.x = gap;
        let width = ((ui.available_width() - gap * 2.0) / 3.0).max(28.0);
        let mut changed = false;
        for (axis, value) in values.iter_mut().enumerate() {
            let response = ui.add_sized(
                [width, EditorTheme::ROW_HEIGHT],
                DragValue::new(value).speed(speed).max_decimals(3),
            );
            changed |= response.changed();
            let stripe = egui::Rect::from_min_size(
                response.rect.min + egui::vec2(1.0, 4.0),
                egui::vec2(2.0, response.rect.height() - 8.0),
            );
            ui.painter()
                .rect_filled(stripe, 1.0, EditorTheme::AXIS[axis]);
        }
        changed
    })
}

/// Linear RGB color with a picker.
pub fn color(ui: &mut Ui, label: &str, rgb: &mut [f32; 3]) -> bool {
    property_row(ui, label, |ui| {
        ui.spacing_mut().interact_size.x = ui.available_width();
        ui.color_edit_button_rgb(rgb).changed()
    })
}

pub fn checkbox(ui: &mut Ui, label: &str, value: &mut bool) -> bool {
    property_row(ui, label, |ui| ui.checkbox(value, "").changed())
}

pub fn text(ui: &mut Ui, label: &str, value: &mut String) -> bool {
    property_row(ui, label, |ui| {
        fill(ui, egui::TextEdit::singleline(value)).changed()
    })
}

/// Read-only value such as an asset handle key.
pub fn value(ui: &mut Ui, label: &str, value: &str) {
    property_row(ui, label, |ui| {
        ui.add(
            egui::Label::new(egui::RichText::new(value).monospace()).truncate(),
        )
        .on_hover_text(value);
    });
}

/// Drop-down over a fixed set of named values.
pub fn choice<T: PartialEq + Copy>(
    ui: &mut Ui,
    label: &str,
    value: &mut T,
    options: &[(T, &str)],
) -> bool {
    let selected = options
        .iter()
        .find(|(option, _)| option == value)
        .map_or("", |(_, name)| name);
    property_row(ui, label, |ui| {
        let mut changed = false;
        egui::ComboBox::from_id_salt(label)
            .width(ui.available_width())
            .selected_text(selected)
            .show_ui(ui, |ui| {
                for (option, name) in options {
                    changed |=
                        ui.selectable_value(value, *option, *name).changed();
                }
            });
        changed
    })
}

/// Godot-style collapsible section with a raised header bar. Returns true
/// when the header's remove button was clicked.
pub fn section(
    ui: &mut Ui,
    title: &str,
    removable: bool,
    body: impl FnOnce(&mut Ui),
) -> bool {
    let id = ui.make_persistent_id(("inspector_section", title));
    let mut state =
        egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            id,
            true,
        );
    let mut remove = false;
    egui::Frame::new()
        .fill(EditorTheme::PANEL_RAISED)
        .corner_radius(EditorTheme::RADIUS)
        .inner_margin(egui::Margin::symmetric(2, 1))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let remove_width = if removable { 24.0 } else { 0.0 };
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(
                        ui.available_width() - remove_width,
                        EditorTheme::ROW_HEIGHT - 2.0,
                    ),
                    egui::Sense::click(),
                );
                let arrow = egui::Rect::from_center_size(
                    egui::pos2(rect.left() + 9.0, rect.center().y),
                    egui::vec2(14.0, 14.0),
                );
                paint_editor_icon(
                    ui.painter(),
                    if state.is_open() {
                        EditorIcon::ChevronDown
                    } else {
                        EditorIcon::ChevronRight
                    },
                    arrow,
                    EditorTheme::TEXT_MUTED,
                );
                ui.painter().text(
                    egui::pos2(rect.left() + 20.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    title,
                    egui::FontId::proportional(13.0),
                    EditorTheme::TEXT,
                );
                if response.clicked() {
                    state.toggle(ui);
                }
                if removable {
                    remove = icon_button_sized(
                        ui,
                        EditorIcon::Close,
                        false,
                        true,
                        egui::vec2(20.0, 18.0),
                    )
                    .on_hover_text("Remove component")
                    .clicked();
                }
            });
        });
    state.store(ui.ctx());
    state.show_body_unindented(ui, |ui| {
        ui.add_space(2.0);
        body(ui);
        ui.add_space(4.0);
    });
    remove
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_rows_share_one_label_column() {
        let context = egui::Context::default();
        let mut starts = Vec::new();
        let _ = context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                let mut number = 1.0_f32;
                let mut values = [0.0_f32; 3];
                let mut name = String::new();
                starts.push(property_row(ui, "Short", |ui| {
                    fill(ui, DragValue::new(&mut number)).rect.left()
                }));
                starts.push(property_row(ui, "A much longer label", |ui| {
                    ui.available_rect_before_wrap().left()
                }));
                vec3(ui, "Position", &mut values, 0.1);
                text(ui, "Name", &mut name);
            });
        });
        assert_eq!(starts.len(), 2);
        assert_eq!(starts[0], starts[1]);
    }
}

//! Godot-style property rows: a muted label column on the left and one
//! full-width widget on the right. Every Inspector field goes through these so
//! all panels share one look.

use egui::emath::Numeric;
use egui::{DragValue, Ui};

use crate::editor::gui_elements::{icon_button_sized, EditorTheme};
use crate::editor::icons::paint_editor_icon;
use crate::editor::EditorIcon;

/// Gives the next property row drawn directly in `ui` a description, shown
/// under its label in the label's tooltip.
pub fn describe_next_row(ui: &Ui, doc: &'static str) {
    ui.data_mut(|data| {
        data.insert_temp(ui.id().with("property_row_doc"), doc);
    });
}

/// Takes the doc set by [`describe_next_row`], so rows that draw their own
/// label (such as collapsible sections) can show it too.
pub fn take_row_doc(ui: &Ui) -> Option<&'static str> {
    ui.data_mut(|data| {
        data.remove_temp::<&'static str>(ui.id().with("property_row_doc"))
    })
}

/// Draws one labeled row and returns what `add` returned.
pub fn property_row<R>(
    ui: &mut Ui,
    label: &str,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    let label_width = (ui.available_width() * 0.4).clamp(64.0, 150.0);
    let doc = take_row_doc(ui);
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
        match doc {
            Some(doc) => response.on_hover_text(format!("{label}\n{doc}")),
            None => response.on_hover_text(label),
        };
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

/// Color with alpha, unmultiplied.
pub fn color4(ui: &mut Ui, label: &str, rgba: &mut [f32; 4]) -> bool {
    property_row(ui, label, |ui| {
        ui.spacing_mut().interact_size.x = ui.available_width();
        ui.color_edit_button_rgba_unmultiplied(rgba).changed()
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
        .fill(EditorTheme::PANEL)
        .stroke(egui::Stroke::new(1.0_f32, EditorTheme::BORDER_SOFT))
        .corner_radius(egui::CornerRadius::same(5))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let open = state.is_open();
            let (band, response) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), EditorTheme::ROW_HEIGHT + 4.0),
                egui::Sense::click(),
            );
            let top = egui::CornerRadius {
                nw: 5,
                ne: 5,
                sw: if open { 0 } else { 5 },
                se: if open { 0 } else { 5 },
            };
            ui.painter().rect_filled(
                band,
                top,
                if response.hovered() {
                    EditorTheme::BUTTON
                } else {
                    EditorTheme::PANEL_RAISED
                },
            );
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    band.min + egui::vec2(0.0, 5.0),
                    egui::vec2(3.0, band.height() - 10.0),
                ),
                egui::CornerRadius::same(2),
                EditorTheme::ACCENT,
            );
            paint_editor_icon(
                ui.painter(),
                if open {
                    EditorIcon::ChevronDown
                } else {
                    EditorIcon::ChevronRight
                },
                egui::Rect::from_center_size(
                    egui::pos2(band.left() + 16.0, band.center().y),
                    egui::vec2(14.0, 14.0),
                ),
                EditorTheme::TEXT_MUTED,
            );
            ui.painter().text(
                egui::pos2(band.left() + 28.0, band.center().y),
                egui::Align2::LEFT_CENTER,
                title,
                egui::FontId::proportional(13.0),
                EditorTheme::TEXT_STRONG,
            );
            if removable {
                let button = egui::Rect::from_center_size(
                    egui::pos2(band.right() - 16.0, band.center().y),
                    egui::vec2(20.0, 18.0),
                );
                remove = ui
                    .scope_builder(
                        egui::UiBuilder::new().max_rect(button),
                        |ui| {
                            icon_button_sized(
                                ui,
                                EditorIcon::Close,
                                false,
                                true,
                                button.size(),
                            )
                            .on_hover_text("Remove component")
                            .clicked()
                        },
                    )
                    .inner;
            }
            if response.clicked() && !remove {
                state.toggle(ui);
            }
            state.store(ui.ctx());
            state.show_body_unindented(ui, |ui| {
                ui.painter().hline(
                    band.x_range(),
                    band.bottom(),
                    egui::Stroke::new(1.0_f32, EditorTheme::BORDER_SOFT),
                );
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(6, 4))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        body(ui);
                    });
            });
        });
    ui.add_space(4.0);
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

    #[test]
    fn a_described_row_shows_its_doc_in_the_label_tooltip() {
        let context = egui::Context::default();
        context.style_mut(|style| style.interaction.tooltip_delay = 0.0);
        let pointer = egui::pos2(20.0, 20.0);
        let mut tooltips = Vec::new();
        for frame in 0..4 {
            let input = egui::RawInput {
                events: vec![egui::Event::PointerMoved(pointer)],
                time: Some(f64::from(frame)),
                ..Default::default()
            };
            let output = context.run(input, |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let mut number = 1.0_f32;
                    describe_next_row(ui, "m/s along the ground");
                    drag(ui, "Speed", DragValue::new(&mut number));
                });
            });
            tooltips = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => {
                        Some(text.galley.text().to_owned())
                    }
                    _ => None,
                })
                .collect();
        }
        assert!(
            tooltips
                .iter()
                .any(|text| text == "Speed\nm/s along the ground"),
            "{tooltips:?}"
        );
    }
}

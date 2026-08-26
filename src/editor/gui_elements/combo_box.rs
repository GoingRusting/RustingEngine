//! Styled ComboBox element used by editor toolbars and forms.

use std::hash::Hash;

use super::{button, ButtonProps, ElementStyle};

/// Text, identity, and style used by one ComboBox.
pub struct ComboBoxProps<'a> {
    /// Stable ID that keeps this popup separate from other ComboBoxes.
    pub id_salt: egui::Id,
    /// Value currently shown while the popup is closed.
    pub selected_text: &'a str,
    /// Optional help shown while the pointer is over the closed control.
    pub tooltip: Option<&'a str>,
    /// False draws the control but prevents opening it.
    pub enabled: bool,
    /// CSS-like size, spacing, color, border, and shadow values.
    pub style: ElementStyle,
    /// Smallest width used by the opened list.
    pub popup_min_width: f32,
}

impl<'a> ComboBoxProps<'a> {
    /// Creates an enabled ComboBox with the default element style.
    #[must_use]
    pub fn new(id_salt: impl Hash, selected_text: &'a str) -> Self {
        Self {
            id_salt: egui::Id::new(id_salt),
            selected_text,
            tooltip: None,
            enabled: true,
            style: ElementStyle::default(),
            popup_min_width: 0.0,
        }
    }
}

/// Result returned after drawing a ComboBox and its optional popup.
pub struct ComboBoxResponse<R> {
    /// Response of the closed control. Its rectangle uses `ElementStyle`.
    pub response: egui::Response,
    /// Value returned by the popup contents while the popup is open.
    pub inner: Option<R>,
}

/// Draws a ComboBox using the same exact box calculation as custom buttons.
pub fn combo_box<R>(
    ui: &mut egui::Ui,
    props: ComboBoxProps<'_>,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> ComboBoxResponse<R> {
    let ComboBoxProps {
        id_salt,
        selected_text,
        tooltip,
        enabled,
        style,
        popup_min_width,
    } = props;
    // Resolve the local salt through this Ui. Two dock areas may use the same
    // component code, but each area has its own Ui ID and therefore its own
    // popup. Creating an Id directly from the salt would ignore that scope.
    let popup_id = ui.make_persistent_id(id_salt).with("popup");
    // Keep this ASCII-only because the default editor font may not contain
    // decorative arrow glyphs on every operating system.
    let button_text = format!("{selected_text}  v");
    let response = button(
        ui,
        ButtonProps {
            text: &button_text,
            tooltip,
            enabled,
            style,
        },
    );

    if enabled && response.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(popup_id));
    }
    let inner = egui::popup::popup_below_widget(
        ui,
        popup_id,
        &response,
        egui::popup::PopupCloseBehavior::CloseOnClick,
        |ui| {
            ui.set_min_width(popup_min_width.max(response.rect.width()));
            add_contents(ui)
        },
    );

    ComboBoxResponse { response, inner }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::gui_elements::EditorTheme;

    #[test]
    fn fixed_combo_box_uses_the_requested_control_size() {
        let context = egui::Context::default();
        let mut control_rect = None;

        let _ = context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                control_rect = Some(
                    combo_box(
                        ui,
                        ComboBoxProps {
                            style: ElementStyle::fixed(76.0, 28.0),
                            ..ComboBoxProps::new("profile", "Debug")
                        },
                        |_| {},
                    )
                    .response
                    .rect,
                );
            });
        });

        assert_eq!(control_rect.unwrap().size(), egui::vec2(76.0, 28.0));
    }

    #[test]
    fn toolbar_combo_box_aligns_with_toolbar_button() {
        let context = egui::Context::default();
        let mut control_rects = None;

        let _ = context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                ui.horizontal(|ui| {
                    let button =
                        EditorTheme::toolbar_button(ui, "Play", false, true);
                    let combo = EditorTheme::toolbar_combo_box(
                        ui,
                        "profile",
                        "Debug",
                        90.0,
                        |_| {},
                    );
                    control_rects = Some((button.rect, combo.response.rect));
                });
            });
        });

        let (button, combo) = control_rects.unwrap();
        assert_eq!(button.height(), combo.height());
        assert_eq!(button.top(), combo.top());
        assert_eq!(button.bottom(), combo.bottom());
    }

    #[test]
    fn repeated_local_salts_are_unique_inside_different_ui_scopes() {
        let context = egui::Context::default();
        let mut popup_ids = Vec::new();

        let _ = context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                for area_id in [1_u64, 2_u64] {
                    ui.push_id(("dock_area", area_id), |ui| {
                        popup_ids.push(
                            ui.make_persistent_id(egui::Id::new("selector"))
                                .with("popup"),
                        );
                        combo_box(
                            ui,
                            ComboBoxProps::new("selector", "Scene"),
                            |_| {},
                        );
                    });
                }
            });
        });

        assert_ne!(popup_ids[0], popup_ids[1]);
    }
}

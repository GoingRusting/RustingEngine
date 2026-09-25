//! Shared modern editor palette and component styles.

use super::{
    button, button_with_sense, combo_box, BorderStyle, ButtonProps,
    ComboBoxProps, ComboBoxResponse, Edges, ElementStyle,
};
use crate::editor::icons::paint_editor_icon;
use crate::editor::EditorIcon;

/// Colors and ready-to-use styles for the RustingEngine editor.
pub struct EditorTheme;

impl EditorTheme {
    // Blender + Godot: neutral blue-gray panels (Godot), compact areas and
    // blue selection with an orange active object (Blender).
    /// Gaps between areas and the window behind them.
    pub const BACKGROUND: egui::Color32 = egui::Color32::from_rgb(24, 26, 31);
    pub const PANEL: egui::Color32 = egui::Color32::from_rgb(37, 40, 47);
    /// Area headers, section headers, and raised rows.
    pub const PANEL_RAISED: egui::Color32 = egui::Color32::from_rgb(46, 50, 59);
    pub const INPUT: egui::Color32 = egui::Color32::from_rgb(28, 30, 36);
    pub const BORDER: egui::Color32 = egui::Color32::from_rgb(58, 63, 74);
    pub const BORDER_SOFT: egui::Color32 = egui::Color32::from_rgb(47, 51, 60);
    pub const TEXT: egui::Color32 = egui::Color32::from_rgb(220, 223, 229);
    pub const TEXT_MUTED: egui::Color32 =
        egui::Color32::from_rgb(138, 145, 158);
    pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(71, 114, 179);
    pub const ACCENT_HOVER: egui::Color32 =
        egui::Color32::from_rgb(87, 131, 198);
    pub const ACCENT_ACTIVE: egui::Color32 =
        egui::Color32::from_rgb(59, 97, 155);
    /// Highlight for the primary (active) object, as in Blender.
    pub const ACTIVE_OBJECT: egui::Color32 =
        egui::Color32::from_rgb(255, 160, 40);
    /// Text on filled accent controls.
    pub const TEXT_STRONG: egui::Color32 = egui::Color32::WHITE;
    /// Notes about limits, unsaved state, and partial support.
    pub const WARNING: egui::Color32 = egui::Color32::from_rgb(235, 200, 90);
    pub const ERROR: egui::Color32 = egui::Color32::from_rgb(255, 128, 128);
    /// Outline of toggled accent controls and link-like text.
    pub const LINK: egui::Color32 = egui::Color32::from_rgb(130, 190, 255);
    pub const BUTTON: egui::Color32 = egui::Color32::from_rgb(53, 57, 67);
    pub const BUTTON_HOVER: egui::Color32 = egui::Color32::from_rgb(65, 70, 82);
    /// X, Y, and Z axis colors used by vector fields and gizmos.
    pub const AXIS: [egui::Color32; 3] = [
        egui::Color32::from_rgb(222, 72, 72),
        egui::Color32::from_rgb(128, 190, 40),
        egui::Color32::from_rgb(60, 132, 228),
    ];
    /// Height of one control or property row.
    pub const ROW_HEIGHT: f32 = 22.0;
    pub const RADIUS: u8 = 3;

    /// Style shared by controls placed on the main editor toolbar.
    fn toolbar_control_style(selected: bool) -> ElementStyle {
        ElementStyle {
            height: super::Length::Px(Self::ROW_HEIGHT),
            margin: Edges::symmetric(1.0, 1.0),
            padding: Edges::symmetric(2.0, 8.0),
            background: Some(if selected {
                Self::ACCENT
            } else {
                Self::BUTTON
            }),
            hover_background: Some(Self::ACCENT_HOVER),
            active_background: Some(Self::ACCENT_ACTIVE),
            text_color: Some(Self::TEXT),
            border: BorderStyle {
                width: 1.0,
                color: if selected {
                    Self::ACCENT_HOVER
                } else {
                    Self::BORDER
                },
                radius: f32::from(Self::RADIUS),
            },
            ..ElementStyle::default()
        }
    }

    /// Applies the palette to native egui controls and windows.
    pub fn apply(context: &egui::Context) {
        let mut style = (*context.style()).clone();
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(8.0, 3.0);
        style.spacing.interact_size.y = Self::ROW_HEIGHT;
        style.spacing.indent = 14.0;
        style.visuals = egui::Visuals::dark();
        style.visuals.override_text_color = Some(Self::TEXT);
        style.visuals.panel_fill = Self::BACKGROUND;
        style.visuals.window_fill = Self::PANEL;
        style.visuals.extreme_bg_color = Self::INPUT;
        style.visuals.faint_bg_color = Self::PANEL_RAISED;
        style.visuals.code_bg_color = Self::INPUT;
        style.visuals.selection.bg_fill = Self::ACCENT;
        style.visuals.selection.stroke = egui::Stroke::new(1.0_f32, Self::TEXT);
        style.visuals.window_corner_radius = egui::CornerRadius::same(6);
        style.visuals.menu_corner_radius = egui::CornerRadius::same(4);
        style.visuals.window_stroke = egui::Stroke::new(1.0_f32, Self::BORDER);
        style.visuals.window_shadow = egui::epaint::Shadow {
            offset: [0, 4],
            blur: 16,
            spread: 0,
            color: egui::Color32::from_black_alpha(110),
        };
        style.visuals.popup_shadow = egui::epaint::Shadow {
            offset: [0, 3],
            blur: 10,
            spread: 0,
            color: egui::Color32::from_black_alpha(100),
        };
        style.visuals.widgets.inactive.weak_bg_fill = Self::BUTTON;
        style.visuals.widgets.inactive.bg_fill = Self::BUTTON;
        style.visuals.widgets.inactive.bg_stroke =
            egui::Stroke::new(1.0_f32, Self::BORDER);
        style.visuals.widgets.inactive.corner_radius =
            egui::CornerRadius::same(Self::RADIUS);
        style.visuals.widgets.hovered.weak_bg_fill = Self::BUTTON_HOVER;
        style.visuals.widgets.hovered.bg_fill = Self::BUTTON_HOVER;
        style.visuals.widgets.hovered.bg_stroke =
            egui::Stroke::new(1.0_f32, Self::ACCENT);
        style.visuals.widgets.hovered.corner_radius =
            egui::CornerRadius::same(Self::RADIUS);
        style.visuals.widgets.active.weak_bg_fill = Self::ACCENT_ACTIVE;
        style.visuals.widgets.active.bg_fill = Self::ACCENT_ACTIVE;
        style.visuals.widgets.active.bg_stroke =
            egui::Stroke::new(1.0_f32, Self::ACCENT_HOVER);
        style.visuals.widgets.active.corner_radius =
            egui::CornerRadius::same(Self::RADIUS);
        style.visuals.widgets.noninteractive.bg_stroke =
            egui::Stroke::new(1.0_f32, Self::BORDER_SOFT);
        style.visuals.collapsing_header_frame = true;
        style.visuals.indent_has_left_vline = false;
        context.set_style(style);
    }

    /// Standard toolbar button with optional blue selected state.
    pub fn toolbar_button(
        ui: &mut egui::Ui,
        text: &str,
        selected: bool,
        enabled: bool,
    ) -> egui::Response {
        let style = Self::toolbar_control_style(selected);
        button(
            ui,
            ButtonProps {
                text,
                tooltip: None,
                enabled,
                style,
            },
        )
    }

    /// Toolbar button with a code-drawn icon before its text.
    pub fn toolbar_icon_button(
        ui: &mut egui::Ui,
        text: &str,
        icon: EditorIcon,
        width: f32,
        enabled: bool,
    ) -> egui::Response {
        let label = format!("    {text}");
        let response = button(
            ui,
            ButtonProps {
                text: &label,
                tooltip: None,
                enabled,
                style: ElementStyle {
                    width: super::Length::Px(width),
                    ..Self::toolbar_control_style(false)
                },
            },
        );
        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(response.rect.left() + 14.0, response.rect.center().y),
            egui::vec2(15.0, 15.0),
        );
        paint_editor_icon(
            ui.painter(),
            icon,
            icon_rect,
            ui.style().interact(&response).text_color(),
        );
        response
    }

    /// ComboBox that aligns exactly with buttons in the main toolbar.
    pub fn toolbar_combo_box<R>(
        ui: &mut egui::Ui,
        id_salt: impl std::hash::Hash,
        selected_text: &str,
        width: f32,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> ComboBoxResponse<R> {
        combo_box(
            ui,
            ComboBoxProps {
                style: ElementStyle {
                    width: super::Length::Px(width),
                    ..Self::toolbar_control_style(false)
                },
                popup_min_width: width,
                ..ComboBoxProps::new(id_salt, selected_text)
            },
            add_contents,
        )
    }

    /// ComboBox with separate widths for its closed control and opened panel.
    pub fn toolbar_combo_box_with_popup<R>(
        ui: &mut egui::Ui,
        id_salt: impl std::hash::Hash,
        selected_text: &str,
        control_width: f32,
        popup_width: f32,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> ComboBoxResponse<R> {
        combo_box(
            ui,
            ComboBoxProps {
                style: ElementStyle {
                    width: super::Length::Px(control_width),
                    ..Self::toolbar_control_style(false)
                },
                popup_min_width: popup_width,
                ..ComboBoxProps::new(id_salt, selected_text)
            },
            add_contents,
        )
    }

    /// Toolbar ComboBox with a code-drawn icon before its label.
    pub fn toolbar_icon_combo_box_with_popup<R>(
        ui: &mut egui::Ui,
        id_salt: impl std::hash::Hash,
        selected_text: &str,
        icon: super::super::EditorIcon,
        control_width: f32,
        popup_width: f32,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> ComboBoxResponse<R> {
        combo_box(
            ui,
            ComboBoxProps {
                leading_icon: Some(icon),
                style: ElementStyle {
                    width: super::Length::Px(control_width),
                    ..Self::toolbar_control_style(false)
                },
                popup_min_width: popup_width,
                ..ComboBoxProps::new(id_salt, selected_text)
            },
            add_contents,
        )
    }

    /// Dropdown menu used to group less frequent toolbar actions.
    pub fn toolbar_menu<R>(
        ui: &mut egui::Ui,
        id_salt: impl std::hash::Hash,
        title: &str,
        control_width: f32,
        popup_width: f32,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> ComboBoxResponse<R> {
        Self::toolbar_combo_box_with_popup(
            ui,
            id_salt,
            title,
            control_width,
            popup_width,
            add_contents,
        )
    }

    /// Quiet label that separates groups inside one dropdown menu.
    pub fn menu_section(ui: &mut egui::Ui, text: &str) {
        ui.add_space(5.0);
        ui.label(
            egui::RichText::new(text)
                .size(11.0)
                .color(Self::TEXT_MUTED)
                .strong(),
        );
        ui.add_space(2.0);
    }

    /// Full-width action with the same styling in every dropdown menu.
    pub fn menu_action(
        ui: &mut egui::Ui,
        text: &str,
        enabled: bool,
    ) -> egui::Response {
        Self::menu_item(ui, text, false, enabled)
    }

    /// Full-width selectable value used inside styled dropdowns.
    pub fn menu_choice(
        ui: &mut egui::Ui,
        text: &str,
        selected: bool,
        enabled: bool,
    ) -> egui::Response {
        Self::menu_item(ui, text, selected, enabled)
    }

    /// Shared renderer for normal actions and selected menu values.
    fn menu_item(
        ui: &mut egui::Ui,
        text: &str,
        selected: bool,
        enabled: bool,
    ) -> egui::Response {
        Self::menu_item_with_sense(
            ui,
            text,
            selected,
            enabled,
            egui::Sense::click(),
        )
    }

    fn menu_item_with_sense(
        ui: &mut egui::Ui,
        text: &str,
        selected: bool,
        enabled: bool,
        sense: egui::Sense,
    ) -> egui::Response {
        button_with_sense(
            ui,
            ButtonProps {
                text,
                tooltip: None,
                enabled,
                style: ElementStyle {
                    width: super::Length::Fill,
                    height: super::Length::Px(Self::ROW_HEIGHT),
                    margin: Edges::symmetric(0.0, 0.0),
                    padding: Edges::symmetric(2.0, 8.0),
                    // Flat list rows: no fill until hovered or selected.
                    background: Some(if selected {
                        Self::ACCENT_ACTIVE
                    } else {
                        egui::Color32::TRANSPARENT
                    }),
                    hover_background: Some(if selected {
                        Self::ACCENT
                    } else {
                        Self::BUTTON
                    }),
                    active_background: Some(Self::ACCENT_ACTIVE),
                    text_color: Some(Self::TEXT),
                    text_align: egui::Align2::LEFT_CENTER,
                    // Any opaque color with zero width: a transparent color
                    // would fall back to the egui widget stroke.
                    border: BorderStyle {
                        width: 0.0,
                        color: Self::PANEL,
                        radius: f32::from(Self::RADIUS),
                    },
                    ..ElementStyle::default()
                },
            },
            sense,
        )
    }

    /// One full-width row in a parent-child tree. `primary` marks the
    /// active object of a multi-selection; `icon` is drawn before the text.
    pub fn tree_row(
        ui: &mut egui::Ui,
        text: &str,
        depth: usize,
        selected: bool,
        primary: bool,
        icon: Option<EditorIcon>,
    ) -> egui::Response {
        const INDENT: f32 = 18.0;
        const ICON: f32 = 16.0;
        let text_left = if icon.is_some() {
            8.0 + ICON + 4.0
        } else {
            8.0
        };
        let response = ui
            .horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(depth as f32 * INDENT);
                button_with_sense(
                    ui,
                    ButtonProps {
                        text,
                        tooltip: None,
                        enabled: true,
                        style: ElementStyle {
                            width: super::Length::Fill,
                            height: super::Length::Px(Self::ROW_HEIGHT),
                            margin: Edges::all(0.0),
                            padding: Edges {
                                top: 2.0,
                                right: 8.0,
                                bottom: 2.0,
                                left: text_left,
                            },
                            background: Some(if primary {
                                Self::ACCENT
                            } else if selected {
                                Self::ACCENT_ACTIVE
                            } else {
                                egui::Color32::TRANSPARENT
                            }),
                            hover_background: Some(if selected || primary {
                                Self::ACCENT_HOVER
                            } else {
                                Self::BUTTON
                            }),
                            active_background: Some(Self::ACCENT_ACTIVE),
                            text_color: Some(Self::TEXT),
                            text_align: egui::Align2::LEFT_CENTER,
                            border: BorderStyle {
                                width: 0.0,
                                color: Self::PANEL,
                                radius: f32::from(Self::RADIUS),
                            },
                            ..ElementStyle::default()
                        },
                    },
                    egui::Sense::click_and_drag(),
                )
            })
            .inner;
        if let Some(icon) = icon {
            let rect = egui::Rect::from_center_size(
                egui::pos2(
                    response.rect.left() + 8.0 + ICON * 0.5,
                    response.rect.center().y,
                ),
                egui::vec2(ICON, ICON),
            );
            // The active object's icon uses Blender's active-object orange.
            let color = if primary {
                Self::ACTIVE_OBJECT
            } else {
                Self::TEXT_MUTED
            };
            paint_editor_icon(ui.painter(), icon, rect, color);
        }

        // Lines make parent depth visible without depending on icon fonts.
        if depth > 0 {
            let painter = ui.painter();
            for level in 0..depth {
                let x = response.rect.left() - (depth - level) as f32 * INDENT
                    + INDENT * 0.5;
                painter.line_segment(
                    [
                        egui::pos2(x, response.rect.top()),
                        egui::pos2(x, response.rect.bottom()),
                    ],
                    egui::Stroke::new(1.0_f32, Self::BORDER),
                );
            }
            let branch_x = response.rect.left() - INDENT * 0.5;
            painter.line_segment(
                [
                    egui::pos2(branch_x, response.rect.center().y),
                    egui::pos2(
                        response.rect.left() - 3.0,
                        response.rect.center().y,
                    ),
                ],
                egui::Stroke::new(1.0_f32, Self::BORDER),
            );
        }
        response
    }

    /// Compact button used by dock headers.
    pub fn dock_button(
        ui: &mut egui::Ui,
        symbol: &str,
        tooltip: &str,
    ) -> egui::Response {
        let style = ElementStyle {
            padding: Edges::all(0.0),
            font_size: Some(11.0),
            background: Some(Self::BUTTON),
            hover_background: Some(Self::ACCENT),
            active_background: Some(Self::ACCENT_ACTIVE),
            border: BorderStyle {
                width: 1.0,
                color: Self::BORDER,
                radius: 4.0,
            },
            ..ElementStyle::fixed(22.0, 20.0)
        };
        button(
            ui,
            ButtonProps {
                text: symbol,
                tooltip: Some(tooltip),
                enabled: true,
                style,
            },
        )
    }
}

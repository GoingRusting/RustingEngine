//! Shared modern editor palette and component styles.

use super::{
    button, combo_box, BorderStyle, ButtonProps, ComboBoxProps,
    ComboBoxResponse, Edges, ElementStyle, ShadowStyle,
};

/// Colors and ready-to-use styles for the RustingEngine editor.
pub struct EditorTheme;

impl EditorTheme {
    pub const BACKGROUND: egui::Color32 = egui::Color32::from_rgb(12, 15, 22);
    pub const PANEL: egui::Color32 = egui::Color32::from_rgb(20, 24, 34);
    pub const PANEL_RAISED: egui::Color32 = egui::Color32::from_rgb(27, 32, 45);
    pub const INPUT: egui::Color32 = egui::Color32::from_rgb(15, 19, 28);
    pub const BORDER: egui::Color32 = egui::Color32::from_rgb(45, 52, 68);
    pub const BORDER_SOFT: egui::Color32 = egui::Color32::from_rgb(34, 40, 54);
    pub const TEXT: egui::Color32 = egui::Color32::from_rgb(226, 231, 241);
    pub const TEXT_MUTED: egui::Color32 =
        egui::Color32::from_rgb(145, 154, 173);
    pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(70, 125, 255);
    pub const ACCENT_HOVER: egui::Color32 =
        egui::Color32::from_rgb(88, 140, 255);
    pub const ACCENT_ACTIVE: egui::Color32 =
        egui::Color32::from_rgb(54, 105, 225);
    pub const BUTTON: egui::Color32 = egui::Color32::from_rgb(31, 37, 51);
    pub const BUTTON_HOVER: egui::Color32 = egui::Color32::from_rgb(42, 50, 68);

    /// Style shared by controls placed on the main editor toolbar.
    fn toolbar_control_style(selected: bool) -> ElementStyle {
        ElementStyle {
            height: super::Length::Px(28.0),
            margin: Edges::symmetric(2.0, 1.0),
            padding: Edges::symmetric(4.0, 10.0),
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
                radius: 5.0,
            },
            shadow: selected.then_some(ShadowStyle {
                offset: [0, 2],
                blur: 8,
                spread: 0,
                color: egui::Color32::from_black_alpha(80),
            }),
            ..ElementStyle::default()
        }
    }

    /// Applies the palette to native egui controls and windows.
    pub fn apply(context: &egui::Context) {
        let mut style = (*context.style()).clone();
        style.spacing.item_spacing = egui::vec2(8.0, 7.0);
        style.spacing.button_padding = egui::vec2(11.0, 6.0);
        style.spacing.indent = 18.0;
        style.visuals = egui::Visuals::dark();
        style.visuals.override_text_color = Some(Self::TEXT);
        style.visuals.panel_fill = Self::BACKGROUND;
        style.visuals.window_fill = Self::PANEL;
        style.visuals.extreme_bg_color = Self::INPUT;
        style.visuals.faint_bg_color = Self::PANEL_RAISED;
        style.visuals.code_bg_color = Self::INPUT;
        style.visuals.selection.bg_fill = Self::ACCENT;
        style.visuals.selection.stroke = egui::Stroke::new(1.0, Self::TEXT);
        style.visuals.window_corner_radius = egui::CornerRadius::same(8);
        style.visuals.menu_corner_radius = egui::CornerRadius::same(7);
        style.visuals.window_stroke = egui::Stroke::new(1.0, Self::BORDER);
        style.visuals.window_shadow = egui::epaint::Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 2,
            color: egui::Color32::from_black_alpha(120),
        };
        style.visuals.popup_shadow = egui::epaint::Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 1,
            color: egui::Color32::from_black_alpha(110),
        };
        style.visuals.widgets.inactive.weak_bg_fill = Self::BUTTON;
        style.visuals.widgets.inactive.bg_fill = Self::BUTTON;
        style.visuals.widgets.inactive.bg_stroke =
            egui::Stroke::new(1.0, Self::BORDER);
        style.visuals.widgets.inactive.corner_radius =
            egui::CornerRadius::same(5);
        style.visuals.widgets.hovered.weak_bg_fill = Self::BUTTON_HOVER;
        style.visuals.widgets.hovered.bg_fill = Self::BUTTON_HOVER;
        style.visuals.widgets.hovered.bg_stroke =
            egui::Stroke::new(1.0, Self::ACCENT);
        style.visuals.widgets.hovered.corner_radius =
            egui::CornerRadius::same(5);
        style.visuals.widgets.active.weak_bg_fill = Self::ACCENT_ACTIVE;
        style.visuals.widgets.active.bg_fill = Self::ACCENT_ACTIVE;
        style.visuals.widgets.active.bg_stroke =
            egui::Stroke::new(1.0, Self::ACCENT_HOVER);
        style.visuals.widgets.active.corner_radius =
            egui::CornerRadius::same(5);
        style.visuals.widgets.noninteractive.bg_stroke =
            egui::Stroke::new(1.0, Self::BORDER_SOFT);
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
        button(
            ui,
            ButtonProps {
                text,
                tooltip: None,
                enabled,
                style: ElementStyle {
                    width: super::Length::Fill,
                    height: super::Length::Px(32.0),
                    margin: Edges::symmetric(1.0, 0.0),
                    padding: Edges::symmetric(6.0, 12.0),
                    background: Some(if selected {
                        Self::ACCENT_ACTIVE
                    } else {
                        Self::PANEL_RAISED
                    }),
                    hover_background: Some(Self::ACCENT),
                    active_background: Some(Self::ACCENT_ACTIVE),
                    text_color: Some(Self::TEXT),
                    text_align: egui::Align2::LEFT_CENTER,
                    border: BorderStyle {
                        width: 1.0,
                        color: if selected {
                            Self::ACCENT_HOVER
                        } else {
                            Self::BORDER_SOFT
                        },
                        radius: 5.0,
                    },
                    ..ElementStyle::default()
                },
            },
        )
    }

    /// One full-width row in a parent-child tree.
    pub fn tree_row(
        ui: &mut egui::Ui,
        text: &str,
        depth: usize,
        selected: bool,
    ) -> egui::Response {
        const INDENT: f32 = 18.0;
        let response = ui
            .horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(depth as f32 * INDENT);
                Self::menu_choice(ui, text, selected, true)
            })
            .inner;

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
                    egui::Stroke::new(1.0, Self::BORDER),
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
                egui::Stroke::new(1.0, Self::BORDER),
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

//! Styled button element.

use super::ElementStyle;

/// Parameters used to draw one button.
pub struct ButtonProps<'a> {
    pub text: &'a str,
    pub tooltip: Option<&'a str>,
    pub enabled: bool,
    pub style: ElementStyle,
}

impl<'a> ButtonProps<'a> {
    /// Creates an enabled button with default styling.
    #[must_use]
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            tooltip: None,
            enabled: true,
            style: ElementStyle::default(),
        }
    }
}

/// Draws a button whose box follows [`ElementStyle`] values.
pub fn button(ui: &mut egui::Ui, props: ButtonProps<'_>) -> egui::Response {
    let ButtonProps {
        text,
        tooltip,
        enabled,
        style,
    } = props;
    let font_id = style.font_size.map_or_else(
        || egui::TextStyle::Button.resolve(ui.style()),
        egui::FontId::proportional,
    );
    let fallback_color = ui.visuals().widgets.inactive.text_color();
    let text_color = style.text_color.unwrap_or(fallback_color);
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        font_id.clone(),
        text_color,
    );
    let automatic_width = galley.size().x + style.padding.horizontal();
    let automatic_height = galley.size().y + style.padding.vertical();
    // Margins live outside the styled box, so Fill must reserve room for them.
    let available = ui.available_size()
        - egui::vec2(style.margin.horizontal(), style.margin.vertical());
    let content_width =
        style.clamp_width(style.width.resolve(available.x, automatic_width));
    let content_height =
        style.clamp_height(style.height.resolve(available.y, automatic_height));
    let outer_size = egui::vec2(
        content_width + style.margin.horizontal(),
        content_height + style.margin.vertical(),
    );
    let (id, outer_rect) = ui.allocate_space(outer_size);
    let button_rect = egui::Rect::from_min_max(
        outer_rect.min + egui::vec2(style.margin.left, style.margin.top),
        outer_rect.max - egui::vec2(style.margin.right, style.margin.bottom),
    );
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let response = ui.interact(button_rect, id, sense);

    if ui.is_rect_visible(button_rect) {
        let visuals = if enabled {
            ui.style().interact(&response)
        } else {
            &ui.visuals().widgets.noninteractive
        };
        let background = if response.is_pointer_button_down_on() {
            style
                .active_background
                .or(style.hover_background)
                .or(style.background)
                .unwrap_or(visuals.weak_bg_fill)
        } else if response.hovered() {
            style
                .hover_background
                .or(style.background)
                .unwrap_or(visuals.weak_bg_fill)
        } else {
            style.background.unwrap_or(visuals.weak_bg_fill)
        };
        let paint_text_color = if response.is_pointer_button_down_on() {
            style
                .active_text_color
                .or(style.hover_text_color)
                .or(style.text_color)
                .unwrap_or(visuals.text_color())
        } else if response.hovered() {
            style
                .hover_text_color
                .or(style.text_color)
                .unwrap_or(visuals.text_color())
        } else {
            style.text_color.unwrap_or(visuals.text_color())
        };
        let border = if style.border.color == egui::Color32::TRANSPARENT {
            visuals.bg_stroke
        } else {
            egui::Stroke::new(style.border.width, style.border.color)
        };
        if let Some(shadow) = style.shadow {
            ui.painter().add(
                shadow.as_egui().as_shape(button_rect, style.border.radius),
            );
        }
        ui.painter().rect(
            button_rect,
            style.border.radius,
            background,
            border,
            egui::StrokeKind::Inside,
        );
        let paint_galley = ui.painter().layout_no_wrap(
            text.to_owned(),
            font_id,
            paint_text_color,
        );
        let text_rect = egui::Rect::from_min_max(
            button_rect.min + egui::vec2(style.padding.left, style.padding.top),
            button_rect.max
                - egui::vec2(style.padding.right, style.padding.bottom),
        );
        let text_position = style
            .text_align
            .align_size_within_rect(paint_galley.size(), text_rect)
            .min;
        ui.painter().with_clip_rect(button_rect).galley(
            text_position,
            paint_galley,
            paint_text_color,
        );
    }

    let response = if enabled {
        response
    } else {
        response.on_disabled_hover_text("This action is currently unavailable")
    };
    match tooltip {
        Some(text) => response.on_hover_text(text),
        None => response,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::gui_elements::{Edges, Length};

    #[test]
    fn fixed_button_keeps_content_size_and_margin() {
        let context = egui::Context::default();
        let mut button_rect = None;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(200.0, 100.0),
            )),
            ..Default::default()
        };

        let _ = context.run(input, |context| {
            egui::CentralPanel::default().show(context, |ui| {
                let mut style = ElementStyle::fixed(80.0, 24.0);
                style.margin = Edges::all(4.0);
                button_rect = Some(
                    button(
                        ui,
                        ButtonProps {
                            text: "Button",
                            tooltip: None,
                            enabled: true,
                            style,
                        },
                    )
                    .rect,
                );
            });
        });

        assert_eq!(button_rect.unwrap().size(), egui::vec2(80.0, 24.0));
    }

    #[test]
    fn fill_button_reserves_space_for_its_margin() {
        let context = egui::Context::default();
        let mut used_width = 0.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(200.0, 100.0),
            )),
            ..Default::default()
        };

        let _ = context.run(input, |context| {
            egui::CentralPanel::default().show(context, |ui| {
                let available = ui.available_width();
                let style = ElementStyle {
                    width: Length::Fill,
                    margin: Edges::symmetric(0.0, 5.0),
                    ..ElementStyle::default()
                };
                let response = button(
                    ui,
                    ButtonProps {
                        text: "Fill",
                        tooltip: None,
                        enabled: true,
                        style,
                    },
                );
                used_width = response.rect.width() + 10.0;
                assert_eq!(used_width, available);
            });
        });
    }
}

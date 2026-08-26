//! Responsive form elements used by editor panels.

use super::{button, ButtonProps, ElementStyle, Length};

/// Styling and text for a text field followed by an action button.
#[derive(Clone, Debug)]
pub struct TextActionProps<'a> {
    pub action_label: &'a str,
    pub tooltip: Option<&'a str>,
    pub breakpoint: f32,
    pub style: ElementStyle,
    pub button_style: ElementStyle,
}

impl<'a> TextActionProps<'a> {
    /// Creates a responsive row with practical editor defaults.
    #[must_use]
    pub fn new(action_label: &'a str) -> Self {
        Self {
            action_label,
            tooltip: None,
            breakpoint: 150.0,
            style: ElementStyle {
                width: Length::Fill,
                ..ElementStyle::default()
            },
            button_style: ElementStyle::fixed(64.0, 24.0),
        }
    }
}

/// Result returned by a responsive text action.
pub struct TextActionResponse {
    pub text_response: egui::Response,
    pub action_response: egui::Response,
}

impl TextActionResponse {
    /// True during the frame in which the action button was clicked.
    #[must_use]
    pub fn clicked(&self) -> bool {
        self.action_response.clicked()
    }
}

/// Draws one line on wide panels and two lines on narrow panels.
pub fn text_action(
    ui: &mut egui::Ui,
    text: &mut String,
    props: TextActionProps<'_>,
) -> TextActionResponse {
    let gap = props.style.gap.max(0.0);
    let row_height = props
        .style
        .height
        .resolve(ui.spacing().interact_size.y, ui.spacing().interact_size.y);
    if ui.available_width() < props.breakpoint {
        let text_response = ui.add_sized(
            [ui.available_width().max(0.0), row_height],
            egui::TextEdit::singleline(text),
        );
        ui.add_space(gap);
        let action_response = button(
            ui,
            ButtonProps {
                text: props.action_label,
                tooltip: props.tooltip,
                enabled: true,
                style: ElementStyle {
                    width: Length::Fill,
                    ..props.button_style
                },
            },
        );
        TextActionResponse {
            text_response,
            action_response,
        }
    } else {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let button_width =
                props.button_style.width.resolve(ui.available_width(), 64.0)
                    + props.button_style.margin.horizontal();
            let text_width =
                (ui.available_width() - button_width - gap).max(0.0);
            let text_response = ui.add_sized(
                [text_width, row_height],
                egui::TextEdit::singleline(text),
            );
            let action_response = button(
                ui,
                ButtonProps {
                    text: props.action_label,
                    tooltip: props.tooltip,
                    enabled: true,
                    style: props.button_style,
                },
            );
            TextActionResponse {
                text_response,
                action_response,
            }
        })
        .inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control_rects(width: f32) -> (egui::Rect, egui::Rect) {
        let context = egui::Context::default();
        let mut text = String::new();
        let mut rects = None;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 120.0),
            )),
            ..Default::default()
        };

        let _ = context.run(input, |context| {
            egui::CentralPanel::default().show(context, |ui| {
                let response =
                    text_action(ui, &mut text, TextActionProps::new("Rename"));
                rects = Some((
                    response.text_response.rect,
                    response.action_response.rect,
                ));
            });
        });
        rects.unwrap()
    }

    #[test]
    fn text_action_changes_layout_at_its_breakpoint() {
        let (wide_text, wide_button) = control_rects(300.0);
        assert!(wide_button.left() > wide_text.right());

        let (narrow_text, narrow_button) = control_rects(120.0);
        assert!(narrow_button.top() > narrow_text.bottom());
    }
}

//! Small code-drawn vector icons used by editor controls.

/// Icons that remain sharp at every DPI without texture assets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EditorIcon {
    Move,
    Rotate,
    Scale,
    ViewOptions,
    SplitColumns,
    SplitRows,
    ChevronDown,
    ChevronUp,
    Close,
    AddObject,
}

pub(super) fn paint_editor_icon(
    painter: &egui::Painter,
    icon: EditorIcon,
    rect: egui::Rect,
    color: egui::Color32,
) {
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.31;
    let stroke = egui::Stroke::new(1.8_f32, color);
    match icon {
        EditorIcon::Move => {
            painter.line_segment(
                [
                    center - egui::vec2(radius, 0.0),
                    center + egui::vec2(radius, 0.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    center - egui::vec2(0.0, radius),
                    center + egui::vec2(0.0, radius),
                ],
                stroke,
            );
            for direction in [
                egui::vec2(1.0, 0.0),
                egui::vec2(-1.0, 0.0),
                egui::vec2(0.0, 1.0),
                egui::vec2(0.0, -1.0),
            ] {
                let tip = center + direction * radius;
                let side = egui::vec2(-direction.y, direction.x);
                painter.line_segment(
                    [tip, tip - direction * 4.0 + side * 3.0],
                    stroke,
                );
                painter.line_segment(
                    [tip, tip - direction * 4.0 - side * 3.0],
                    stroke,
                );
            }
        }
        EditorIcon::Rotate => {
            let start = -0.35_f32;
            let end = std::f32::consts::TAU - 1.15;
            let points = (0..=24).map(|step| {
                let angle = start + (end - start) * step as f32 / 24.0;
                center + egui::vec2(angle.cos(), angle.sin()) * radius
            });
            painter.add(egui::Shape::line(points.collect(), stroke));
            let tip = center + egui::vec2(end.cos(), end.sin()) * radius;
            let tangent = egui::vec2(-end.sin(), end.cos());
            let radial = egui::vec2(end.cos(), end.sin());
            painter.line_segment(
                [tip, tip - tangent * 5.0 + radial * 3.0],
                stroke,
            );
            painter.line_segment(
                [tip, tip - tangent * 5.0 - radial * 3.0],
                stroke,
            );
        }
        EditorIcon::Scale => {
            let start = center - egui::vec2(radius * 0.72, radius * 0.72);
            let end = center + egui::vec2(radius * 0.72, radius * 0.72);
            painter.line_segment([start, end], stroke);
            let box_size = egui::vec2(5.0, 5.0);
            painter.rect_filled(
                egui::Rect::from_center_size(start, box_size),
                1.0,
                color,
            );
            painter.rect_filled(
                egui::Rect::from_center_size(end, box_size),
                1.0,
                color,
            );
        }
        EditorIcon::ViewOptions => {
            for (offset, knob) in [(-0.58, -0.2), (0.0, 0.35), (0.58, -0.4)] {
                let y = center.y + radius * offset;
                painter.line_segment(
                    [
                        egui::pos2(center.x - radius, y),
                        egui::pos2(center.x + radius, y),
                    ],
                    stroke,
                );
                painter.circle_filled(
                    egui::pos2(center.x + radius * knob, y),
                    2.4,
                    color,
                );
            }
        }
        EditorIcon::SplitColumns => {
            let outline = egui::Rect::from_center_size(
                center,
                egui::vec2(radius * 1.75, radius * 1.45),
            );
            painter.rect_stroke(outline, 1.5, stroke, egui::StrokeKind::Inside);
            painter.line_segment(
                [
                    egui::pos2(center.x, outline.top()),
                    egui::pos2(center.x, outline.bottom()),
                ],
                stroke,
            );
        }
        EditorIcon::SplitRows => {
            let outline = egui::Rect::from_center_size(
                center,
                egui::vec2(radius * 1.75, radius * 1.45),
            );
            painter.rect_stroke(outline, 1.5, stroke, egui::StrokeKind::Inside);
            painter.line_segment(
                [
                    egui::pos2(outline.left(), center.y),
                    egui::pos2(outline.right(), center.y),
                ],
                stroke,
            );
        }
        EditorIcon::ChevronDown | EditorIcon::ChevronUp => {
            let direction = if icon == EditorIcon::ChevronDown {
                1.0
            } else {
                -1.0
            };
            painter.line_segment(
                [
                    center
                        + egui::vec2(-radius * 0.65, -direction * radius * 0.3),
                    center + egui::vec2(0.0, direction * radius * 0.35),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    center + egui::vec2(0.0, direction * radius * 0.35),
                    center
                        + egui::vec2(radius * 0.65, -direction * radius * 0.3),
                ],
                stroke,
            );
        }
        EditorIcon::Close => {
            let extent = egui::vec2(radius * 0.58, radius * 0.58);
            painter.line_segment([center - extent, center + extent], stroke);
            painter.line_segment(
                [
                    center + egui::vec2(-extent.x, extent.y),
                    center + egui::vec2(extent.x, -extent.y),
                ],
                stroke,
            );
        }
        EditorIcon::AddObject => {
            let cube_center = center + egui::vec2(2.0, 1.0);
            let cube_extent = radius * 0.58;
            painter.rect_stroke(
                egui::Rect::from_center_size(
                    cube_center,
                    egui::vec2(cube_extent * 1.35, cube_extent * 1.35),
                ),
                1.0,
                stroke,
                egui::StrokeKind::Inside,
            );
            let plus_center = center - egui::vec2(radius * 0.55, radius * 0.55);
            painter.line_segment(
                [
                    plus_center - egui::vec2(3.0, 0.0),
                    plus_center + egui::vec2(3.0, 0.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    plus_center - egui::vec2(0.0, 3.0),
                    plus_center + egui::vec2(0.0, 3.0),
                ],
                stroke,
            );
        }
    }
}

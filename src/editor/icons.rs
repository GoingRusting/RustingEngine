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
    ChevronRight,
    Close,
    AddObject,
    Camera,
    Light,
    Mesh,
    Empty,
    Eye,
    EyeClosed,
    Play,
    Code,
    Tree,
    Gear,
    Console,
    Folder,
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
        EditorIcon::ChevronRight => {
            let tip = center + egui::vec2(radius * 0.35, 0.0);
            for sign in [-1.0, 1.0] {
                painter.line_segment(
                    [
                        center
                            + egui::vec2(-radius * 0.3, sign * radius * 0.65),
                        tip,
                    ],
                    stroke,
                );
            }
        }
        EditorIcon::Play => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    center + egui::vec2(-radius * 0.55, -radius * 0.75),
                    center + egui::vec2(radius * 0.8, 0.0),
                    center + egui::vec2(-radius * 0.55, radius * 0.75),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        EditorIcon::Code => {
            let (w, h) = (radius * 0.45, radius * 0.7);
            for sign in [-1.0, 1.0] {
                let tip = center + egui::vec2(sign * radius * 1.0, 0.0);
                let back = center + egui::vec2(sign * (radius - w), 0.0);
                painter.line_segment([tip, back + egui::vec2(0.0, -h)], stroke);
                painter.line_segment([tip, back + egui::vec2(0.0, h)], stroke);
            }
            painter.line_segment(
                [
                    center + egui::vec2(radius * 0.2, -h),
                    center + egui::vec2(-radius * 0.2, h),
                ],
                stroke,
            );
        }
        EditorIcon::Tree => {
            let left = center.x - radius * 0.8;
            let top = center.y - radius * 0.75;
            for (row, indent) in [(0.0, 0.0), (1.0, 0.6), (2.0, 0.6)] {
                let y = top + row * radius * 0.75;
                let x = left + indent * radius;
                painter.circle_filled(egui::pos2(x, y), 1.8, color);
                painter.line_segment(
                    [egui::pos2(x + 4.0, y), egui::pos2(center.x + radius, y)],
                    stroke,
                );
            }
        }
        EditorIcon::Gear => {
            painter.circle_stroke(center, radius * 0.55, stroke);
            let teeth = egui::Stroke::new(2.6_f32, color);
            for step in 0..6 {
                let angle = step as f32 * std::f32::consts::TAU / 6.0;
                let direction = egui::vec2(angle.cos(), angle.sin());
                painter.line_segment(
                    [
                        center + direction * radius * 0.6,
                        center + direction * radius * 1.0,
                    ],
                    teeth,
                );
            }
        }
        EditorIcon::Console => {
            let outline = egui::Rect::from_center_size(
                center,
                egui::vec2(radius * 2.1, radius * 1.7),
            );
            painter.rect_stroke(outline, 1.5, stroke, egui::StrokeKind::Inside);
            let start = outline.left_center() + egui::vec2(radius * 0.4, 0.0);
            painter.line_segment(
                [
                    start + egui::vec2(0.0, -radius * 0.35),
                    start + egui::vec2(radius * 0.35, 0.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    start + egui::vec2(radius * 0.35, 0.0),
                    start + egui::vec2(0.0, radius * 0.35),
                ],
                stroke,
            );
        }
        EditorIcon::Folder => {
            let body = egui::Rect::from_center_size(
                center + egui::vec2(0.0, radius * 0.1),
                egui::vec2(radius * 2.1, radius * 1.5),
            );
            painter.rect_stroke(body, 1.5, stroke, egui::StrokeKind::Inside);
            painter.line_segment(
                [
                    body.left_top() + egui::vec2(1.0, 0.0),
                    body.left_top() + egui::vec2(radius * 0.8, 0.0),
                ],
                egui::Stroke::new(3.0_f32, color),
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
        EditorIcon::Camera => {
            let body = egui::Rect::from_center_size(
                center - egui::vec2(radius * 0.3, 0.0),
                egui::vec2(radius * 1.2, radius * 1.1),
            );
            painter.rect_stroke(body, 1.5, stroke, egui::StrokeKind::Inside);
            let lens_x = body.right() + radius * 0.75;
            painter.add(egui::Shape::closed_line(
                vec![
                    egui::pos2(body.right(), center.y),
                    egui::pos2(lens_x, center.y - radius * 0.55),
                    egui::pos2(lens_x, center.y + radius * 0.55),
                ],
                stroke,
            ));
        }
        EditorIcon::Light => {
            painter.circle_stroke(center, radius * 0.5, stroke);
            for step in 0..8 {
                let angle = step as f32 * std::f32::consts::TAU / 8.0;
                let direction = egui::vec2(angle.cos(), angle.sin());
                painter.line_segment(
                    [
                        center + direction * radius * 0.8,
                        center + direction * radius * 1.15,
                    ],
                    stroke,
                );
            }
        }
        EditorIcon::Mesh => {
            let size = radius * 1.2;
            let offset = egui::vec2(radius * 0.45, -radius * 0.45);
            let front = egui::Rect::from_center_size(
                center - offset * 0.5,
                egui::vec2(size, size),
            );
            let back = front.translate(offset);
            painter.rect_stroke(front, 0.0, stroke, egui::StrokeKind::Middle);
            painter.rect_stroke(back, 0.0, stroke, egui::StrokeKind::Middle);
            for (a, b) in [
                (front.left_top(), back.left_top()),
                (front.right_top(), back.right_top()),
                (front.right_bottom(), back.right_bottom()),
            ] {
                painter.line_segment([a, b], stroke);
            }
        }
        EditorIcon::Empty => {
            // Blender's "plain axes" empty.
            for direction in [egui::vec2(1.0, 0.0), egui::vec2(0.0, 1.0)] {
                painter.line_segment(
                    [center - direction * radius, center + direction * radius],
                    stroke,
                );
            }
        }
        EditorIcon::Eye | EditorIcon::EyeClosed => {
            let lid = |sign: f32| {
                (0..=12)
                    .map(|step| {
                        let t = step as f32 / 12.0 * 2.0 - 1.0;
                        center
                            + egui::vec2(
                                t * radius * 1.1,
                                sign * (1.0 - t * t) * radius * 0.6,
                            )
                    })
                    .collect::<Vec<_>>()
            };
            painter.add(egui::Shape::line(lid(1.0), stroke));
            if icon == EditorIcon::Eye {
                painter.add(egui::Shape::line(lid(-1.0), stroke));
                painter.circle_filled(center, radius * 0.3, color);
            }
        }
    }
}

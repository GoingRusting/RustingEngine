//! Project Settings page: a header with the project and its export actions,
//! a category rail, and cards of settings and live metrics.
//!
//! Scene-saved renderer options are marked as such; the rest are session
//! options, as in the Render Settings panel.

use super::gui_elements::{kit, EditorTheme};
use super::EditorIcon;
use crate::rendering::scene_renderer::{
    resolve_quality, CullingStats, RenderCounters, RendererCapabilities,
};
use crate::runtime::ExtractionReport;
use crate::runtime::{
    Antialiasing, CpuFrameTimings, CullingMode, QualityProfile, RenderSettings,
    ShadowQuality,
};

/// Export the user asked for this frame.
pub(super) enum ExportRequest {
    Native,
    Windows,
}

/// Everything the page reads; `settings` is the only mutable part.
pub(super) struct ProjectSettingsView<'a> {
    pub project_root: &'a str,
    pub scene_path: &'a mut String,
    pub scene_message: Option<&'a str>,
    pub settings: &'a mut RenderSettings,
    pub build_running: bool,
    /// Meshes, materials, textures, scenes, LOD groups.
    pub asset_counts: Option<(usize, usize, usize, usize, usize)>,
    pub capabilities: Option<&'a RendererCapabilities>,
    pub extraction: Option<ExtractionReport>,
    pub culling: Option<CullingStats>,
    pub counters: Option<RenderCounters>,
    pub cpu: Option<CpuFrameTimings>,
    pub gpu_passes: Option<&'a str>,
    pub physics: Option<&'a str>,
    pub windows_target: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Category {
    Overview,
    Rendering,
    Display,
    Export,
    Diagnostics,
}

const CATEGORIES: [(Category, EditorIcon, &str); 5] = [
    (Category::Overview, EditorIcon::Tree, "Overview"),
    (Category::Rendering, EditorIcon::Camera, "Rendering"),
    (Category::Display, EditorIcon::Play, "Display"),
    (Category::Export, EditorIcon::Folder, "Export"),
    (Category::Diagnostics, EditorIcon::Console, "Diagnostics"),
];

pub(super) fn draw(
    ui: &mut egui::Ui,
    view: ProjectSettingsView<'_>,
) -> Option<ExportRequest> {
    let mut export = None;
    let id = ui.make_persistent_id("project_settings_category");
    let mut category = ui
        .data(|data| data.get_temp::<u8>(id))
        .and_then(|index| CATEGORIES.get(usize::from(index)))
        .map_or(Category::Overview, |(category, ..)| *category);
    let ProjectSettingsView {
        project_root,
        scene_path,
        scene_message,
        settings,
        build_running,
        asset_counts,
        capabilities,
        extraction,
        culling,
        counters,
        cpu,
        gpu_passes,
        physics,
        windows_target,
    } = view;
    let wide = ui.available_width() > 560.0;
    let mut body = |ui: &mut egui::Ui| {
        egui::ScrollArea::vertical()
            .id_salt(("project_settings_scroll", category as u8))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(2.0);
                match category {
                    Category::Overview => overview(
                        ui,
                        project_root,
                        scene_path,
                        asset_counts,
                        capabilities,
                        settings,
                    ),
                    Category::Rendering => {
                        rendering(ui, settings, capabilities)
                    }
                    Category::Display => display(ui, settings),
                    Category::Export => {
                        export = exports(
                            ui,
                            build_running,
                            scene_message,
                            windows_target,
                        );
                    }
                    Category::Diagnostics => diagnostics(
                        ui, extraction, culling, counters, cpu, gpu_passes,
                        physics,
                    ),
                }
                ui.add_space(12.0);
            });
    };
    let mut picked = category;
    if wide {
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(150.0, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (item, icon, label) in CATEGORIES {
                        if kit::nav_item(
                            ui,
                            icon,
                            label,
                            item == category,
                            false,
                        )
                        .clicked()
                        {
                            picked = item;
                        }
                    }
                },
            );
            ui.add_space(4.0);
            ui.vertical(body);
        });
    } else {
        egui::ScrollArea::horizontal()
            .id_salt("project_settings_tabs")
            .scroll_bar_visibility(
                egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (item, icon, label) in CATEGORIES {
                        if kit::nav_item(
                            ui,
                            icon,
                            label,
                            item == category,
                            true,
                        )
                        .clicked()
                        {
                            picked = item;
                        }
                    }
                });
            });
        ui.add_space(6.0);
        body(ui);
    }
    if picked != category {
        category = picked;
        let index = CATEGORIES
            .iter()
            .position(|(item, ..)| *item == category)
            .unwrap_or(0) as u8;
        ui.data_mut(|data| data.insert_temp(id, index));
    }
    export
}

fn overview(
    ui: &mut egui::Ui,
    project_root: &str,
    scene_path: &mut String,
    asset_counts: Option<(usize, usize, usize, usize, usize)>,
    capabilities: Option<&RendererCapabilities>,
    settings: &RenderSettings,
) {
    if let Some((meshes, materials, textures, scenes, lods)) = asset_counts {
        let tiles = [
            ("Meshes", meshes, EditorTheme::ACCENT_HOVER),
            ("Materials", materials, EditorTheme::ACTIVE_OBJECT),
            ("Textures", textures, EditorTheme::AXIS[1]),
            ("Scenes", scenes, EditorTheme::AXIS[0]),
            ("LOD groups", lods, EditorTheme::LINK),
        ];
        kit::tile_grid(ui, tiles.len(), 120.0, |ui, index, width| {
            let (caption, count, color) = tiles[index];
            kit::stat_tile(
                ui,
                width,
                caption,
                &count.to_string(),
                "",
                None,
                color,
            );
        });
    }
    ui.add_space(4.0);
    kit::card(
        ui,
        EditorIcon::Folder,
        "Project",
        "Where the game lives",
        |ui, rows| {
            rows.row(ui, "Folder", "Project root on disk", |ui| {
                if project_root.is_empty() {
                    kit::pill(ui, "No project open", EditorTheme::WARNING);
                } else {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(project_root).monospace(),
                        )
                        .truncate(),
                    )
                    .on_hover_text(project_root);
                }
            });
            rows.row(
                ui,
                "Startup scene",
                "Path relative to the project folder",
                |ui| {
                    ui.add(
                        egui::TextEdit::singleline(scene_path)
                            .desired_width(f32::INFINITY),
                    );
                },
            );
        },
    );
    ui.add_space(8.0);
    kit::card(
        ui,
        EditorIcon::Camera,
        "Renderer",
        "What this machine will run",
        |ui, rows| {
            rows.row(ui, "Requested quality", "From Rendering", |ui| {
                kit::pill(
                    ui,
                    &format!("{:?}", settings.quality),
                    EditorTheme::ACCENT_HOVER,
                );
            });
            if let Some(caps) = capabilities {
                rows.row(
                    ui,
                    "Resolved quality",
                    "Auto picks by GPU capabilities",
                    |ui| {
                        kit::pill(
                            ui,
                            &format!(
                                "{:?}",
                                resolve_quality(settings.quality, caps)
                            ),
                            EditorTheme::AXIS[1],
                        );
                    },
                );
            }
        },
    );
}

fn rendering(
    ui: &mut egui::Ui,
    settings: &mut RenderSettings,
    capabilities: Option<&RendererCapabilities>,
) {
    kit::card(
        ui,
        EditorIcon::Camera,
        "Scene rendering",
        "Saved with the scene and used by the exported game",
        |ui, rows| {
            rows.row(
                ui,
                "Quality",
                "Auto chooses a profile from the GPU",
                |ui| {
                    kit::segmented(
                        ui,
                        "quality",
                        &mut settings.quality,
                        &[
                            (QualityProfile::Auto, "Auto"),
                            (QualityProfile::Eco, "Eco"),
                            (QualityProfile::Balanced, "Balanced"),
                            (QualityProfile::High, "High"),
                        ],
                    );
                },
            );
            rows.row(ui, "Culling", "Skip objects that cannot be seen", |ui| {
                kit::segmented(
                    ui,
                    "culling",
                    &mut settings.culling,
                    &[
                        (CullingMode::Auto, "Auto"),
                        (CullingMode::Disabled, "Off"),
                        (CullingMode::Frustum, "Frustum"),
                        (
                            CullingMode::FrustumAndOcclusion,
                            "Frustum + Occlusion",
                        ),
                    ],
                );
            });
        },
    );
    ui.add_space(8.0);
    kit::card(
        ui,
        EditorIcon::Light,
        "Image quality",
        "Session options; the scene file does not keep them",
        |ui, rows| {
            rows.row(ui, "Anti-aliasing", "Smooths jagged edges", |ui| {
                kit::segmented(
                    ui,
                    "antialiasing",
                    &mut settings.antialiasing,
                    &[
                        (Antialiasing::Auto, "Auto"),
                        (Antialiasing::Off, "Off"),
                        (Antialiasing::Msaa2, "MSAA 2x"),
                        (Antialiasing::Msaa4, "MSAA 4x"),
                        (Antialiasing::Fxaa, "FXAA"),
                    ],
                );
            });
            if let Some(caps) = capabilities {
                rows.row(ui, "MSAA in use", "After GPU limits", |ui| {
                    kit::pill(
                        ui,
                        &format!(
                            "{}x",
                            crate::rendering::scene_renderer::scene_sample_count(
                                settings.antialiasing,
                                resolve_quality(settings.quality, caps),
                                caps,
                            )
                        ),
                        EditorTheme::AXIS[1],
                    );
                });
            }
            rows.row(ui, "Shadow map", "Resolution of shadow maps", |ui| {
                kit::segmented(
                    ui,
                    "shadows",
                    &mut settings.shadows,
                    &[
                        (ShadowQuality::Auto, "Auto"),
                        (ShadowQuality::Low, "1024"),
                        (ShadowQuality::Medium, "2048"),
                        (ShadowQuality::High, "4096"),
                    ],
                );
            });
            rows.row(
                ui,
                "Hard shadows",
                "One shadow-map tap: crisp edges for a low-poly look",
                |ui| {
                    kit::toggle(ui, &mut settings.hard_shadows);
                },
            );
            rows.row(
                ui,
                "Reflections",
                "Screen-space reflections; turn off to save GPU time",
                |ui| {
                    kit::toggle(ui, &mut settings.reflections);
                },
            );
        },
    );
}

fn display(ui: &mut egui::Ui, settings: &mut RenderSettings) {
    kit::card(
        ui,
        EditorIcon::Play,
        "Frame pacing",
        "How fast the game presents frames",
        |ui, rows| {
            rows.row(
                ui,
                "VSync",
                "Wait for the display; removes tearing",
                |ui| {
                    kit::toggle(ui, &mut settings.vsync);
                },
            );
            rows.row(
                ui,
                "Limit FPS",
                "Cap the frame rate to save power",
                |ui| {
                    kit::toggle(ui, &mut settings.limit_fps);
                },
            );
            rows.row(ui, "Maximum FPS", "Used when the limit is on", |ui| {
                ui.add_enabled(
                    settings.limit_fps,
                    egui::DragValue::new(&mut settings.max_fps)
                        .range(1..=1_000)
                        .suffix(" fps"),
                );
            });
        },
    );
}

fn exports(
    ui: &mut egui::Ui,
    build_running: bool,
    message: Option<&str>,
    windows_target: &str,
) -> Option<ExportRequest> {
    let mut request = None;
    kit::card(
        ui,
        EditorIcon::Folder,
        "Export game",
        "Build a release and copy the runtime files next to it",
        |ui, rows| {
            rows.row(ui, "This platform", "Native release build", |ui| {
                if kit::action_button(
                    ui,
                    EditorIcon::Play,
                    "Export Game...",
                    true,
                    !build_running,
                )
                .clicked()
                {
                    request = Some(ExportRequest::Native);
                }
            });
            if !cfg!(target_os = "windows") {
                rows.row(
                    ui,
                    "Windows",
                    &format!(
                        "Needs `rustup target add {windows_target}` and mingw-w64"
                    ),
                    |ui| {
                        if kit::action_button(
                            ui,
                            EditorIcon::Folder,
                            "Export for Windows...",
                            false,
                            !build_running,
                        )
                        .clicked()
                        {
                            request = Some(ExportRequest::Windows);
                        }
                    },
                );
            }
        },
    );
    if build_running {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(
                egui::RichText::new(
                    "Build running. Output is in the Code Editor.",
                )
                .color(EditorTheme::TEXT_MUTED),
            );
        });
    }
    if let Some(message) = message {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(message).color(EditorTheme::TEXT_MUTED));
    }
    request
}

fn diagnostics(
    ui: &mut egui::Ui,
    extraction: Option<ExtractionReport>,
    culling: Option<CullingStats>,
    counters: Option<RenderCounters>,
    cpu: Option<CpuFrameTimings>,
    gpu_passes: Option<&str>,
    physics: Option<&str>,
) {
    let ms = |time: std::time::Duration| time.as_secs_f64() * 1000.0;
    if extraction.is_none()
        && culling.is_none()
        && counters.is_none()
        && cpu.is_none()
    {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("No frame data yet")
                    .size(15.0)
                    .color(EditorTheme::TEXT),
            );
            ui.label(
                egui::RichText::new(
                    "Metrics appear once the scene renders a frame.",
                )
                .color(EditorTheme::TEXT_MUTED),
            );
        });
        return;
    }
    if let Some(report) = extraction {
        kit::card(
            ui,
            EditorIcon::Mesh,
            "Scene extraction",
            "Renderables handed to the renderer",
            |ui, _| {
                ui.add_space(8.0);
                let tiles = [
                    ("Renderables", report.total, EditorTheme::ACCENT_HOVER),
                    ("Added", report.added, EditorTheme::AXIS[1]),
                    ("Changed", report.changed, EditorTheme::WARNING),
                    ("Removed", report.removed, EditorTheme::AXIS[0]),
                ];
                kit::tile_grid(ui, tiles.len(), 100.0, |ui, index, width| {
                    let (caption, value, color) = tiles[index];
                    kit::stat_tile(
                        ui,
                        width,
                        caption,
                        &value.to_string(),
                        "",
                        None,
                        color,
                    );
                });
            },
        );
        ui.add_space(8.0);
    }
    if let Some(stats) = culling {
        kit::card(
            ui,
            EditorIcon::Eye,
            "Culling",
            &format!("{:?} path", stats.path),
            |ui, _| {
                ui.add_space(8.0);
                let submitted = stats.submitted.max(1) as f32;
                let time = stats.time.map_or("n/a".to_owned(), |time| {
                    format!("{:.3} ms", ms(time))
                });
                let tiles = [
                    (
                        "Submitted",
                        stats.submitted.to_string(),
                        None,
                        EditorTheme::ACCENT_HOVER,
                    ),
                    (
                        "Visible",
                        stats.visible.to_string(),
                        Some(stats.visible as f32 / submitted),
                        EditorTheme::AXIS[1],
                    ),
                    (
                        "Culled",
                        stats.culled.to_string(),
                        Some(stats.culled as f32 / submitted),
                        EditorTheme::AXIS[0],
                    ),
                    ("Cost", time, None, EditorTheme::WARNING),
                ];
                kit::tile_grid(ui, tiles.len(), 100.0, |ui, index, width| {
                    let (caption, value, meter, color) = &tiles[index];
                    kit::stat_tile(
                        ui, width, caption, value, "", *meter, *color,
                    );
                });
            },
        );
        ui.add_space(8.0);
    }
    if let Some(timings) = cpu {
        kit::card(
            ui,
            EditorIcon::Gear,
            "CPU frame",
            "Milliseconds per frame part",
            |ui, _| {
                ui.add_space(8.0);
                let parts = [
                    ("Physics", timings.physics, EditorTheme::AXIS[0]),
                    ("Extract", timings.extraction, EditorTheme::WARNING),
                    ("Prepare", timings.preparation, EditorTheme::AXIS[1]),
                    ("Record", timings.recording, EditorTheme::ACCENT_HOVER),
                    ("Editor", timings.editor, EditorTheme::LINK),
                ];
                let total: f64 =
                    parts.iter().map(|(_, time, _)| ms(*time)).sum();
                let (bar, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 14.0),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(
                    bar,
                    egui::CornerRadius::same(4),
                    EditorTheme::INPUT,
                );
                let mut x = bar.left();
                for (_, time, color) in parts {
                    let width = if total > 0.0 {
                        bar.width() * (ms(time) / total) as f32
                    } else {
                        0.0
                    };
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            egui::pos2(x, bar.top()),
                            egui::vec2(width, bar.height()),
                        ),
                        egui::CornerRadius::ZERO,
                        color,
                    );
                    x += width;
                }
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    for (label, time, color) in parts {
                        let (dot, _) = ui.allocate_exact_size(
                            egui::vec2(8.0, 8.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().circle_filled(dot.center(), 4.0, color);
                        ui.label(
                            egui::RichText::new(format!(
                                "{label} {:.3}",
                                ms(time)
                            ))
                            .size(12.0)
                            .color(EditorTheme::TEXT_MUTED),
                        );
                        ui.add_space(6.0);
                    }
                });
                ui.add_space(8.0);
            },
        );
        ui.add_space(8.0);
    }
    if let Some(counters) = counters {
        kit::card(
            ui,
            EditorIcon::Image,
            "Renderer work",
            "Counters for the last frame",
            |ui, _| {
                ui.add_space(8.0);
                let mib = |bytes: u64| bytes as f64 / (1024.0 * 1024.0);
                let tiles = [
                    ("Draws", counters.draws.to_string()),
                    ("Dispatches", counters.dispatches.to_string()),
                    ("Triangles", counters.triangles.to_string()),
                    ("Visible", counters.visible_instances.to_string()),
                    (
                        "Upload",
                        format!("{:.2} MiB", mib(counters.upload_bytes)),
                    ),
                    (
                        "GPU memory",
                        format!("{:.1} MiB", mib(counters.gpu_memory_bytes)),
                    ),
                ];
                kit::tile_grid(ui, tiles.len(), 110.0, |ui, index, width| {
                    let (caption, value) = &tiles[index];
                    kit::stat_tile(
                        ui,
                        width,
                        caption,
                        value,
                        "",
                        None,
                        EditorTheme::ACCENT_HOVER,
                    );
                });
            },
        );
        ui.add_space(8.0);
    }
    for (title, text) in [("GPU passes", gpu_passes), ("Physics", physics)] {
        if let Some(text) = text {
            kit::card(ui, EditorIcon::Console, title, "", |ui, _| {
                ui.add_space(6.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(text)
                            .monospace()
                            .size(11.0)
                            .color(EditorTheme::TEXT_MUTED),
                    )
                    .wrap(),
                );
                ui.add_space(8.0);
            });
            ui.add_space(8.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page_text(index: u8) -> String {
        let context = egui::Context::default();
        let mut settings = RenderSettings::default();
        let mut scene = "scenes/main.rscene".to_owned();
        let output = context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                let id = ui.make_persistent_id("project_settings_category");
                ui.data_mut(|data| data.insert_temp(id, index));
                draw(
                    ui,
                    ProjectSettingsView {
                        project_root: "/games/demo",
                        scene_path: &mut scene,
                        scene_message: None,
                        settings: &mut settings,
                        build_running: false,
                        asset_counts: Some((1, 2, 3, 4, 5)),
                        capabilities: None,
                        extraction: Some(ExtractionReport::default()),
                        culling: Some(CullingStats::default()),
                        counters: Some(RenderCounters::default()),
                        cpu: Some(CpuFrameTimings::default()),
                        gpu_passes: None,
                        physics: None,
                        windows_target: "x86_64-pc-windows-gnu",
                    },
                );
            });
        });
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn every_category_draws_its_cards() {
        for (index, needle) in [
            (0, "Startup scene"),
            (1, "Scene rendering"),
            (2, "Frame pacing"),
            (3, "Export game"),
            (4, "Renderer work"),
        ] {
            let text = page_text(index);
            assert!(text.contains(needle), "{needle} missing: {text}");
        }
    }
}

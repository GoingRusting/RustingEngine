//! Assets area: the project's `assets` folder as a Godot-style FileSystem
//! tree. Folders come first and fold; each file shows a type icon and a type
//! badge. Double-click does the obvious thing for the type (add a model to
//! the scene, put an image on the selected object), and right-click lists
//! every action.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::file_dialogs::{DialogPurpose, FileDialogs};
use super::gui_elements::EditorTheme;
use super::icons::paint_editor_icon;
use super::{AssetRequest, EditorAssetState, EditorIcon};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AssetKind {
    Model,
    Image,
    Scene,
    Other,
}

pub(super) fn asset_kind(path: &Path) -> AssetKind {
    match extension(path).as_str() {
        "gltf" | "glb" => AssetKind::Model,
        "png" | "jpg" | "jpeg" | "bmp" | "tga" => AssetKind::Image,
        "rscene" => AssetKind::Scene,
        _ => AssetKind::Other,
    }
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Cooked caches the importers write next to their sources. They are not
/// something to pick, so the browser hides them.
fn is_engine_cache(path: &Path) -> bool {
    matches!(extension(path).as_str(), "rmesh" | "rtexture")
}

/// Drag payload of a model row; the Scene View adds it on drop.
pub(super) struct ModelDrag(pub PathBuf);

/// Drag payload of an image row; Hierarchy rows and Inspector texture slots
/// assign it on drop.
pub(super) struct ImageDrag(pub PathBuf);

/// Side of a decoded preview; rows draw it at icon size, tooltips larger.
const THUMBNAIL_SIDE: u32 = 96;
/// New images decoded per frame, so opening a big folder does not stall.
const THUMBNAILS_PER_FRAME: usize = 2;

/// Decodes `path` into a small egui texture.
fn load_thumbnail(
    context: &egui::Context,
    path: &Path,
) -> Option<egui::TextureHandle> {
    let image = image::open(path)
        .ok()?
        .thumbnail(THUMBNAIL_SIDE, THUMBNAIL_SIDE)
        .into_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Some(context.load_texture(
        path.to_string_lossy(),
        egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
        egui::TextureOptions::LINEAR,
    ))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AssetRow {
    pub depth: usize,
    pub name: String,
    pub path: PathBuf,
    pub folder: bool,
}

/// Rows of the tree under `root`: folders before files at every level, both
/// by name. Rows inside a `collapsed` folder are left out. A non-empty
/// `filter` keeps files whose relative path contains it (any case) and
/// opens every folder on the way to them.
pub(super) fn asset_rows(
    root: &Path,
    files: &[PathBuf],
    filter: &str,
    collapsed: &HashSet<PathBuf>,
) -> Vec<AssetRow> {
    let filter = filter.to_lowercase();
    let mut shown = files
        .iter()
        .filter(|path| !is_engine_cache(path))
        .filter_map(|path| Some((path, path.strip_prefix(root).ok()?)))
        .filter(|(_, relative)| {
            filter.is_empty()
                || relative.to_string_lossy().to_lowercase().contains(&filter)
        })
        .collect::<Vec<_>>();
    // Sort key per component: folders (0) before files (1), then by name.
    let key = |relative: &Path| {
        let parts = relative.components().collect::<Vec<_>>();
        parts
            .iter()
            .enumerate()
            .map(|(index, part)| {
                let name = part.as_os_str().to_string_lossy().to_lowercase();
                (index + 1 == parts.len(), name)
            })
            .collect::<Vec<_>>()
    };
    shown.sort_by_cached_key(|(_, relative)| key(relative));

    let mut rows = Vec::new();
    let mut emitted = HashSet::new();
    for (path, relative) in shown {
        let mut folder = root.to_path_buf();
        let mut hidden = false;
        let parts = relative.components().collect::<Vec<_>>();
        for (depth, part) in parts[..parts.len() - 1].iter().enumerate() {
            folder.push(part);
            if !hidden && emitted.insert(folder.clone()) {
                rows.push(AssetRow {
                    depth,
                    name: part.as_os_str().to_string_lossy().into_owned(),
                    path: folder.clone(),
                    folder: true,
                });
            }
            hidden |= filter.is_empty() && collapsed.contains(&folder);
        }
        if !hidden {
            rows.push(AssetRow {
                depth: parts.len() - 1,
                name: path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                path: path.clone(),
                folder: false,
            });
        }
    }
    rows
}

/// Draws the Assets area and records the chosen action in `request`.
pub(super) fn draw_assets_area(
    ui: &mut egui::Ui,
    assets: &mut EditorAssetState,
    project_root: &str,
    files: &[PathBuf],
    has_selection: bool,
    dialogs: &mut FileDialogs,
    request: &mut Option<AssetRequest>,
) {
    let mut import = false;
    ui.horizontal(|ui| {
        import = EditorTheme::toolbar_icon_button(
            ui,
            "Import",
            EditorIcon::AddObject,
            78.0,
            true,
        )
        .on_hover_text("Copy models and images into the project")
        .clicked();
        ui.add(
            egui::TextEdit::singleline(&mut assets.filter)
                .hint_text("Filter files")
                .desired_width(f32::INFINITY),
        );
    });
    ui.add_space(2.0);

    let root = PathBuf::from(project_root).join("assets");
    let rows = asset_rows(&root, files, &assets.filter, &assets.collapsed);
    let footer = EditorTheme::ROW_HEIGHT;
    egui::ScrollArea::vertical()
        .id_salt("assets_panel_scroll")
        .auto_shrink([false, false])
        .max_height((ui.available_height() - footer).max(0.0))
        .show_rows(ui, EditorTheme::ROW_HEIGHT, rows.len(), |ui, range| {
            let mut decode_budget = THUMBNAILS_PER_FRAME;
            for row in &rows[range] {
                draw_row(
                    ui,
                    row,
                    assets,
                    has_selection,
                    request,
                    &mut decode_budget,
                );
            }
        });
    if rows.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(if assets.filter.is_empty() {
                    "No assets yet.\nImport glTF models and images, or copy \
                     them into the project's assets folder."
                } else {
                    "No files match the filter."
                })
                .color(EditorTheme::TEXT_MUTED),
            );
        });
    }
    if let Some(message) = &assets.message {
        ui.label(
            egui::RichText::new(message)
                .small()
                .color(EditorTheme::TEXT_MUTED),
        );
    }
    if import {
        dialogs.pick_files(
            DialogPurpose::ImportFiles,
            rfd::AsyncFileDialog::new()
                .set_title("Import Project Assets")
                .add_filter(
                    "Models and images",
                    &["gltf", "glb", "png", "jpg", "jpeg", "bmp", "tga"],
                )
                .add_filter("All files", &["*"]),
        );
    }
}

fn draw_row(
    ui: &mut egui::Ui,
    row: &AssetRow,
    assets: &mut EditorAssetState,
    has_selection: bool,
    request: &mut Option<AssetRequest>,
    decode_budget: &mut usize,
) {
    let kind = asset_kind(&row.path);
    let icon = match (row.folder, kind) {
        (true, _) => EditorIcon::Folder,
        (_, AssetKind::Model) => EditorIcon::Mesh,
        (_, AssetKind::Image) => EditorIcon::Image,
        (_, AssetKind::Scene) => EditorIcon::Tree,
        (_, AssetKind::Other) => EditorIcon::File,
    };
    let selected = assets.selected.as_ref() == Some(&row.path);
    let response = EditorTheme::tree_row(
        ui,
        &row.name,
        row.depth,
        selected,
        false,
        Some(icon),
    );

    // Right side: fold chevron for folders, type badge for files.
    let right =
        egui::pos2(response.rect.right() - 6.0, response.rect.center().y);
    if row.folder {
        let open = !assets.collapsed.contains(&row.path);
        paint_editor_icon(
            ui.painter(),
            if open {
                EditorIcon::ChevronDown
            } else {
                EditorIcon::ChevronRight
            },
            egui::Rect::from_center_size(
                right - egui::vec2(6.0, 0.0),
                egui::vec2(14.0, 14.0),
            ),
            EditorTheme::TEXT_MUTED,
        );
    } else {
        let badge = extension(&row.path).to_ascii_uppercase();
        // Files with actions stand out from support files like `.bin`.
        let badge_color = match kind {
            AssetKind::Model => EditorTheme::ACTIVE_OBJECT,
            AssetKind::Image => EditorTheme::LINK,
            AssetKind::Scene | AssetKind::Other => EditorTheme::TEXT_MUTED,
        };
        let galley = ui.painter().layout_no_wrap(
            badge,
            egui::FontId::proportional(10.0),
            badge_color,
        );
        let rect = egui::Rect::from_min_size(
            right
                - egui::vec2(
                    galley.size().x + 8.0,
                    galley.size().y * 0.5 + 2.0,
                ),
            galley.size() + egui::vec2(8.0, 4.0),
        );
        ui.painter().rect_filled(
            rect,
            f32::from(EditorTheme::RADIUS),
            EditorTheme::INPUT,
        );
        ui.painter().galley(
            rect.min + egui::vec2(4.0, 2.0),
            galley,
            badge_color,
        );
    }

    // ponytail: previews are decoded once per session; an image edited on
    // disk keeps its old preview until the editor restarts.
    if kind == AssetKind::Image && !row.folder {
        if !assets.thumbnails.contains_key(&row.path) && *decode_budget > 0 {
            *decode_budget -= 1;
            let thumbnail = load_thumbnail(ui.ctx(), &row.path);
            assets.thumbnails.insert(row.path.clone(), thumbnail);
        }
        if let Some(Some(thumbnail)) = assets.thumbnails.get(&row.path) {
            let rect = egui::Rect::from_center_size(
                egui::pos2(
                    response.rect.left() + 16.0,
                    response.rect.center().y,
                ),
                egui::vec2(16.0, 16.0),
            );
            ui.painter().rect_filled(rect, 0.0, EditorTheme::INPUT);
            ui.painter().image(
                thumbnail.id(),
                rect,
                egui::Rect::from_min_max(
                    egui::pos2(0.0, 0.0),
                    egui::pos2(1.0, 1.0),
                ),
                egui::Color32::WHITE,
            );
        }
    }

    if response.clicked() {
        if row.folder && !assets.collapsed.remove(&row.path) {
            assets.collapsed.insert(row.path.clone());
        }
        assets.selected = Some(row.path.clone());
    }
    if row.folder {
        return;
    }
    let add_model = || AssetRequest::AddModel(row.path.clone());
    let use_image = || AssetRequest::AssignTexture(row.path.clone());
    match kind {
        AssetKind::Model => {
            response.dnd_set_drag_payload(ModelDrag(row.path.clone()));
        }
        AssetKind::Image => {
            response.dnd_set_drag_payload(ImageDrag(row.path.clone()));
        }
        AssetKind::Scene | AssetKind::Other => {}
    }
    let thumbnail = assets.thumbnails.get(&row.path).cloned().flatten();
    let response = match kind {
        AssetKind::Model => response.on_hover_text(
            "Double-click or drag into the Scene View to add the model",
        ),
        AssetKind::Image => response.on_hover_ui(|ui| {
            if let Some(thumbnail) = &thumbnail {
                ui.image((thumbnail.id(), thumbnail.size_vec2()));
            }
            ui.label(if has_selection {
                "Double-click to use as the selected object's base color. \
                 Drag onto a Hierarchy object or an Inspector texture slot."
            } else {
                "Drag onto a Hierarchy object or an Inspector texture slot."
            });
        }),
        _ => response,
    };
    if response.double_clicked() {
        match kind {
            AssetKind::Model => *request = Some(add_model()),
            AssetKind::Image if has_selection => *request = Some(use_image()),
            AssetKind::Image => {
                *request = Some(AssetRequest::LoadTexture(row.path.clone()));
            }
            _ => {}
        }
    }
    response.context_menu(|ui| {
        assets.selected = Some(row.path.clone());
        match kind {
            AssetKind::Model => {
                EditorTheme::menu_section(ui, "MODEL");
                if EditorTheme::menu_action(ui, "Add to Scene", true).clicked()
                {
                    *request = Some(add_model());
                    ui.close_menu();
                }
            }
            AssetKind::Image => {
                EditorTheme::menu_section(ui, "IMAGE");
                if EditorTheme::menu_action(
                    ui,
                    "Use as Base Color on Selected",
                    has_selection,
                )
                .clicked()
                {
                    *request = Some(use_image());
                    ui.close_menu();
                }
                if EditorTheme::menu_action(ui, "Load Texture", true).clicked()
                {
                    *request =
                        Some(AssetRequest::LoadTexture(row.path.clone()));
                    ui.close_menu();
                }
            }
            AssetKind::Scene | AssetKind::Other => {
                ui.label(
                    egui::RichText::new("No actions for this file type")
                        .color(EditorTheme::TEXT_MUTED),
                );
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_put_folders_first_hide_caches_and_fold() {
        let root = PathBuf::from("/p/assets");
        let files = [
            "z.png",
            "models/crate.gltf",
            "models/crate.mesh-0-0.rmesh",
            "models/crate.bin",
            "a.glb",
        ]
        .map(|file| root.join(file));
        let names = |rows: Vec<AssetRow>| {
            rows.into_iter()
                .map(|row| format!("{}{}", "  ".repeat(row.depth), row.name))
                .collect::<Vec<_>>()
        };
        let mut collapsed = HashSet::new();
        assert_eq!(
            names(asset_rows(&root, &files, "", &collapsed)),
            ["models", "  crate.bin", "  crate.gltf", "a.glb", "z.png"]
        );
        collapsed.insert(root.join("models"));
        assert_eq!(
            names(asset_rows(&root, &files, "", &collapsed)),
            ["models", "a.glb", "z.png"]
        );
        // Filtering opens folders on the way to a match.
        assert_eq!(
            names(asset_rows(&root, &files, "CRATE.G", &collapsed)),
            ["models", "  crate.gltf"]
        );
    }

    #[test]
    fn image_rows_decode_a_few_thumbnails_per_frame() {
        let root = std::env::temp_dir()
            .join(format!("rusting-thumbnails-{}", uuid::Uuid::new_v4()));
        let folder = root.join("assets");
        std::fs::create_dir_all(&folder).unwrap();
        let mut files = ["a.png", "b.png", "c.png"]
            .map(|name| {
                let path = folder.join(name);
                image::RgbaImage::new(300, 200).save(&path).unwrap();
                path
            })
            .to_vec();
        let broken = folder.join("d.png");
        std::fs::write(&broken, b"not an image").unwrap();
        files.push(broken.clone());

        let context = egui::Context::default();
        let mut assets = EditorAssetState::default();
        let mut dialogs = FileDialogs::default();
        let mut frame = |assets: &mut EditorAssetState| {
            let output = context.run(egui::RawInput::default(), |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    draw_assets_area(
                        ui,
                        assets,
                        &root.to_string_lossy(),
                        &files,
                        false,
                        &mut dialogs,
                        &mut None,
                    );
                });
            });
            // Thumbnails are painted as textured meshes over the row icon.
            output
                .shapes
                .iter()
                .filter(|clipped| {
                    matches!(&clipped.shape, egui::Shape::Mesh(mesh)
                        if mesh.texture_id != egui::TextureId::default())
                })
                .count()
        };

        assert_eq!(frame(&mut assets), THUMBNAILS_PER_FRAME);
        assert_eq!(frame(&mut assets), 3);
        assert!(assets.thumbnails[&broken].is_none());
        let size = assets.thumbnails[&folder.join("a.png")]
            .as_ref()
            .unwrap()
            .size();
        assert_eq!(size, [96, 64]);
        std::fs::remove_dir_all(root).unwrap();
    }
}

//! The Inspector for a `.rdata` data asset file picked in Assets, like
//! Godot's resource inspector.

use bevy_ecs::prelude::World;

use super::{placement, reflected, widgets};
use crate::assets::DataAssetTypes;
use crate::editor::gui_elements::EditorTheme;
use crate::editor::DataInspection;

/// What the user asked for while the data asset was drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::editor) enum DataAssetAction {
    None,
    /// Write the edited value to the file.
    Save,
    /// Go back to the selected object.
    Close,
}

pub(in crate::editor) fn draw_data_asset(
    ui: &mut egui::Ui,
    world: &World,
    inspection: &mut DataInspection,
) -> DataAssetAction {
    let mut action = DataAssetAction::None;
    ui.horizontal(|ui| {
        let name = inspection
            .path
            .file_name()
            .map_or_else(String::new, |name| {
                name.to_string_lossy().into_owned()
            });
        ui.label(egui::RichText::new(name).strong())
            .on_hover_text(inspection.path.display().to_string());
        ui.with_layout(
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                if ui
                    .small_button("×")
                    .on_hover_text("Back to the selected object")
                    .clicked()
                {
                    action = DataAssetAction::Close;
                }
            },
        );
    });
    if let Some(error) = &inspection.error {
        ui.colored_label(EditorTheme::ERROR, error);
    }
    let Some(registered) = world
        .get_resource::<DataAssetTypes>()
        .and_then(|types| types.get(&inspection.type_name))
    else {
        ui.colored_label(
            EditorTheme::TEXT_MUTED,
            format!(
                "The game does not register `{}`, so this file cannot be \
                 edited here.",
                inspection.type_name
            ),
        );
        return action;
    };
    let label = placement::component_label(&inspection.type_name);
    widgets::section(ui, &label, false, |ui| {
        if reflected::edit_component(
            ui,
            world,
            &registered.info,
            &mut inspection.value,
        ) {
            inspection.dirty = true;
        }
    });
    ui.colored_label(
        EditorTheme::TEXT_MUTED,
        "Edits are saved to the file. Every object that uses it sees them.",
    );
    // A drag writes once, when it ends.
    if action == DataAssetAction::None
        && inspection.dirty
        && !ui.input(|input| input.pointer.any_down())
    {
        action = DataAssetAction::Save;
    }
    action
}

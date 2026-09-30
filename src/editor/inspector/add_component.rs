//! The Add Component picker: one wide button that opens a searchable list of
//! components grouped by kind.

use super::placement::{component_group, component_help};
use crate::editor::gui_elements::{kit, EditorTheme};
use crate::editor::EditorIcon;

pub(super) struct Entry {
    pub label: String,
    pub name: String,
    /// Why the component cannot be added here; `None` when it can.
    pub blocked: Option<String>,
}

pub(super) enum Pick {
    Physics,
    Component(String),
}

/// Group order in the list; unknown groups go last.
const GROUPS: [&str; 5] =
    ["Physics", "Environment", "Engine", "Game", "User Interface"];

pub(super) fn draw(
    ui: &mut egui::Ui,
    entries: &[Entry],
    offer_physics: bool,
) -> Option<Pick> {
    let mut picked = None;
    let id = ui.make_persistent_id("add_component_popup");
    let button = kit::wide_button(ui, EditorIcon::AddObject, "Add Component");
    if button.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(id));
        ui.data_mut(|data| {
            data.insert_temp(id, String::new());
            data.insert_temp(id.with("focus"), String::new());
        });
    }
    egui::popup::popup_below_widget(
        ui,
        id,
        &button,
        egui::popup::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_width(LIST_WIDTH + DETAIL_WIDTH + 12.0);
            let mut query = ui
                .data(|data| data.get_temp::<String>(id))
                .unwrap_or_default();
            let search = kit::search_field(ui, &mut query, "Search components");
            if !search.has_focus() && query.is_empty() {
                search.request_focus();
            }
            ui.data_mut(|data| data.insert_temp(id, query.clone()));
            let query = query.to_lowercase();
            ui.add_space(4.0);

            // One row per pickable thing, in group order.
            let mut rows: Vec<(&'static str, Row)> = Vec::new();
            let mut groups: Vec<&str> = GROUPS.to_vec();
            for entry in entries {
                let group = component_group(&entry.name);
                if !groups.contains(&group) {
                    groups.push(group);
                }
            }
            for group in groups {
                if group == "Physics" && offer_physics {
                    rows.push((group, Row::Physics));
                }
                for entry in entries
                    .iter()
                    .filter(|entry| component_group(&entry.name) == group)
                {
                    rows.push((group, Row::Entry(entry)));
                }
            }
            rows.retain(|(_, row)| {
                query.is_empty()
                    || row.label().to_lowercase().contains(&query)
                    || row.name().to_lowercase().contains(&query)
                    || component_help(row.name())
                        .0
                        .to_lowercase()
                        .contains(&query)
            });

            let focus_key = id.with("focus");
            let mut focus = ui
                .data(|data| data.get_temp::<String>(focus_key))
                .unwrap_or_default();
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(LIST_WIDTH, 340.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        kit::list_well(ui, 0.0, |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(336.0)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    draw_rows(
                                        ui,
                                        &rows,
                                        &mut focus,
                                        &mut picked,
                                    );
                                });
                        });
                    },
                );
                ui.allocate_ui_with_layout(
                    egui::vec2(DETAIL_WIDTH, 340.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        let shown = rows
                            .iter()
                            .map(|(_, row)| row)
                            .find(|row| row.name() == focus)
                            .or_else(|| rows.first().map(|(_, row)| row));
                        if let Some(row) = shown {
                            draw_details(ui, row, &mut picked);
                        } else {
                            ui.colored_label(
                                EditorTheme::TEXT_MUTED,
                                "No component matches.",
                            );
                        }
                    },
                );
            });
            ui.data_mut(|data| data.insert_temp(focus_key, focus));
        },
    );
    if picked.is_some() {
        ui.memory_mut(|memory| memory.close_popup());
    }
    picked
}

const LIST_WIDTH: f32 = 210.0;
const DETAIL_WIDTH: f32 = 290.0;

#[derive(Clone, Copy)]
enum Row<'a> {
    Physics,
    Entry(&'a Entry),
}

impl Row<'_> {
    fn label(&self) -> &str {
        match self {
            Row::Physics => "Physics",
            Row::Entry(entry) => &entry.label,
        }
    }

    /// Registry name; `"physics"` for the physics bundle.
    fn name(&self) -> &str {
        match self {
            Row::Physics => "physics",
            Row::Entry(entry) => &entry.name,
        }
    }

    fn blocked(&self) -> Option<&str> {
        match self {
            Row::Physics => None,
            Row::Entry(entry) => entry.blocked.as_deref(),
        }
    }

    fn pick(&self) -> Pick {
        match self {
            Row::Physics => Pick::Physics,
            Row::Entry(entry) => Pick::Component(entry.name.clone()),
        }
    }
}

fn draw_rows(
    ui: &mut egui::Ui,
    rows: &[(&'static str, Row)],
    focus: &mut String,
    picked: &mut Option<Pick>,
) {
    let mut last_group = "";
    for (group, row) in rows {
        if *group != last_group {
            EditorTheme::menu_section(ui, &group.to_uppercase());
            last_group = group;
        }
        let response =
            EditorTheme::menu_action(ui, row.label(), row.blocked().is_none());
        if response.hovered() || response.has_focus() {
            *focus = row.name().to_owned();
        }
        if row.blocked().is_none() && response.clicked() {
            *picked = Some(row.pick());
        }
    }
}

fn draw_details(ui: &mut egui::Ui, row: &Row, picked: &mut Option<Pick>) {
    let (what, when) = component_help(row.name());
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new(row.label())
            .size(15.0)
            .strong()
            .color(EditorTheme::TEXT),
    );
    ui.label(
        egui::RichText::new(row.name())
            .small()
            .monospace()
            .color(EditorTheme::TEXT_MUTED),
    );
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new("WHAT IT DOES")
            .small()
            .strong()
            .color(EditorTheme::TEXT_MUTED),
    );
    ui.add(egui::Label::new(what).wrap());
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new("USE IT FOR")
            .small()
            .strong()
            .color(EditorTheme::TEXT_MUTED),
    );
    ui.add(egui::Label::new(when).wrap());
    ui.add_space(10.0);
    if let Some(reason) = row.blocked() {
        ui.add(
            egui::Label::new(
                egui::RichText::new(reason).color(EditorTheme::WARNING),
            )
            .wrap(),
        );
    } else if ui
        .add_sized(
            [ui.available_width(), 26.0],
            egui::Button::new(format!("Add {}", row.label())),
        )
        .clicked()
    {
        *picked = Some(row.pick());
    }
}

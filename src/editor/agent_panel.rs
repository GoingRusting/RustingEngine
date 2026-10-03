//! Agent area: a journal of scene changes written by tools outside the editor.

use bevy_ecs::prelude::Resource;
use egui::Ui;

use super::gui_elements::EditorTheme;

/// One outside write the editor reloaded.
#[derive(Clone, Debug)]
pub(super) struct JournalEntry {
    /// Scene file that changed.
    pub(super) path: String,
    /// "N added, N changed, N removed".
    pub(super) summary: String,
    /// Scene IDs of entities the write added or changed.
    pub(super) ids: Vec<uuid::Uuid>,
}

/// Outside writes in the order they arrived, oldest first.
#[derive(Resource, Clone, Debug, Default)]
pub struct AgentJournal {
    pub(super) entries: Vec<JournalEntry>,
}

impl AgentJournal {
    /// The oldest entries are dropped past this count.
    const CAPACITY: usize = 200;

    pub(super) fn record(&mut self, entry: JournalEntry) {
        self.entries.push(entry);
        if self.entries.len() > Self::CAPACITY {
            self.entries.remove(0);
        }
    }
}

/// Draws the journal, newest first. Returns the IDs of the entry the user
/// clicked, so the caller can select those entities.
pub(super) fn draw_agent_area(
    ui: &mut Ui,
    journal: &mut AgentJournal,
) -> Option<Vec<uuid::Uuid>> {
    let mut clicked = None;
    ui.horizontal(|ui| {
        ui.label(format!("Journal ({})", journal.entries.len()));
        if ui.button("Clear").clicked() {
            journal.entries.clear();
        }
    });
    if journal.entries.is_empty() {
        ui.colored_label(
            EditorTheme::TEXT_MUTED,
            "No outside changes yet. Scene writes from tools such as \
             `rusting scene patch` appear here.",
        );
        return None;
    }
    egui::ScrollArea::vertical()
        .id_salt("agent_area")
        .show(ui, |ui| {
            for (index, entry) in journal.entries.iter().enumerate().rev() {
                let label = format!(
                    "#{} {} ({})",
                    index + 1,
                    entry.path,
                    entry.summary
                );
                if ui.selectable_label(false, label).clicked() {
                    clicked = Some(entry.ids.clone());
                }
            }
        });
    clicked
}

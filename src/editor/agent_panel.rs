//! Agent area: a journal of scene changes written by tools outside the
//! editor, and the newest `rusting test` results.

use bevy_ecs::prelude::Resource;
use egui::Ui;

use std::path::Path;

use super::gui_elements::{kit, EditorTheme};

/// One outside write the editor reloaded.
#[derive(Clone, Debug)]
pub(super) struct JournalEntry {
    /// Scene file that changed.
    pub(super) path: String,
    /// "N added, N changed, N removed".
    pub(super) summary: String,
    /// Scene IDs of entities the write added or changed.
    pub(super) ids: Vec<uuid::Uuid>,
    /// Revision of the scene file this entry describes, so Accept applies
    /// only the write the user reviewed.
    pub(super) revision: String,
}

/// One scenario of the newest `rusting test` run.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ResultRow {
    pub(super) file: String,
    pub(super) ok: bool,
    pub(super) message: String,
}

/// Which list the Agent area shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum AgentTab {
    #[default]
    Journal,
    Results,
}

/// Outside writes in the order they arrived, oldest first, and the cached
/// test results.
#[derive(Resource, Clone, Debug, Default)]
pub struct AgentJournal {
    pub(super) entries: Vec<JournalEntry>,
    pub(super) tab: AgentTab,
    /// An outside write the editor has not applied, because the scene had
    /// unsaved edits or agent edits are paused.
    pub(super) pending: Option<JournalEntry>,
    /// Queue outside writes as pending instead of reloading them.
    pub(super) paused: bool,
    /// Set by the Accept and Reject buttons; the scene watcher applies them
    /// on its next poll, behind one Undo snapshot.
    pub(super) accept_requested: bool,
    pub(super) reject_requested: bool,
    /// Results file modification time and its rows, reread when it changes.
    results: Option<(std::time::SystemTime, Vec<ResultRow>)>,
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

/// Rows of a `rusting test` results file; `None` when it is missing or
/// not a results file.
pub(super) fn read_test_results(path: &Path) -> Option<Vec<ResultRow>> {
    let data: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let rows = data["scenarios"].as_array()?.iter().map(|run| ResultRow {
        file: run["file"]
            .as_str()
            .map(|file| {
                Path::new(file).file_name().map_or(file.to_owned(), |name| {
                    name.to_string_lossy().into_owned()
                })
            })
            .unwrap_or_default(),
        ok: run["ok"].as_bool().unwrap_or(false),
        message: run["message"].as_str().unwrap_or_default().to_owned(),
    });
    Some(rows.collect())
}

/// Draws the Journal or Results tab. Returns the IDs of the journal entry
/// the user clicked, so the caller can select those entities.
pub(super) fn draw_agent_area(
    ui: &mut Ui,
    journal: &mut AgentJournal,
    project_root: &str,
) -> Option<Vec<uuid::Uuid>> {
    kit::segmented(
        ui,
        "agent_tab",
        &mut journal.tab,
        &[
            (AgentTab::Journal, "Journal"),
            (AgentTab::Results, "Results"),
        ],
    );
    if journal.tab == AgentTab::Results {
        draw_results(ui, journal, project_root);
        return None;
    }
    let mut clicked = None;
    ui.horizontal(|ui| {
        kit::toggle(ui, &mut journal.paused);
        ui.label("Pause agent edits");
    });
    if let Some(pending) = &journal.pending {
        ui.colored_label(
            EditorTheme::WARNING,
            format!("Pending: {} ({})", pending.path, pending.summary),
        );
        let ids = pending.ids.clone();
        ui.horizontal(|ui| {
            if ui.button("Accept").clicked() {
                journal.accept_requested = true;
            }
            if ui.button("Reject").clicked() {
                journal.reject_requested = true;
            }
            if ui.button("Select").clicked() {
                clicked = Some(ids);
            }
        });
    }
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

/// The newest `rusting test` run, one row per scenario, failures in the
/// error color.
fn draw_results(ui: &mut Ui, journal: &mut AgentJournal, project_root: &str) {
    let path = Path::new(project_root).join(crate::project::TEST_RESULTS_FILE);
    let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    if modified != journal.results.as_ref().map(|(time, _)| *time) {
        journal.results = modified.zip(read_test_results(&path));
    }
    let Some((_, rows)) = &journal.results else {
        ui.colored_label(
            EditorTheme::TEXT_MUTED,
            "No test results yet. Run `rusting test` on this project.",
        );
        return;
    };
    let failed = rows.iter().filter(|row| !row.ok).count();
    ui.label(format!("{} scenarios, {failed} failed", rows.len()));
    egui::ScrollArea::vertical()
        .id_salt("agent_results")
        .show(ui, |ui| {
            for row in rows {
                ui.horizontal(|ui| {
                    if row.ok {
                        kit::pill(ui, "pass", EditorTheme::ACCENT);
                    } else {
                        kit::pill(ui, "fail", EditorTheme::ERROR);
                    }
                    ui.label(&row.file);
                });
                if !row.ok {
                    ui.colored_label(EditorTheme::ERROR, &row.message);
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_results_are_read_from_the_file_rusting_test_writes() {
        let root = std::env::temp_dir()
            .join(format!("rusting-agent-results-{}", std::process::id()));
        let path = root.join(crate::project::TEST_RESULTS_FILE);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        assert_eq!(read_test_results(&path), None);
        std::fs::write(
            &path,
            serde_json::json!({"root": "/games/demo", "scenarios": [
                {"file": "/games/demo/tests/jump.json", "ok": true,
                 "message": "\"jump\" after 60 ticks", "logs": []},
                {"file": "/games/demo/tests/coin.json", "ok": false,
                 "message": "tick 30: expected 1, got 0", "logs": []},
            ]})
            .to_string(),
        )
        .unwrap();
        let rows = read_test_results(&path).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].file, "jump.json");
        assert!(rows[0].ok);
        assert_eq!(rows[1].file, "coin.json");
        assert!(!rows[1].ok);
        assert_eq!(rows[1].message, "tick 30: expected 1, got 0");
        // The panel draws the rows from the project root.
        let mut journal = AgentJournal {
            tab: AgentTab::Results,
            ..AgentJournal::default()
        };
        let context = egui::Context::default();
        let _ = context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                draw_agent_area(ui, &mut journal, root.to_str().unwrap());
            });
        });
        assert_eq!(
            journal.results.as_ref().map(|(_, rows)| rows.len()),
            Some(2)
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}

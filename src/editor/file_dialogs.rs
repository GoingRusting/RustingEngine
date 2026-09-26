//! Native file dialogs that do not block the editor frame loop.
//!
//! A blocking `rfd::FileDialog` call stops the event loop, so the window
//! manager stops getting answers to its pings and reports the editor as not
//! responding. The dialog future is created on the main thread (macOS needs
//! that), driven on a worker thread, and its result is polled each frame.

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::task::{Context, Poll, Wake};
use std::thread::JoinHandle;

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::Resource;

/// What the editor does with the paths a dialog returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DialogPurpose {
    OpenProject,
    ProjectParentFolder,
    CreateProjectIn,
    OpenScene,
    SaveSceneBeforeContinue,
    SaveSceneAs,
    /// `target` is a Cargo target triple, or `None` for this system.
    ExportGame {
        target: Option<&'static str>,
    },
    ImportFiles,
    MaterialTexture {
        entity: Entity,
        slot: usize,
    },
}

struct PendingDialog {
    purpose: DialogPurpose,
    paths: JoinHandle<Vec<PathBuf>>,
}

/// The one native dialog that may be open at a time.
#[derive(Resource, Default)]
pub struct FileDialogs {
    pending: Option<PendingDialog>,
}

impl FileDialogs {
    pub(super) fn is_open(&self) -> bool {
        self.pending.is_some()
    }

    /// Starts `paths` on a worker thread. Ignored while another dialog is
    /// open, like a second click on a modal dialog's parent.
    pub(super) fn open(
        &mut self,
        purpose: DialogPurpose,
        paths: impl Future<Output = Vec<PathBuf>> + Send + 'static,
    ) {
        if self.is_open() {
            return;
        }
        self.pending = Some(PendingDialog {
            purpose,
            paths: std::thread::spawn(move || block_on(paths)),
        });
    }

    pub(super) fn pick_file(
        &mut self,
        purpose: DialogPurpose,
        dialog: rfd::AsyncFileDialog,
    ) {
        let file = dialog.pick_file();
        self.open(purpose, async move { one_path(file.await) });
    }

    pub(super) fn pick_files(
        &mut self,
        purpose: DialogPurpose,
        dialog: rfd::AsyncFileDialog,
    ) {
        let files = dialog.pick_files();
        self.open(purpose, async move {
            files
                .await
                .unwrap_or_default()
                .iter()
                .map(|file| file.path().to_owned())
                .collect()
        });
    }

    pub(super) fn pick_folder(
        &mut self,
        purpose: DialogPurpose,
        dialog: rfd::AsyncFileDialog,
    ) {
        let folder = dialog.pick_folder();
        self.open(purpose, async move { one_path(folder.await) });
    }

    pub(super) fn save_file(
        &mut self,
        purpose: DialogPurpose,
        dialog: rfd::AsyncFileDialog,
    ) {
        let file = dialog.save_file();
        self.open(purpose, async move { one_path(file.await) });
    }

    /// Returns the finished dialog's purpose and chosen paths. The path list
    /// is empty when the dialog was cancelled.
    pub(super) fn poll(&mut self) -> Option<(DialogPurpose, Vec<PathBuf>)> {
        if !self.pending.as_ref()?.paths.is_finished() {
            return None;
        }
        let pending = self.pending.take()?;
        // A panicked worker counts as a cancel.
        let paths = pending.paths.join().unwrap_or_default();
        Some((pending.purpose, paths))
    }
}

fn one_path(file: Option<rfd::FileHandle>) -> Vec<PathBuf> {
    file.map(|file| file.path().to_owned())
        .into_iter()
        .collect()
}

struct ThreadWaker(std::thread::Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// Runs `future` to completion on the current thread.
fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Arc::new(ThreadWaker(std::thread::current())).into();
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        std::thread::park();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(dialogs: &mut FileDialogs) -> (DialogPurpose, Vec<PathBuf>) {
        for _ in 0..500 {
            if let Some(result) = dialogs.poll() {
                return result;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        panic!("dialog never finished");
    }

    #[test]
    fn dialog_results_arrive_by_polling_and_one_dialog_opens_at_a_time() {
        let mut dialogs = FileDialogs::default();
        let (release, gate) = std::sync::mpsc::channel::<()>();
        dialogs.open(DialogPurpose::OpenScene, async move {
            gate.recv().unwrap();
            vec![PathBuf::from("scenes/main.rscene")]
        });
        // A second dialog while the first is open is ignored.
        dialogs.open(DialogPurpose::ImportFiles, async { Vec::new() });
        assert!(dialogs.is_open());
        assert_eq!(dialogs.poll(), None);

        release.send(()).unwrap();
        assert_eq!(
            wait(&mut dialogs),
            (
                DialogPurpose::OpenScene,
                vec![PathBuf::from("scenes/main.rscene")]
            )
        );
        assert!(!dialogs.is_open());

        dialogs.open(DialogPurpose::ExportGame { target: None }, async {
            Vec::new()
        });
        assert_eq!(
            wait(&mut dialogs),
            (DialogPurpose::ExportGame { target: None }, Vec::new())
        );
    }
}

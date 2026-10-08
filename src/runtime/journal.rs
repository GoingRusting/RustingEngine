//! Operation journal: while a command runs inside [`begin`], every project
//! file written through [`super::write_atomic`] is recorded with the
//! command, the tool, the time, and the file's content before and after, so
//! `rusting log` can list operations and `rusting revert` can undo one.
//! Entries are lines of `<project>/.rusting/journal.jsonl`; contents are
//! stored once each in `.rusting/blobs/<hash>`.

use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::lease::{locate, now};

#[derive(Debug, Clone)]
struct Operation {
    id: String,
    command: String,
    tool: String,
    time: u64,
}

thread_local! {
    static CURRENT: RefCell<Option<Operation>> = const { RefCell::new(None) };
}

/// One file change of an operation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct JournalEntry {
    pub op: String,
    pub time: u64,
    pub tool: String,
    pub command: String,
    /// Project-relative, `/` separators.
    pub file: String,
    /// Content hash before the write; `None` when the file was created.
    pub before: Option<String>,
    /// Content hash after; `None` when the file was deleted.
    pub after: Option<String>,
}

/// Ends the operation when dropped.
pub struct OperationGuard(());

impl Drop for OperationGuard {
    fn drop(&mut self) {
        CURRENT.with(|current| current.borrow_mut().take());
    }
}

/// Starts recording writes on this thread as one operation running
/// `command`. The tool is `RUSTING_AGENT`, or `rusting`.
#[must_use]
pub fn begin(command: &str) -> OperationGuard {
    let operation = Operation {
        id: uuid::Uuid::new_v4().simple().to_string()[..8].to_owned(),
        command: command.to_owned(),
        tool: super::lease::current_agent().unwrap_or_else(|| "rusting".into()),
        time: now(),
    };
    CURRENT.with(|current| *current.borrow_mut() = Some(operation));
    OperationGuard(())
}

pub(crate) fn active() -> bool {
    CURRENT.with(|current| current.borrow().is_some())
}

fn journal_file(root: &Path) -> PathBuf {
    root.join(".rusting/journal.jsonl")
}

fn blob(root: &Path, hash: &str) -> PathBuf {
    root.join(".rusting/blobs").join(hash)
}

fn store(root: &Path, bytes: &[u8]) -> std::io::Result<String> {
    let hash = super::scene_revision(bytes);
    let path = blob(root, &hash);
    if !path.is_file() {
        super::lease::state_dir(root, "blobs")?;
        super::write_atomic_unchecked(&path, bytes)?;
    }
    Ok(hash)
}

/// Records that `path` changed from `before` to `after` in the current
/// operation. Does nothing outside an operation or a project, or when the
/// content did not change.
pub(crate) fn record(
    path: &Path,
    before: Option<&[u8]>,
    after: Option<&[u8]>,
) -> std::io::Result<()> {
    let Some(operation) = CURRENT.with(|current| current.borrow().clone())
    else {
        return Ok(());
    };
    let Some((root, file)) = locate(path) else {
        return Ok(());
    };
    if before == after || file.starts_with(".rusting/") {
        return Ok(());
    }
    let entry = JournalEntry {
        op: operation.id,
        time: operation.time,
        tool: operation.tool,
        command: operation.command,
        file,
        before: before.map(|bytes| store(&root, bytes)).transpose()?,
        after: after.map(|bytes| store(&root, bytes)).transpose()?,
    };
    let mut line = serde_json::to_vec(&entry)?;
    line.push(b'\n');
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(journal_file(&root))?
        .write_all(&line)
}

/// Every recorded change of the project at `root`, oldest first.
#[must_use]
pub fn entries(root: &Path) -> Vec<JournalEntry> {
    std::fs::read_to_string(journal_file(root))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// Why [`revert`] did nothing.
#[derive(Debug)]
pub enum RevertError {
    UnknownOperation(String),
    /// A file changed after the operation; `by` is the operation that
    /// changed it, or `None` for a change made outside the journal.
    Changed {
        file: String,
        by: Option<String>,
    },
    Io(std::io::Error),
}

impl From<std::io::Error> for RevertError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Puts back every file operation `op` changed, as one new operation. Fails
/// without writing when any of them changed since.
pub fn revert(root: &Path, op: &str) -> Result<Vec<String>, RevertError> {
    let all = entries(root);
    let Some(position) = all.iter().position(|entry| entry.op == op) else {
        return Err(RevertError::UnknownOperation(op.to_owned()));
    };
    // file -> (content before the operation, content after it)
    let mut files: Vec<(String, Option<String>, Option<String>)> = Vec::new();
    for entry in all.iter().filter(|entry| entry.op == op) {
        // The journal is a plain file: never follow it out of the project.
        let inside = Path::new(&entry.file)
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)));
        let hash_ok = |hash: &Option<String>| {
            hash.as_ref().is_none_or(|hash| {
                !hash.is_empty() && hash.bytes().all(|b| b.is_ascii_hexdigit())
            })
        };
        if !inside || !hash_ok(&entry.before) || !hash_ok(&entry.after) {
            return Err(RevertError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("journal entry for `{}` is malformed", entry.file),
            )));
        }
        match files.iter_mut().find(|(file, ..)| *file == entry.file) {
            Some(change) => change.2.clone_from(&entry.after),
            None => files.push((
                entry.file.clone(),
                entry.before.clone(),
                entry.after.clone(),
            )),
        }
    }
    for (file, _, after) in &files {
        let current = std::fs::read(root.join(file))
            .ok()
            .map(|bytes| super::scene_revision(&bytes));
        if current != *after {
            let by = all[position..]
                .iter()
                .find(|entry| entry.op != op && entry.file == *file)
                .map(|entry| entry.op.clone());
            return Err(RevertError::Changed {
                file: file.clone(),
                by,
            });
        }
    }
    let _operation = begin(&format!("revert {op}"));
    for (file, before, _) in &files {
        let path = root.join(file);
        match before {
            Some(hash) => {
                super::write_atomic(&path, &std::fs::read(blob(root, hash))?)?;
            }
            None => {
                super::lease::check_write(&path)?;
                let old = std::fs::read(&path)?;
                std::fs::remove_file(&path)?;
                record(&path, Some(&old), None)?;
            }
        }
    }
    Ok(files.into_iter().map(|(file, ..)| file).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_in_an_operation_are_logged_and_revert_until_changed_again() {
        let root = std::env::temp_dir()
            .join(format!("rusting-journal-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("project.json"), "{}").unwrap();
        let (scene, new) = (root.join("a.rscene"), root.join("b.json"));
        std::fs::write(&scene, "one").unwrap();
        // Outside an operation nothing is recorded.
        super::super::write_atomic(&scene, b"two").unwrap();
        assert!(entries(&root).is_empty());

        let first = {
            let _op = begin("scene patch a");
            super::super::write_atomic(&scene, b"three").unwrap();
            super::super::write_atomic(&new, b"made").unwrap();
            entries(&root)[0].op.clone()
        };
        let second = {
            let _op = begin("scene patch again");
            super::super::write_atomic(&scene, b"four").unwrap();
            entries(&root)[2].op.clone()
        };
        let logged = entries(&root);
        assert_eq!(logged.len(), 3);
        assert_eq!(logged[0].command, "scene patch a");
        assert_eq!(logged[1].before, None);

        match revert(&root, &first) {
            Err(RevertError::Changed { file, by }) => {
                assert_eq!(
                    (file.as_str(), by),
                    ("a.rscene", Some(second.clone()))
                );
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(revert(&root, &second).unwrap(), ["a.rscene"]);
        assert_eq!(std::fs::read(&scene).unwrap(), b"three");
        revert(&root, &first).unwrap();
        assert_eq!(std::fs::read(&scene).unwrap(), b"two");
        assert!(!new.exists());
        assert_eq!(entries(&root).len(), 6);
        assert_eq!(
            std::fs::read_to_string(root.join(".rusting/.gitignore")).unwrap(),
            "*\n"
        );
        let mut journal = std::fs::OpenOptions::new()
            .append(true)
            .open(journal_file(&root))
            .unwrap();
        writeln!(journal, r#"{{"op":"evil","time":0,"tool":"x","command":"x","file":"../escape","before":null,"after":null}}"#).unwrap();
        assert!(matches!(revert(&root, "evil"), Err(RevertError::Io(_))));
        std::fs::remove_dir_all(root).unwrap();
    }
}

//! Scoped leases: an agent claims a file or folder of a project, and every
//! scene write through [`super::write_atomic`] by anyone else is refused
//! with the holder named until the lease is released or expires. Leases live
//! in `<project>/.rusting/leases.json`; the holder is `RUSTING_AGENT`.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Environment variable naming the agent that claims and writes.
pub const AGENT_ENV: &str = "RUSTING_AGENT";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Lease {
    /// Project-relative path with `/` separators; a folder covers its files.
    pub path: String,
    pub holder: String,
    /// Unix seconds.
    pub expires: u64,
}

/// The agent name from `RUSTING_AGENT`, if set and not empty.
#[must_use]
pub fn current_agent() -> Option<String> {
    std::env::var(AGENT_ENV)
        .ok()
        .filter(|name| !name.is_empty())
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs())
}

/// The project root holding `path` (the nearest folder with
/// `project.json`) and `path` relative to it.
pub(crate) fn locate(path: &Path) -> Option<(PathBuf, String)> {
    let path = std::path::absolute(path).ok()?;
    let root = path
        .ancestors()
        .skip(1)
        .find(|folder| folder.join("project.json").is_file())?;
    let relative = path.strip_prefix(root).ok()?;
    let relative = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    Some((root.to_owned(), relative))
}

fn file(root: &Path) -> PathBuf {
    root.join(".rusting/leases.json")
}

/// Live leases of the project at `root`.
#[must_use]
pub fn leases(root: &Path) -> Vec<Lease> {
    let now = now();
    std::fs::read(file(root))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Lease>>(&bytes).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|lease| lease.expires > now)
        .collect()
}

fn covers(lease: &str, path: &str) -> bool {
    lease.is_empty()
        || path == lease
        || path.starts_with(&format!("{}/", lease.trim_end_matches('/')))
}

fn held_error(lease: &Lease) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::ResourceBusy,
        format!(
            "`{}` is leased by `{}` for {} more seconds",
            lease.path,
            lease.holder,
            lease.expires.saturating_sub(now())
        ),
    )
}

/// Refuses a write to `path` that another agent's lease covers.
pub fn check_write(path: &Path) -> std::io::Result<()> {
    let Some((root, relative)) = locate(path) else {
        return Ok(());
    };
    if !file(&root).is_file() {
        return Ok(());
    }
    let me = current_agent();
    match leases(&root).iter().find(|lease| {
        Some(&lease.holder) != me.as_ref()
            && (covers(&lease.path, &relative)
                || covers(&relative, &lease.path))
    }) {
        Some(lease) if covers(&lease.path, &relative) => Err(held_error(lease)),
        _ => Ok(()),
    }
}

/// Runs `change` on the project's live leases under a lock file and saves
/// the result.
fn update<T>(
    root: &Path,
    change: impl FnOnce(&mut Vec<Lease>) -> std::io::Result<T>,
) -> std::io::Result<T> {
    std::fs::create_dir_all(root.join(".rusting"))?;
    let lock = root.join(".rusting/leases.lock");
    let mut tries = 0;
    // ponytail: a lock file left by a killed process blocks claims until it
    // is deleted; switch to an OS file lock if that happens in practice.
    while std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
        .is_err()
    {
        tries += 1;
        if tries > 200 {
            return Err(std::io::Error::other(format!(
                "{} stayed locked; delete it if no rusting process is running",
                lock.display()
            )));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut list = leases(root);
    let result = change(&mut list).and_then(|value| {
        super::write_atomic_unchecked(
            file(root),
            &serde_json::to_vec_pretty(&list)?,
        )?;
        Ok(value)
    });
    let _ = std::fs::remove_file(&lock);
    result
}

/// Claims `path` (a file or folder) for `holder` for `duration`. Fails with
/// the other holder named when a live lease overlaps it; claiming again
/// renews it.
pub fn claim(
    path: &Path,
    holder: &str,
    duration: Duration,
) -> std::io::Result<Lease> {
    let (root, relative) = locate(path).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} is not inside a project", path.display()),
        )
    })?;
    update(&root, |list| {
        if let Some(lease) = list.iter().find(|lease| {
            lease.holder != holder
                && (covers(&lease.path, &relative)
                    || covers(&relative, &lease.path))
        }) {
            return Err(held_error(lease));
        }
        list.retain(|lease| lease.path != relative);
        let lease = Lease {
            path: relative.clone(),
            holder: holder.to_owned(),
            expires: now() + duration.as_secs().max(1),
        };
        list.push(lease.clone());
        list.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(lease)
    })
}

/// Releases `holder`'s lease on `path`. Releasing a lease that is not there
/// succeeds; another agent's lease fails with the holder named.
pub fn release(path: &Path, holder: &str) -> std::io::Result<()> {
    let Some((root, relative)) = locate(path) else {
        return Ok(());
    };
    update(&root, |list| {
        if let Some(lease) = list
            .iter()
            .find(|lease| lease.path == relative && lease.holder != holder)
        {
            return Err(held_error(lease));
        }
        list.retain(|lease| lease.path != relative);
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lease_refuses_other_writers_until_released() {
        let root = std::env::temp_dir()
            .join(format!("rusting-lease-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("scenes")).unwrap();
        std::fs::write(root.join("project.json"), "{}").unwrap();
        let scene = root.join("scenes/main.rscene");
        let hour = Duration::from_secs(3600);

        claim(&root.join("scenes"), "alice", hour).unwrap();
        let refused = claim(&scene, "bob", hour).unwrap_err();
        assert!(refused.to_string().contains("leased by `alice`"));
        assert!(release(&root.join("scenes"), "bob").is_err());
        assert_eq!(leases(&root).len(), 1);
        // Not leased: a sibling file, or any file outside a project.
        assert!(check_write(&root.join("project.json")).is_ok());
        assert!(check_write(&std::env::temp_dir().join("x.rscene")).is_ok());

        release(&root.join("scenes"), "alice").unwrap();
        claim(&scene, "bob", hour).unwrap();
        assert_eq!(leases(&root)[0].path, "scenes/main.rscene");
        std::fs::remove_dir_all(root).unwrap();
    }
}

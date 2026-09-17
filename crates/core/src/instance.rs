//! One launcher at a time.
//!
//! Two running instances do not see each other: both read the mod list,
//! the profiles and `pak_config.yaml` into memory, and whichever writes
//! last silently discards what the other one did. The guard against that
//! is a single lock file in the state directory, held for as long as the
//! process lives.
//!
//! It is an *advisory lock* on the file, not the file's mere existence,
//! and that is the whole point: the kernel drops such a lock when the
//! process ends, whether it ended cleanly, was killed, or died with the
//! machine. The obvious alternative — a file holding the PID — needs a
//! staleness check of its own ("is 4711 still alive?"), which reads
//! differently on every platform and is wrong precisely when the number
//! has meanwhile been handed to some unrelated process. A launcher that
//! locks itself out after a crash would be the worse failure by far.
//!
//! For the same reason the file is never deleted. Removing it on exit
//! would open a race in which one process unlinks the file another has
//! just opened, leaving two locks on two different inodes and both
//! instances convinced they are alone. An empty file left behind costs
//! nothing; the lock on it, not its presence, is the signal.
//!
//! The locking itself comes from `fs4`, which wraps `flock` on Unix and
//! `LockFileEx` on Windows behind one signature — so this is one of the
//! few places that could have needed a `cfg` and does not. The standard
//! library has offered the same thing since Rust 1.89 (`File::try_lock`),
//! but `sm2-core` is deliberately kept buildable against 1.85, and a
//! single small dependency is the cheaper of the two prices.

use crate::error::{Error, Result};
use crate::paths::AppDirs;
use fs4::{FileExt, TryLockError};
use std::fs::{File, OpenOptions};
use std::path::Path;

/// The lock file's name inside the state directory.
const LOCK_FILE: &str = "instance.lock";

/// Proof that no second instance is running.
///
/// The lock lasts exactly as long as this value: dropping it, or ending
/// the process in any way at all, releases it. Callers therefore have to
/// keep it alive for the whole run — a `let _ = ...` would release it on
/// the spot and is the one mistake this type invites.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    /// Takes the lock for this process.
    ///
    /// Three outcomes, and the difference between the last two is
    /// deliberate:
    ///
    /// - `Ok(Some(lock))` — we are alone, the lock is held.
    /// - `Err(Error::AlreadyRunning)` — another instance has it. The only
    ///   case in which the caller must give up.
    /// - `Ok(None)` — the lock could not be taken *at all*, because the
    ///   state directory or the file in it is unusable. A warning goes to
    ///   the log and the program carries on unprotected.
    ///
    /// That last case is a decision, not an oversight, and it matches how
    /// `paths::migrate_legacy_dir` treats its own failures: a launcher
    /// that refuses to start because of a helper file it does not even
    /// need for the task at hand is a worse outcome than the unlikely
    /// second instance it would have prevented.
    pub fn acquire(dirs: &AppDirs) -> Result<Option<Self>> {
        let path = dirs.state.join(LOCK_FILE);

        let file = match open_lock_file(&path) {
            Ok(file) => file,
            Err(source) => {
                tracing::warn!(
                    path = %path.display(),
                    %source,
                    "lock file unusable, running without protection against a second instance"
                );
                return Ok(None);
            }
        };

        // Called through the trait on purpose: on a compiler from 1.89 on,
        // `file.try_lock()` would silently pick the standard library's
        // inherent method instead, and with it a `TryLockError` of a
        // different type — which is exactly the 1.89 this crate must not
        // require.
        match FileExt::try_lock(&file) {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(TryLockError::WouldBlock) => Err(Error::AlreadyRunning),
            Err(TryLockError::Error(source)) => {
                // Not contention — that arrives as `WouldBlock`. This is a
                // filesystem that cannot lock at all, some network mounts
                // among them. Same reasoning as above: warn, run.
                tracing::warn!(
                    path = %path.display(),
                    %source,
                    "lock could not be taken, running without protection against a second instance"
                );
                Ok(None)
            }
        }
    }
}

/// Opens the lock file, creating the state directory if this is the first
/// start. `truncate(false)` because the file's contents are irrelevant —
/// emptying it would be a write to a file another instance may be holding
/// a lock on.
fn open_lock_file(path: &Path) -> std::io::Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AppDirs;
    use std::path::Path;

    /// Base directories below a temporary directory. Only `state` is ever
    /// touched here; the other two are filled in so the fixture cannot be
    /// mistaken for a partially built `AppDirs`.
    fn dirs_below(root: &Path) -> AppDirs {
        AppDirs {
            config: root.join("config"),
            data: root.join("data"),
            state: root.join("state"),
        }
    }

    #[test]
    fn acquires_the_lock_when_the_state_directory_does_not_exist_yet() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs_below(root.path());

        let lock = InstanceLock::acquire(&dirs).unwrap();

        assert!(lock.is_some(), "a fresh installation must be allowed to start");
        assert!(dirs.state.is_dir(), "the state directory was not created");
    }

    #[test]
    fn refuses_a_second_lock_while_the_first_one_is_alive() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs_below(root.path());
        let _first = InstanceLock::acquire(&dirs).unwrap();

        let second = InstanceLock::acquire(&dirs);

        assert!(
            matches!(second, Err(Error::AlreadyRunning)),
            "a second instance must be turned away, got {second:?}"
        );
    }

    #[test]
    fn frees_the_lock_once_the_guard_is_dropped() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs_below(root.path());
        let first = InstanceLock::acquire(&dirs).unwrap();

        drop(first);

        assert!(
            InstanceLock::acquire(&dirs).is_ok(),
            "the lock outlived its guard"
        );
    }

    #[test]
    fn starts_anyway_when_the_lock_file_cannot_be_created() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs_below(root.path());
        // A plain file where the state directory belongs: `create_dir_all`
        // cannot get past it on any platform.
        std::fs::write(&dirs.state, "not a directory").unwrap();

        let lock = InstanceLock::acquire(&dirs).unwrap();

        assert!(
            lock.is_none(),
            "an unusable state directory must not stop the program, only the lock"
        );
    }
}

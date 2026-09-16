use super::Platform;
use crate::error::{Error, Result};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub struct Unix;

impl Unix {
    fn home() -> PathBuf {
        std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
    }

    /// Looks for `umu-run` in the PATH. Needed to launch the Windows
    /// executable in the existing Proton prefix without Steam.
    pub fn umu_launcher() -> Option<PathBuf> {
        which_in_path("umu-run")
    }
}

impl Platform for Unix {
    fn steam_roots() -> Vec<PathBuf> {
        let home = Self::home();
        vec![
            home.join(".local/share/Steam"),
            home.join(".steam/steam"),
            home.join(".steam/root"),
            home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
        ]
    }

    fn user_profile_root(app_id: u32, library: &Path) -> PathBuf {
        library
            .join("steamapps/compatdata")
            .join(app_id.to_string())
            .join("pfx/drive_c/users/steamuser")
    }

    fn launch_via_steam(app_id: u32) -> Result<()> {
        let url = format!("steam://rungameid/{app_id}");
        // A spawn error here always concerns `xdg-open`, never the
        // `steam://` URL: at this point the operating system only tries to
        // start the opener itself, and the URL is merely handed to it as an
        // argument. `Error::io` expects a path that names the resource
        // actually affected — that is `xdg-open` here, not the URL, which
        // rendered as a "path" would produce a nonsensical message like
        // "E/A-Fehler bei steam://…".
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| Error::io("xdg-open", e))?;
        Ok(())
    }

    fn launch_direct(exe: &Path, env: &[(&str, &str)]) -> Result<()> {
        let umu = Self::umu_launcher().ok_or_else(|| {
            Error::io(
                exe,
                std::io::Error::new(std::io::ErrorKind::NotFound, crate::t!("error.umu_run_not_found")),
            )
        })?;

        let work_dir = exe.parent().unwrap_or(Path::new("."));
        let mut cmd = std::process::Command::new(&umu);
        cmd.arg(exe).current_dir(work_dir);
        for (k, v) in env {
            cmd.env(k, v);
        }
        // A spawn error here always concerns `umu`, never `exe`: at this
        // point the operating system does not even look at `exe`, it only
        // tries to start the launcher itself.
        cmd.spawn().map_err(|e| Error::io(&umu, e))?;
        Ok(())
    }

    fn open_folder(path: &Path) -> Result<()> {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|e| Error::io(path, e))?;
        Ok(())
    }

    fn find_tool(name: &str) -> Option<PathBuf> {
        which_in_path(name)
    }

    fn direct_launch_available() -> bool {
        Self::umu_launcher().is_some()
    }

    /// Only a process whose `/proc/<pid>/comm` reads exactly `steam` counts
    /// as detected. Helper processes such as `steamwebhelper` deliberately
    /// do not: they are browser subprocesses without cloud synchronisation
    /// of their own, and matching on "contains steam" alone would fire for
    /// every `steamwebhelper` or `steamerrorreporter` and make the warning
    /// worthless. If `/proc` is missing (on a system without procfs, for
    /// instance), `false` is returned instead of an error — the
    /// verification is a precaution, not a hard requirement.
    fn steam_is_running() -> bool {
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return false;
        };
        for entry in entries.filter_map(std::result::Result::ok) {
            let comm = entry.path().join("comm");
            if let Ok(name) = std::fs::read_to_string(&comm) {
                if name.trim() == "steam" {
                    return true;
                }
            }
        }
        false
    }
}

/// Checks whether `path` holds a regular file with the execute bit set.
/// Without this verification a file of the same name in the PATH that is
/// not executable would count as a hit, and `launch_direct` would then fail
/// with a raw spawn error instead of the helpful "umu-launcher is missing"
/// message.
fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Minimal PATH lookup — avoids a dependency for twenty lines.
pub(crate) fn which_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    which_in(name, &path)
}

/// PATH lookup over an explicit PATH value rather than the real process
/// environment, so the logic can be tested without touching `$PATH`.
fn which_in(name: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_profile_root_points_into_the_proton_prefix() {
        let library = Path::new("/spiele/SteamLibrary");
        let root = Unix::user_profile_root(2183900, library);

        assert_eq!(
            root,
            PathBuf::from(
                "/spiele/SteamLibrary/steamapps/compatdata/2183900/pfx/drive_c/users/steamuser"
            )
        );
    }

    #[test]
    fn steam_roots_contains_the_usual_locations() {
        let roots = Unix::steam_roots();
        let as_text: Vec<String> =
            roots.iter().map(|p| p.display().to_string()).collect();

        assert!(as_text.iter().any(|p| p.ends_with(".local/share/Steam")));
        assert!(as_text.iter().any(|p| p.ends_with(".steam/steam")));
        assert!(
            as_text.iter().any(|p| p.contains("com.valvesoftware.Steam")),
            "Flatpak Steam has to be taken into account"
        );
    }

    /// `Platform::find_tool` is a pure forward to
    /// `which_in_path`/`which_in`, and their PATH lookup logic itself is
    /// tested hermetically against an explicit PATH value below
    /// (`which_in_finds_executable_candidate` and friends). Testing
    /// `find_tool` against the real `$PATH` would mean changing that
    /// process-wide state globally, which would be unsafe in parallel with
    /// other tests — hence deliberately no test of its own here, only the
    /// forward itself (one line, see `impl Platform for Unix`).
    ///
    /// Pure smoke test: whether or not `/proc` exists and a Steam process
    /// is running, the call must not crash. The actual detection behaviour
    /// depends on the running system and cannot be checked
    /// deterministically without mocking processes.
    #[test]
    fn steam_is_running_does_not_panic() {
        let _ = Unix::steam_is_running();
    }

    #[test]
    fn which_in_finds_executable_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let candidate = dir.path().join("umu-run");
        std::fs::write(&candidate, "#!/bin/sh\n").unwrap();
        let mut perms = std::fs::metadata(&candidate).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&candidate, perms).unwrap();

        let found = which_in("umu-run", dir.path().as_os_str());

        assert_eq!(found, Some(candidate));
    }

    #[test]
    fn which_in_skips_non_executable_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let candidate = dir.path().join("umu-run");
        // The default permissions of `fs::write` are not executable
        // (0o644) — exactly the case `is_executable` has to catch.
        std::fs::write(&candidate, "#!/bin/sh\n").unwrap();

        let found = which_in("umu-run", dir.path().as_os_str());

        assert!(
            found.is_none(),
            "a non-executable file must not count as a hit"
        );
    }
}

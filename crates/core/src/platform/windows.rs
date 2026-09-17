//! The Windows side of `Platform`.
//!
//! The counterpart to `unix.rs`, and deliberately its mirror image: where
//! Linux reaches into a Proton prefix, Windows uses the real user profile,
//! and where Linux hands work to `xdg-open`, Windows hands it to
//! `explorer.exe`. Everything above this module — the load order, the
//! profiles, the save backups — is the same code on both.
//!
//! Untested on real hardware at the time of writing. It compiles in CI and
//! the logic that can be checked without a Windows machine is covered
//! below, but nothing here has yet started a game or found a savegame on a
//! machine that actually has Space Marine 2 installed.

use super::Platform;
use crate::error::{Error, Result};
use std::ffi::OsStr;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Windows;

/// Keeps a helper process from flashing up a console window. The launcher
/// is a console application (it is a command line tool as well as a
/// graphical one), so a child process started from the interface would
/// otherwise open a second, black window for the fraction of a second it
/// runs.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// What `PATHEXT` is assumed to hold when it is not set. Only relevant on
/// a system where something has cleared it; Windows itself always sets it.
const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

impl Windows {
    /// `%USERPROFILE%`, or an empty path when it is unset — the same
    /// treatment `unix.rs` gives a missing `$HOME`. An empty path fails
    /// the `is_dir()` check in `save_dir` and turns into a proper error
    /// there rather than a panic here.
    fn user_profile() -> PathBuf {
        std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default()
    }
}

impl Platform for Windows {
    /// Only the fallback for the case where `steamlocate` finds nothing at
    /// all (see `GamePaths::discover`). On Windows `steamlocate` reads
    /// Steam's own registry key, which is more reliable than any guess
    /// about the program directory — these two are what is left when even
    /// that fails.
    fn steam_roots() -> Vec<PathBuf> {
        let mut roots = Vec::new();
        // The 32-bit program directory first: Steam is a 32-bit
        // application and installs there by default, so on a normal
        // installation this is the hit and the second entry is never
        // reached.
        for variable in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Some(dir) = std::env::var_os(variable) {
                roots.push(PathBuf::from(dir).join("Steam"));
            }
        }
        roots
    }

    /// On Windows the savegames sit under the real user profile. Both
    /// parameters describe the detour Linux has to take through the Proton
    /// prefix and have no counterpart here — the relative path below it
    /// (`AppData/Local/Saber/...`, see `GamePaths::save_dir`) is identical
    /// on both systems, which is the whole reason this trait method
    /// returns a root rather than a finished path.
    fn user_profile_root(_app_id: u32, _library: &Path) -> PathBuf {
        Self::user_profile()
    }

    fn launch_via_steam(app_id: u32) -> Result<()> {
        let url = format!("steam://rungameid/{app_id}");
        // `explorer.exe` rather than `cmd /C start`: the URL then reaches
        // the registered protocol handler without a detour through
        // `cmd.exe`, whose own quoting rules differ from the ones
        // `Command` applies to its arguments — a mismatch that is a
        // standing source of bugs on Windows even when, as here, the URL
        // is built from a constant and carries nothing worth injecting.
        //
        // As in `unix.rs`: a spawn error concerns the opener, never the
        // URL, which at this point is only an argument being handed over.
        Command::new("explorer.exe")
            .arg(&url)
            .spawn()
            .map_err(|e| Error::io("explorer.exe", e))?;
        Ok(())
    }

    /// Windows needs no `umu-run`: the executable is a Windows program on
    /// a Windows machine, so it is simply started. The environment
    /// variables are passed on unchanged — `launch::no_eac_env` adds
    /// `WINEPREFIX` only when a Proton prefix actually exists on disk, so
    /// the one variable that would be meaningless here never arrives.
    fn launch_direct(exe: &Path, env: &[(&str, &str)]) -> Result<()> {
        let work_dir = exe.parent().unwrap_or(Path::new("."));
        let mut cmd = Command::new(exe);
        cmd.current_dir(work_dir);
        for (key, value) in env {
            cmd.env(key, value);
        }
        cmd.spawn().map_err(|e| Error::io(exe, e))?;
        Ok(())
    }

    fn open_folder(path: &Path) -> Result<()> {
        // `explorer.exe` answers with exit code 1 even when it has opened
        // the window. Harmless here, because `spawn` does not wait for the
        // status — but it is the reason this must not be turned into
        // `status()` later on.
        Command::new("explorer.exe").arg(path).spawn().map_err(|e| Error::io(path, e))?;
        Ok(())
    }

    fn find_tool(name: &str) -> Option<PathBuf> {
        which_in_path(name)
    }

    /// Always available. The EAC-less launch means starting the retail
    /// executable directly, and nothing extra has to be installed for that
    /// — unlike on Linux, where it depends on `umu-run` being present.
    fn direct_launch_available() -> bool {
        true
    }

    /// Asks `tasklist` for a process called exactly `steam.exe`.
    ///
    /// The Windows counterpart to reading `/proc` on Linux, and it keeps
    /// that side's sharpness: `IMAGENAME eq steam.exe` is an exact match,
    /// so `steamwebhelper.exe` does not set it off. Matching loosely would
    /// make the warning fire whenever Steam's browser subprocesses are
    /// around and thereby worthless.
    ///
    /// Deliberately no process enumeration through the Windows API and no
    /// crate for it: this is a precaution, not a guarantee — `saves`
    /// always takes and verifies its own backup before a restore either
    /// way — and `tasklist` is part of Windows. A `false` from a failed
    /// call therefore costs a warning, not the safety net.
    ///
    /// The output is searched for the image name rather than parsed. When
    /// the filter matches nothing, `tasklist` prints a notice that is
    /// translated into the system's language and must not be compared
    /// against; the image name in a hit is not translated.
    fn steam_is_running() -> bool {
        let output = Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq steam.exe", "/NH", "/FO", "CSV"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        match output {
            Ok(out) => String::from_utf8_lossy(&out.stdout).to_lowercase().contains("steam.exe"),
            Err(_) => false,
        }
    }
}

/// PATH lookup over the real environment.
fn which_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let pathext =
        std::env::var_os("PATHEXT").unwrap_or_else(|| std::ffi::OsString::from(DEFAULT_PATHEXT));
    which_in(name, &path, &pathext)
}

/// The lookup itself, over explicit `PATH` and `PATHEXT` values so it can
/// be tested without touching the process environment — the same split
/// `unix.rs` makes, and for the same reason: changing those variables
/// globally would be unsafe next to tests running in parallel.
///
/// Windows knows no execute bit, so unlike the Unix side there is nothing
/// to check beyond "is a file". What takes its place is `PATHEXT`: a bare
/// `7z` has to find `7z.exe`.
///
/// A name that already carries an extension is looked up as it stands.
/// That reads an unusual tool name like `libfoo-1.2` as having the
/// extension `2` — accepted, because the names actually looked up here
/// (`unar`, `7z`) carry none, and the alternative would be guessing which
/// dots are extensions.
fn which_in(name: &str, path: &OsStr, pathext: &OsStr) -> Option<PathBuf> {
    let extensions: Vec<String> = pathext
        .to_string_lossy()
        .split(';')
        .filter(|extension| !extension.is_empty())
        .map(str::to_owned)
        .collect();
    let named_already = Path::new(name).extension().is_some();

    for dir in std::env::split_paths(path) {
        if named_already {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
            continue;
        }
        for extension in &extensions {
            let candidate = dir.join(format!("{name}{extension}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows file names are case-insensitive, and `which_in` returns the
    /// name it assembled from `PATHEXT` (".EXE"), not the spelling that is
    /// on disk (".exe"). Both open the very same file, so an assertion
    /// that compares the strings byte for byte is not testing the lookup —
    /// it is testing how `PATHEXT` happens to be capitalised.
    fn assert_same_file(found: Option<PathBuf>, expected: &Path) {
        let found = found.expect("the tool must be found");
        assert_eq!(
            found.to_string_lossy().to_lowercase(),
            expected.to_string_lossy().to_lowercase(),
            "found {} , expected {}",
            found.display(),
            expected.display()
        );
    }

    /// The counterpart to `user_profile_root_points_into_the_proton_prefix`
    /// on the Unix side: there the library is the whole answer, here it is
    /// none of it.
    ///
    /// Compared against each other rather than against a literal path, so
    /// that nothing has to set `%USERPROFILE%`: changing an environment
    /// variable is process-wide, and `cargo test` runs these next to each
    /// other in threads.
    #[test]
    fn user_profile_root_ignores_the_library_and_the_app_id() {
        let one = Windows::user_profile_root(2183900, Path::new(r"D:\SteamLibrary"));
        let other = Windows::user_profile_root(1, Path::new(r"E:\Somewhere\Else"));

        assert_eq!(one, other, "neither library nor app id may reach the result");
        assert_eq!(one, Windows::user_profile());
    }

    #[test]
    fn steam_roots_look_in_both_program_directories() {
        let roots = Windows::steam_roots();
        let as_text: Vec<String> = roots.iter().map(|p| p.display().to_string()).collect();

        assert!(!roots.is_empty(), "at least one program directory must be known");
        assert!(
            as_text.iter().all(|p| p.ends_with("Steam")),
            "every root has to end in the Steam directory: {as_text:?}"
        );
    }

    /// A launch without EAC needs no extra tool here — the opposite of the
    /// Linux side, where it stands or falls with `umu-run`.
    #[test]
    fn a_direct_launch_needs_nothing_extra() {
        assert!(Windows::direct_launch_available());
    }

    #[test]
    fn steam_is_running_does_not_panic() {
        let _ = Windows::steam_is_running();
    }

    #[test]
    fn which_in_appends_the_extensions_from_pathext() {
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join("7z.exe");
        std::fs::write(&tool, b"MZ").unwrap();

        let found = which_in("7z", dir.path().as_os_str(), OsStr::new(".COM;.EXE"));

        assert_same_file(found, &tool);
    }

    /// The order in `PATHEXT` decides, not the order on disk.
    #[test]
    fn which_in_honours_the_order_of_pathext() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tool.cmd"), b"@echo off").unwrap();
        std::fs::write(dir.path().join("tool.exe"), b"MZ").unwrap();

        let found = which_in("tool", dir.path().as_os_str(), OsStr::new(".EXE;.CMD"));

        assert_same_file(found, &dir.path().join("tool.exe"));
    }

    #[test]
    fn which_in_takes_a_name_that_already_carries_an_extension_as_it_stands() {
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join("7z.exe");
        std::fs::write(&tool, b"MZ").unwrap();

        let found = which_in("7z.exe", dir.path().as_os_str(), OsStr::new(".COM;.EXE"));

        assert_same_file(found, &tool);
    }

    #[test]
    fn which_in_reports_nothing_for_a_tool_that_is_not_there() {
        let dir = tempfile::tempdir().unwrap();

        let found = which_in("nowhere", dir.path().as_os_str(), OsStr::new(".EXE"));

        assert!(found.is_none());
    }

    /// A directory of the right name must not count as a hit: `Command`
    /// would then fail with a raw spawn error instead of the
    /// "tool is missing" message the caller is prepared for.
    #[test]
    fn which_in_does_not_mistake_a_directory_for_a_tool() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("7z.exe")).unwrap();

        let found = which_in("7z", dir.path().as_os_str(), OsStr::new(".EXE"));

        assert!(found.is_none());
    }
}

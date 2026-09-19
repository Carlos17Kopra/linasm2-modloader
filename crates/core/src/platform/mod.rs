use crate::error::Result;
use std::path::{Path, PathBuf};

#[cfg(unix)]
pub mod unix;
#[cfg(windows)]
pub mod windows;

/// The project's entire platform-dependent surface.
///
/// Windows and Linux do not differ in the path logic, only in its root: the
/// Proton prefix is an alternative `C:\`.
pub trait Platform {
    /// Places where a Steam installation can live.
    fn steam_roots() -> Vec<PathBuf>;

    /// The root below which `AppData/Local/...` lives.
    ///
    /// Linux: `<library>/steamapps/compatdata/<app_id>/pfx/drive_c/users/steamuser`
    /// Windows: `%USERPROFILE%`
    fn user_profile_root(app_id: u32, library: &Path) -> PathBuf;

    /// Launches the game the regular way, through Steam.
    fn launch_via_steam(app_id: u32) -> Result<()>;

    /// Launches the executable bypassing Steam (EAC bypass).
    fn launch_direct(exe: &Path, env: &[(&str, &str)]) -> Result<()>;

    /// Opens a directory in the file manager.
    fn open_folder(path: &Path) -> Result<()>;

    /// Looks for a command line tool (`unar`, `7z`, say) in the PATH.
    fn find_tool(name: &str) -> Option<PathBuf>;

    /// Is a direct launch bypassing Steam (EAC bypass) possible on this
    /// system at all — regardless of whether it is actually wanted in a
    /// given call?
    fn direct_launch_available() -> bool;

    /// Is the Steam client running right now? Relevant for spec §6.5/§9 R2:
    /// cloud synchronisation can overwrite a restored save in the
    /// background.
    fn steam_is_running() -> bool;

    /// How a found update is applied here.
    fn update_method() -> UpdateMethod;

    /// Opens a URL in whatever the user browses with.
    fn open_url(url: &str) -> Result<()>;
}

/// How a found update is applied here. It describes the platform, so it
/// lives with the trait rather than in `update`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateMethod {
    /// Download the release's `install.sh`, verify it, run it.
    Installer,
    /// No installer exists here — send the user to the release page.
    ReleasePage,
}

#[cfg(unix)]
pub type Current = unix::Unix;
#[cfg(windows)]
pub type Current = windows::Windows;

#[cfg(test)]
mod tests {
    use super::*;

    /// Linux installs by running the release's own `install.sh`; Windows
    /// has no such script and can only send the user to the release
    /// page. This is the one genuinely platform-dependent piece of the
    /// update path, and the reason it sits behind the trait instead of
    /// behind a `cfg` somewhere above it.
    #[test]
    fn every_platform_says_how_it_updates() {
        let method = Current::update_method();
        if cfg!(unix) {
            assert_eq!(method, UpdateMethod::Installer);
        } else {
            assert_eq!(method, UpdateMethod::ReleasePage);
        }
    }
}

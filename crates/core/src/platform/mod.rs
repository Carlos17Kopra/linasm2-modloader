use crate::error::Result;
use std::path::{Path, PathBuf};

#[cfg(unix)]
pub mod unix;

/// Die gesamte plattformabhängige Fläche des Projekts.
///
/// Windows und Linux unterscheiden sich nicht in der Pfadlogik, sondern nur
/// in deren Wurzel: der Proton-Prefix ist ein alternatives `C:\`.
pub trait Platform {
    /// Orte, an denen eine Steam-Installation liegen kann.
    fn steam_roots() -> Vec<PathBuf>;

    /// Wurzel, unterhalb derer `AppData/Local/...` liegt.
    ///
    /// Linux: `<library>/steamapps/compatdata/<app_id>/pfx/drive_c/users/steamuser`
    /// Windows: `%USERPROFILE%`
    fn user_profile_root(app_id: u32, library: &Path) -> PathBuf;

    /// Startet das Spiel regulär über Steam.
    fn launch_via_steam(app_id: u32) -> Result<()>;

    /// Startet die Executable unter Umgehung von Steam (EAC-Bypass).
    fn launch_direct(exe: &Path, env: &[(&str, &str)]) -> Result<()>;

    /// Öffnet ein Verzeichnis im Dateimanager.
    fn open_folder(path: &Path) -> Result<()>;
}

#[cfg(unix)]
pub type Current = unix::Unix;

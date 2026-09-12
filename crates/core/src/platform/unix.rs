use super::Platform;
use crate::error::{Error, Result};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub struct Unix;

impl Unix {
    fn home() -> PathBuf {
        std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
    }

    /// Sucht `umu-run` im PATH. Nötig, um die Windows-Executable im
    /// vorhandenen Proton-Prefix ohne Steam zu starten.
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
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| Error::io(&url, e))?;
        Ok(())
    }

    fn launch_direct(exe: &Path, env: &[(&str, &str)]) -> Result<()> {
        let umu = Self::umu_launcher().ok_or_else(|| {
            Error::io(
                exe,
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "umu-run nicht im PATH gefunden – für den Start ohne Steam wird \
                     umu-launcher benötigt (https://github.com/Open-Wine-Components/umu-launcher)",
                ),
            )
        })?;

        let work_dir = exe.parent().unwrap_or(Path::new("."));
        let mut cmd = std::process::Command::new(&umu);
        cmd.arg(exe).current_dir(work_dir);
        for (k, v) in env {
            cmd.env(k, v);
        }
        // Der Spawn-Fehler betrifft immer `umu`, nicht `exe`: das
        // Betriebssystem prüft `exe` zu diesem Zeitpunkt noch gar nicht, es
        // versucht nur, den Launcher selbst zu starten.
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
}

/// Prüft, ob unter `path` eine reguläre Datei mit gesetztem Ausführungsbit
/// liegt. Ohne diese Prüfung würde eine gleichnamige, aber nicht
/// ausführbare Datei im PATH als Treffer zählen und `launch_direct` schlägt
/// dann mit einem rohen Spawn-Fehler fehl statt mit der hilfreichen
/// "umu-launcher fehlt"-Meldung.
fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Minimaler PATH-Lookup – vermeidet eine Abhängigkeit für zwanzig Zeilen.
fn which_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    which_in(name, &path)
}

/// PATH-Lookup über einen expliziten PATH-Wert statt über die echte
/// Prozessumgebung, damit sich die Logik ohne Eingriff in `$PATH` testen
/// lässt.
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
            "Flatpak-Steam muss berücksichtigt werden"
        );
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
        // Standard-Berechtigungen von `fs::write` sind nicht ausführbar (0o644) –
        // genau der Fall, den `is_executable` abfangen muss.
        std::fs::write(&candidate, "#!/bin/sh\n").unwrap();

        let found = which_in("umu-run", dir.path().as_os_str());

        assert!(
            found.is_none(),
            "eine nicht ausführbare Datei darf nicht als Treffer zählen"
        );
    }
}

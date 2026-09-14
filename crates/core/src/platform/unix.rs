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
        // Der Spawn-Fehler betrifft immer `xdg-open`, nicht die
        // `steam://`-URL: das Betriebssystem versucht an dieser Stelle nur,
        // den Opener selbst zu starten, die URL wird ihm lediglich als
        // Argument übergeben. `Error::io` erwartet einen Pfad, der die
        // eigentlich betroffene Ressource benennt – das ist hier `xdg-open`,
        // nicht die URL (die als "Pfad" gerendert eine unsinnige Meldung wie
        // "E/A-Fehler bei steam://…" ergäbe).
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

    fn find_tool(name: &str) -> Option<PathBuf> {
        which_in_path(name)
    }

    fn direct_launch_available() -> bool {
        Self::umu_launcher().is_some()
    }

    /// Erkannt wird ausschließlich ein Prozess, dessen `/proc/<pid>/comm`
    /// exakt `steam` lautet. Hilfsprozesse wie `steamwebhelper` zählen
    /// bewusst nicht: sie sind Browser-Unterprozesse ohne eigene
    /// Cloud-Synchronisation, und ein Treffer allein auf "enthält steam"
    /// würde bei jedem `steamwebhelper` oder `steamerrorreporter`
    /// anschlagen und die Warnung wertlos machen. Fehlt `/proc` (z. B. auf
    /// einem System ohne procfs), wird `false` zurückgegeben statt eines
    /// Fehlers – die Prüfung ist eine Vorsichtsmaßnahme, kein hartes
    /// Erfordernis.
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
pub(crate) fn which_in_path(name: &str) -> Option<PathBuf> {
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

    /// `Platform::find_tool` ist eine reine Weiterleitung an
    /// `which_in_path`/`which_in` – deren PATH-Lookup-Logik selbst ist unten
    /// (`which_in_finds_executable_candidate` etc.) hermetisch gegen einen
    /// expliziten PATH-Wert getestet. Ein Test von `find_tool` gegen den
    /// echten `$PATH` würde diesen Prozessweiten Zustand global verändern
    /// müssen und wäre damit parallel zu anderen Tests unsicher – deshalb
    /// hier bewusst kein eigener Test, nur die Weiterleitung selbst (eine
    /// Zeile, siehe `impl Platform for Unix`).
    ///
    /// Reiner Rauchtest: unabhängig davon, ob `/proc` existiert oder ein
    /// Steam-Prozess läuft, darf der Aufruf nicht abstürzen. Das
    /// tatsächliche Erkennungsverhalten hängt vom laufenden System ab und
    /// lässt sich ohne Prozess-Mocking nicht deterministisch prüfen.
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

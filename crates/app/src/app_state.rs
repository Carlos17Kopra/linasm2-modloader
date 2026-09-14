//! Gemeinsamer Zustand für alle CLI-Kommandos: Pfade, Einstellungen,
//! Bibliothek und Pak-Konfiguration in einer Struktur, einmal beim Start
//! geladen und mit dem Verzeichnisinhalt abgeglichen.

use anyhow::{Context, Result};
use sm2_core::library::Library;
use sm2_core::pak_config::PakConfig;
use sm2_core::paths::{app_dirs, AppDirs, GamePaths};
use sm2_core::settings::Settings;
use sm2_core::Error;
use std::path::{Path, PathBuf};

pub struct AppState {
    pub paths: GamePaths,
    pub settings: Settings,
    pub dirs: AppDirs,
    pub library: Library,
    pub config: PakConfig,
}

impl AppState {
    /// Erkennt das Spiel, lädt alle Zustände und gleicht die Konfiguration
    /// mit dem Verzeichnisinhalt ab. Meldet Abweichungen auf stderr.
    ///
    /// Der Abgleich wird bewusst NICHT sofort auf die Platte geschrieben:
    /// `reconcile` ist deterministisch – bei jedem Aufruf aus demselben
    /// Verzeichnisinhalt reproduzierbar –, und ein sofortiges Schreiben
    /// würde rein lesende Kommandos wie `list` oder `paths` an einem
    /// schreibgeschützten Mods-Verzeichnis unnötig scheitern lassen. Die
    /// abgeglichene Konfiguration landet erst auf der Platte, wenn ohnehin
    /// ein verändernder Befehl `persist()` aufruft.
    pub fn open() -> Result<Self> {
        let dirs = app_dirs().context("Basisverzeichnisse nicht ermittelbar")?;
        std::fs::create_dir_all(&dirs.config)
            .with_context(|| format!("{} konnte nicht angelegt werden", dirs.config.display()))?;
        std::fs::create_dir_all(&dirs.data)
            .with_context(|| format!("{} konnte nicht angelegt werden", dirs.data.display()))?;

        let settings = Settings::load(&dirs.config.join("settings.toml"))?;

        let paths = match &settings.game_dir {
            Some(dir) => {
                // Bei manueller Angabe die Bibliothek aus dem Pfad ableiten.
                let library = dir
                    .ancestors()
                    .find(|a| a.join("steamapps/common").is_dir())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| dir.clone());
                GamePaths::from_game_dir(dir, &library)?
            }
            None => GamePaths::discover().context(
                "Space Marine 2 nicht gefunden – Pfad in settings.toml unter game_dir eintragen",
            )?,
        };

        let library = Library::load(&dirs.data.join("library.json"))?;
        let mut config = PakConfig::load(&paths.pak_config_path())?;

        let reconciliation = config.reconcile(&paths.list_paks()?);
        for pak in &reconciliation.added {
            eprintln!(
                "Hinweis: {pak} war nicht in pak_config.yaml eingetragen und wurde aktiv übernommen."
            );
        }
        for pak in &reconciliation.removed {
            eprintln!(
                "Hinweis: {pak} steht in pak_config.yaml, die Datei fehlt aber – Eintrag entfernt."
            );
        }

        Ok(Self { paths, settings, dirs, library, config })
    }

    /// Schreibt Bibliothek und Konfiguration auf die Platte.
    ///
    /// Reihenfolge ist bewusst gewählt: `library.json` (unsere eigenen
    /// Aufzeichnungen) zuerst, `pak_config.yaml` (die für die Spiel-Engine
    /// sichtbare Datei) zuletzt. Schlägt der zweite Schritt fehl, hat die
    /// Engine den neuen Zustand noch nicht gesehen – nur unsere eigenen,
    /// noch nicht wirksam gewordenen Aufzeichnungen sind dann voraus. In der
    /// umgekehrten Reihenfolge würde derselbe Fehlerfall eine für die Engine
    /// bereits wirksame Änderung mit veralteten eigenen Aufzeichnungen
    /// hinterlassen.
    ///
    /// Prüft vorher, ob das Mods-Verzeichnis überhaupt beschreibbar ist –
    /// hier und nicht beim bloßen `open()`, damit rein lesende Kommandos an
    /// einem schreibgeschützten Verzeichnis nicht scheitern.
    pub fn persist(&self) -> Result<()> {
        check_write_permission(&self.paths.mods_dir())?;
        self.library.save(&self.dirs.data.join("library.json"))?;
        self.config.save(&self.paths.pak_config_path())?;
        Ok(())
    }

    pub fn profiles_dir(&self) -> PathBuf {
        self.dirs.data.join("profiles")
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.dirs.data.join("backups/saves")
    }
}

/// Stellt fest, ob wir in `dir` schreiben können – bevor ein verändernder
/// Befehl Änderungen vornimmt, die dann erst beim Speichern scheitern.
fn check_write_permission(dir: &Path) -> Result<()> {
    let probe = dir.join(".sm2-modloader-writetest");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(_) => Err(Error::NotWritable(dir.to_path_buf()).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Baut einen `AppState` direkt aus den öffentlichen Feldern, ohne
    /// `open()` (das eine echte Spielinstallation über `discover()` oder
    /// `settings.toml` verlangt). Für Tests reicht ein minimales, gültiges
    /// Spielverzeichnis.
    fn state_fixture(base: &Path) -> AppState {
        let game = base.join("game");
        std::fs::create_dir_all(game.join("client_pc/root/mods")).unwrap();
        let paths = GamePaths::from_game_dir(&game, base).unwrap();
        AppState {
            paths,
            settings: Settings::default(),
            dirs: AppDirs {
                config: base.join("config"),
                data: base.join("data"),
                state: base.join("state"),
            },
            library: Library::default(),
            config: PakConfig::default(),
        }
    }

    #[test]
    fn profiles_dir_and_backups_dir_are_under_the_data_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let state = state_fixture(tmp.path());

        assert_eq!(state.profiles_dir(), state.dirs.data.join("profiles"));
        assert_eq!(state.backups_dir(), state.dirs.data.join("backups/saves"));
    }

    #[test]
    fn check_write_permission_succeeds_for_a_writable_dir_and_leaves_no_probe_file() {
        let tmp = tempfile::tempdir().unwrap();
        check_write_permission(tmp.path()).unwrap();
        assert!(!tmp.path().join(".sm2-modloader-writetest").exists());
    }

    #[test]
    fn check_write_permission_fails_for_a_missing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("gibt_es_nicht");
        assert!(check_write_permission(&missing).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn check_write_permission_fails_for_a_read_only_dir() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("readonly");
        std::fs::create_dir_all(&dir).unwrap();
        let mut perms = std::fs::metadata(&dir).unwrap().permissions();
        perms.set_mode(0o555);
        std::fs::set_permissions(&dir, perms.clone()).unwrap();

        let result = check_write_permission(&dir);

        // Aufräumen, damit tempfile das Verzeichnis wieder löschen kann.
        perms.set_mode(0o755);
        std::fs::set_permissions(&dir, perms).unwrap();

        if result.is_ok() {
            // Läuft der Test als root, übergeht der Kernel den
            // Schreibschutz-Modus vollständig – kein Fehlschlag dieses Tests.
            eprintln!("übersprungen: Prozess kann den Schreibschutz offenbar übergehen (root?)");
            return;
        }
        assert!(result.is_err());
    }
}

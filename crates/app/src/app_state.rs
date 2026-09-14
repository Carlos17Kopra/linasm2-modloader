//! Gemeinsamer Zustand für alle CLI-Kommandos: Pfade, Einstellungen,
//! Bibliothek und Pak-Konfiguration in einer Struktur, einmal beim Start
//! geladen und mit dem Verzeichnisinhalt abgeglichen.

use anyhow::{Context, Result};
use sm2_core::library::Library;
use sm2_core::pak_config::{KnownState, PakConfig};
use sm2_core::paths::{app_dirs, AppDirs, GamePaths};
use sm2_core::settings::Settings;
use sm2_core::Error;
use std::collections::HashMap;
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

        let mut library = Library::load(&dirs.data.join("library.json"))?;
        let mut config = PakConfig::load(&paths.pak_config_path())?;
        reconcile_and_report(&mut library, &mut config, &paths)?;

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
    ///
    /// Aktualisiert außerdem für jedes Pak, das gerade Teil der
    /// Konfiguration ist, `ModInfo::last_known_disabled`/
    /// `last_known_position` in der Bibliothek (siehe deren Doc-Kommentare):
    /// das ist der einzige Ort, an dem diese Werte geschrieben werden, und
    /// die einzige Quelle, aus der `PakConfig::reconcile` ein später
    /// wieder auftauchendes Pak an seinen alten Platz zurückstellen kann.
    /// Ein Pak, das aktuell fehlt, wird hier bewusst nicht angefasst – sein
    /// zuletzt bekannter Zustand bleibt genau deshalb erhalten.
    pub fn persist(&mut self) -> Result<()> {
        check_write_permission(&self.paths.mods_dir())?;
        for (position, entry) in self.config.entries.iter().enumerate() {
            if let Some(mod_info) = self.library.mods.get_mut(&entry.pak) {
                mod_info.last_known_disabled = entry.disabled;
                mod_info.last_known_position = position;
            }
        }
        self.library.save(&self.dirs.data.join("library.json"))?;
        self.config.save(&self.paths.pak_config_path())?;
        Ok(())
    }

    /// Ermittelt das Savegame-Verzeichnis, unter Berücksichtigung einer
    /// Vorgabe in `settings.steam_user` (siehe `GamePaths::save_dir`s
    /// Doc-Kommentar für die genaue Auflösungsreihenfolge). Einziger
    /// Aufrufpunkt in der CLI, damit `steam_user` nicht an jeder einzelnen
    /// Stelle, die das Save-Verzeichnis braucht, erneut verdrahtet wird.
    pub fn save_dir(&self) -> Result<PathBuf> {
        self.paths.save_dir(self.settings.steam_user.as_deref()).map_err(Into::into)
    }

    pub fn profiles_dir(&self) -> PathBuf {
        self.dirs.data.join("profiles")
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.dirs.data.join("backups/saves")
    }
}

/// Gleicht `config` mit dem tatsächlichen Verzeichnisinhalt ab (Spec §6.3)
/// und meldet jede Abweichung auf stderr. Eigene Funktion statt inline in
/// `open()`, damit die Logik in Tests unabhängig von einer echten
/// Spielinstallation (die `open()` über `discover()`/`settings.toml`
/// verlangt) durchlaufen werden kann.
///
/// Baut den bekannten Zustand aus `library` für `PakConfig::reconcile` auf
/// (siehe `KnownState`s Doc-Kommentar): `pak_config.rs` kennt die Bibliothek
/// bewusst nicht selbst, um die Modulschichtung nicht umzukehren – die
/// App-Schicht baut diese Map explizit und übergibt sie als Parameter.
/// Prüft anschließend den dritten Abgleichsfall aus Spec §6.3 ("außerhalb
/// verändert") über `Library::detect_altered`.
fn reconcile_and_report(library: &mut Library, config: &mut PakConfig, paths: &GamePaths) -> Result<()> {
    let present = paths.list_paks()?;

    let known_state: HashMap<String, KnownState> = library
        .mods
        .values()
        .map(|m| {
            (m.pak.clone(), KnownState { disabled: m.last_known_disabled, position: m.last_known_position })
        })
        .collect();

    let reconciliation = config.reconcile(&present, &known_state);
    let restored: std::collections::HashSet<&str> =
        reconciliation.restored.iter().map(String::as_str).collect();
    for pak in &reconciliation.added {
        if restored.contains(pak.as_str()) {
            eprintln!(
                "Hinweis: {pak} war zwischenzeitlich nicht vorhanden und wurde mit \
                 vorherigem Aktivierungszustand und vorheriger Position wiederhergestellt."
            );
        } else {
            eprintln!(
                "Hinweis: {pak} war nicht in pak_config.yaml eingetragen und wurde aktiv übernommen."
            );
        }
    }
    for pak in &reconciliation.removed {
        eprintln!("Hinweis: {pak} steht in pak_config.yaml, die Datei fehlt aber – Eintrag entfernt.");
    }

    // Nur Größe/Änderungszeit werden hier standardmäßig geprüft (siehe
    // `Library::detect_altered`s Doc-Kommentar) – ein vollständiger Hash
    // läuft nur, wenn dieser billige Vorfilter eine Abweichung anzeigt.
    let altered = library.detect_altered(&paths.mods_dir(), &present)?;
    for pak in &altered {
        eprintln!(
            "Hinweis: {pak} weicht vom zuletzt bekannten Stand ab – vermutlich außerhalb \
             des Loaders verändert oder ersetzt."
        );
    }

    Ok(())
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

/// Baut einen `AppState` direkt aus den öffentlichen Feldern, ohne `open()`
/// (das eine echte Spielinstallation über `discover()` oder
/// `settings.toml` verlangt). Für Tests reicht ein minimales, gültiges
/// Spielverzeichnis. `pub(crate)`, damit auch die Tests in `cli.rs` diese
/// Fixture nutzen können, statt sie zu duplizieren.
#[cfg(test)]
pub(crate) fn test_fixture(base: &Path) -> AppState {
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

#[cfg(test)]
mod tests {
    use super::*;
    use sm2_core::library::ModInfo;
    use sm2_core::pak_config::PakEntry;

    fn minimal_mod_info(pak: &str) -> ModInfo {
        ModInfo {
            pak: pak.to_string(),
            name: pak.to_string(),
            author: None,
            version: None,
            nexus_id: None,
            notes: None,
            hash: "irrelevant".to_string(),
            size: 1,
            imported_at: "2026-09-12T18:00:00Z".to_string(),
            source: None,
            last_known_disabled: false,
            last_known_position: 0,
            mtime: None,
        }
    }

    /// Der Kernfall aus dem Review (1a): ein Pak verschwindet (z. B. durch
    /// ein Steam-Update, Spec §9 R3), `persist()` schreibt die dadurch
    /// verkürzte Konfiguration, das Pak taucht wieder auf – und muss dann
    /// mit seinem vorherigen Aktivierungszustand UND seiner vorherigen
    /// Position zurückkehren, nicht enabled-alphabetisch ans Ende. Prüft die
    /// tatsächliche Verdrahtung (persist() -> library.json -> reconcile),
    /// nicht nur `PakConfig::reconcile` isoliert (das deckt schon
    /// `pak_config.rs`s eigener Test ab).
    #[test]
    fn a_pak_that_disappears_and_reappears_keeps_its_previous_state_across_persist() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        let mods_dir = state.paths.mods_dir();

        for name in ["a.pak", "b.pak", "c.pak"] {
            std::fs::write(mods_dir.join(name), b"INHALT").unwrap();
            state.library.mods.insert(name.to_string(), minimal_mod_info(name));
        }
        state.config.entries = vec![
            PakEntry { pak: "a.pak".into(), disabled: false },
            PakEntry { pak: "b.pak".into(), disabled: true },
            PakEntry { pak: "c.pak".into(), disabled: false },
        ];

        // Schreibt last_known_disabled/last_known_position für alle drei.
        state.persist().unwrap();

        // b.pak verschwindet (z. B. Steam-Update) und wird abgeglichen.
        std::fs::remove_file(mods_dir.join("b.pak")).unwrap();
        reconcile_and_report(&mut state.library, &mut state.config, &state.paths).unwrap();
        assert_eq!(
            state.config.entries.iter().map(|e| e.pak.as_str()).collect::<Vec<_>>(),
            vec!["a.pak", "c.pak"],
            "b.pak muss durch den Abgleich entfernt werden"
        );
        // persist() schreibt die verkürzte Konfiguration; b.pak bleibt in der
        // Bibliothek unangetastet (es ist nicht Teil von config.entries).
        state.persist().unwrap();

        // b.pak taucht wieder auf.
        std::fs::write(mods_dir.join("b.pak"), b"INHALT").unwrap();
        reconcile_and_report(&mut state.library, &mut state.config, &state.paths).unwrap();

        let names: Vec<&str> = state.config.entries.iter().map(|e| e.pak.as_str()).collect();
        assert_eq!(names, vec!["a.pak", "b.pak", "c.pak"], "b.pak muss an seine alte Position zurückkehren");
        assert!(state.config.entries[1].disabled, "b.pak war deaktiviert und muss es wieder sein");
    }

    /// Baut unter `base` (derselben Wurzel, die `test_fixture` als
    /// `library_dir` verwendet) zwei Proton-Save-Nutzerverzeichnisse auf, wie
    /// sie bei mehreren Steam-Profilen im selben Prefix entstehen.
    fn write_two_save_users(base: &Path) -> (String, String) {
        let user_root = base
            .join("steamapps/compatdata/2183900/pfx/drive_c/users/steamuser")
            .join("AppData/Local/Saber/Space Marine 2/storage/steam/user");
        let a = "76561198000000001".to_string();
        let b = "76561198000000002".to_string();
        std::fs::create_dir_all(user_root.join(&a).join("Main")).unwrap();
        std::fs::create_dir_all(user_root.join(&b).join("Main")).unwrap();
        (a, b)
    }

    /// 2b: `settings.steam_user` ist keine Schmuck-Einstellung mehr, sondern
    /// wird tatsächlich gelesen und löst die sonst tödliche Mehrdeutigkeit
    /// mehrerer Save-Nutzerprofile auf.
    #[test]
    fn save_dir_uses_the_configured_steam_user_to_resolve_ambiguity() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        let (a, _b) = write_two_save_users(tmp.path());
        state.settings.steam_user = Some(a.clone());

        let saves = state.save_dir().unwrap();

        assert!(saves.ends_with(format!("{a}/Main")));
    }

    /// Ein `steam_user`, der zu keinem gefundenen Profil passt (z. B. ein
    /// Tippfehler in `settings.toml`), muss einen klaren, die vorhandenen
    /// Profile nennenden Fehler ergeben.
    #[test]
    fn save_dir_reports_a_clear_error_when_the_configured_steam_user_matches_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        write_two_save_users(tmp.path());
        state.settings.steam_user = Some("00000000000000000".to_string());

        let err = state.save_dir().unwrap_err();

        assert!(
            err.to_string().contains("00000000000000000"),
            "Fehler muss die (nicht gefundene) Vorgabe nennen: {err}"
        );
    }

    #[test]
    fn profiles_dir_and_backups_dir_are_under_the_data_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_fixture(tmp.path());

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

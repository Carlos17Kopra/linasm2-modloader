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

/// Gewicht einer Meldung aus dem Abgleich – bestimmt in der Oberfläche
/// Farbe und Symbol, auf der Kommandozeile das Präfix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    /// Etwas ist geschehen, das der Nutzer wissen sollte, aber nichts fehlt.
    Info,
    /// Etwas weicht vom erwarteten Zustand ab und braucht womöglich eine
    /// Entscheidung.
    Warning,
    /// Etwas Erwartetes fehlt.
    Error,
}

/// Eine Meldung aus dem Abgleich zwischen Konfiguration und Verzeichnis.
///
/// Struktur statt `eprintln!`: die grafische Oberfläche zeigt dieselben
/// Meldungen als Hinweisleiste über der Mod-Liste an, und ein bereits auf
/// stderr geschriebener Text ließe sich dort nicht mehr einfärben, gruppieren
/// oder wegklicken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub kind: NoticeKind,
    pub text: String,
}

impl Notice {
    fn info(text: impl Into<String>) -> Self {
        Self { kind: NoticeKind::Info, text: text.into() }
    }

    fn warning(text: impl Into<String>) -> Self {
        Self { kind: NoticeKind::Warning, text: text.into() }
    }

    fn error(text: impl Into<String>) -> Self {
        Self { kind: NoticeKind::Error, text: text.into() }
    }
}

pub struct AppState {
    pub paths: GamePaths,
    pub settings: Settings,
    pub dirs: AppDirs,
    pub library: Library,
    pub config: PakConfig,
    /// Seit dem letzten Abholen angefallene Meldungen – siehe `Notice`.
    pub notices: Vec<Notice>,
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
        let (dirs, settings) = load_dirs_and_settings()?;
        Self::open_with(dirs, settings)
    }

    /// Wie `open()`, aber mit bereits geladenen Basisverzeichnissen und
    /// Einstellungen.
    ///
    /// Eigener Einstiegspunkt für die grafische Oberfläche: schlägt die
    /// Spielerkennung fehl, zeigt sie den Erstlauf-Bildschirm und muss das
    /// vom Nutzer gewählte Verzeichnis in dieselben Einstellungen schreiben
    /// können – die also den fehlgeschlagenen Versuch überleben müssen.
    pub fn open_with(dirs: AppDirs, settings: Settings) -> Result<Self> {
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

        let library_path = dirs.data.join("library.json");
        let mut library = Library::load(&library_path)?;
        let mut config = PakConfig::load(&paths.pak_config_path())?;
        let (cache_refreshed, mut notices) = reconcile_and_collect(&mut library, &mut config, &paths)?;
        if cache_refreshed {
            save_library_cache_best_effort(&library, &library_path, &mut notices);
        }

        Ok(Self { paths, settings, dirs, library, config, notices })
    }

    /// Holt alle seit dem letzten Aufruf angefallenen Meldungen ab und leert
    /// den Puffer – damit dieselbe Meldung nicht zweimal erscheint.
    pub fn take_notices(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.notices)
    }

    /// Ist das Mods-Verzeichnis beschreibbar? Die Oberfläche fragt das beim
    /// Laden einmal, um Aktivieren, Sortieren und Import zu sperren, statt
    /// den Nutzer erst beim Speichern scheitern zu lassen.
    pub fn mods_dir_is_writable(&self) -> bool {
        check_write_permission(&self.paths.mods_dir()).is_ok()
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
    ///
    /// Ein Pak in `config.entries` ohne `ModInfo` (von Hand in `mods/`
    /// kopiert statt über `import_pak` importiert – siehe Review-Punkt 1)
    /// bekommt hier einen minimalen `ModInfo`-Eintrag verpasst, statt für
    /// immer historienlos zu bleiben: sonst käme ein solches Pak nach einem
    /// Verschwinden (z. B. Steam-Update) nie an seine vorherige Position
    /// zurück, obwohl `reconcile`s Anhänge-Regel eigentlich genau für diese
    /// Population gedacht ist. Der Hash-Aufwand dafür trifft nur `persist()`
    /// (einen bewusst schreibenden Aufruf), nicht `AppState::open()` – ein
    /// rein lesender Befehl wie `list` hasht ein neu entdecktes, von Hand
    /// kopiertes Pak also nicht (siehe Review-Punkt 2, derselbe Grundsatz).
    ///
    /// Schlägt das Hashen fehl, wird der Eintrag übersprungen (nicht
    /// `persist()` insgesamt) und – konsistent mit `detect_altered`s eigenem
    /// Umgang mit Lesefehlern – als deutsche Warnung gemeldet: verschwindet
    /// die Datei einfach wieder, meldet sie das nächste `reconcile` ohnehin
    /// unter `removed`, aber ein dauerhafter Leserechte-Fehler bei
    /// unverändert vorhandener Datei würde sonst still und für immer
    /// unbemerkt bleiben, statt dass der Nutzer erfährt, warum dieses Pak nie
    /// eine Historie bekommt.
    pub fn persist(&mut self) -> Result<()> {
        check_write_permission(&self.paths.mods_dir())?;
        let mods_dir = self.paths.mods_dir();
        let mut warnings = Vec::new();
        for (position, entry) in self.config.entries.iter().enumerate() {
            match self.library.mods.get_mut(&entry.pak) {
                Some(mod_info) => {
                    mod_info.last_known_disabled = entry.disabled;
                    mod_info.last_known_position = Some(position);
                }
                None => match register_unknown_pak(&mods_dir, &entry.pak, entry.disabled, position) {
                    Ok(info) => {
                        self.library.mods.insert(entry.pak.clone(), info);
                    }
                    Err(e) => warnings.push(Notice::warning(format!(
                        "{} konnte nicht für die Positions-/Aktivierungshistorie \
                         registriert werden – {e}",
                        entry.pak
                    ))),
                },
            }
        }
        self.notices.append(&mut warnings);
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
/// App-Schicht baut diese Map explizit und übergibt sie als Parameter. Ein
/// `ModInfo` ohne `last_known_position` (nie Teil der Konfiguration gewesen,
/// oder ein `library.json` von vor diesem Feld) wird dabei ausgeschlossen
/// statt mit einer geratenen Position aufgenommen – siehe Review-Punkt 4 und
/// `ModInfo::last_known_position`s Doc-Kommentar.
///
/// Prüft anschließend den dritten Abgleichsfall aus Spec §6.3 ("außerhalb
/// verändert") über `Library::detect_altered` und gibt zurück, ob dessen
/// billiger Vorfilter-Cache aufgefrischt wurde – der Aufrufer (`open()`)
/// schreibt `library.json` dann sofort neu (siehe Review-Punkt 2), damit ein
/// veraltetes oder fehlendes `mtime` nicht bei jedem weiteren – auch rein
/// lesenden – Aufruf erneut zu einem vollständigen Hash führt.
fn reconcile_and_collect(
    library: &mut Library,
    config: &mut PakConfig,
    paths: &GamePaths,
) -> Result<(bool, Vec<Notice>)> {
    let mut notices = Vec::new();
    let present = paths.list_paks()?;

    let known_state: HashMap<String, KnownState> = library
        .mods
        .values()
        .filter_map(|m| {
            m.last_known_position
                .map(|position| (m.pak.clone(), KnownState { disabled: m.last_known_disabled, position }))
        })
        .collect();

    let reconciliation = config.reconcile(&present, &known_state);
    let restored: std::collections::HashSet<&str> =
        reconciliation.restored.iter().map(String::as_str).collect();
    for pak in &reconciliation.added {
        if restored.contains(pak.as_str()) {
            notices.push(Notice::info(format!(
                "{pak} war zwischenzeitlich nicht vorhanden und wurde mit vorherigem \
                 Aktivierungszustand und vorheriger Position wiederhergestellt."
            )));
        } else {
            notices.push(Notice::warning(format!(
                "{pak} stand nicht in pak_config.yaml und wurde aktiv übernommen – eine Datei \
                 im Mods-Verzeichnis lädt ohnehin, ungesteuert und zuerst."
            )));
        }
    }
    for pak in &reconciliation.removed {
        notices.push(Notice::error(format!(
            "{pak} steht in pak_config.yaml, die Datei fehlt aber – Eintrag entfernt."
        )));
    }

    // Nur Größe/Änderungszeit werden hier standardmäßig geprüft (siehe
    // `Library::detect_altered`s Doc-Kommentar) – ein vollständiger Hash
    // läuft nur, wenn dieser billige Vorfilter eine Abweichung anzeigt.
    let report = library.detect_altered(&paths.mods_dir(), &present)?;
    for pak in &report.altered {
        notices.push(Notice::warning(format!(
            "{pak} wurde außerhalb des Loaders verändert (Hash weicht ab)."
        )));
    }
    for warning in &report.warnings {
        notices.push(Notice::warning(warning.clone()));
    }

    Ok((report.cache_refreshed, notices))
}

/// Schreibt `library.json` nach einer reinen Cache-Auffrischung (siehe
/// `Library::detect_altered`s `cache_refreshed`) – und zwar nur bestmöglich:
/// schlägt das Schreiben fehl (z. B. weil das Anwendungsdatenverzeichnis
/// zwar existiert, aber nicht beschreibbar ist), wird nur gewarnt, `open()`
/// selbst schlägt NICHT fehl. Bis zu diesem Punkt sind Laden der Bibliothek
/// und der Konfiguration bereits gelungen; ein hartes `?` hier würde also
/// genau die Fehlerklasse wieder einführen ("jeder Befehl, auch
/// `paths`/`list`, scheitert"), die durch das Einführen von
/// `cache_refreshed` gerade erst beseitigt wurde – dieses Schreiben ist
/// reine Cache-Pflege, kein Ergebnis, das der Nutzer mit seinem Aufruf
/// beabsichtigt hat. Eigene Funktion, damit dieses Verhalten (warnen statt
/// scheitern) unabhängig von einer echten Spielinstallation testbar ist.
fn save_library_cache_best_effort(library: &Library, library_path: &Path, notices: &mut Vec<Notice>) {
    if let Err(e) = library.save(library_path) {
        notices.push(Notice::warning(format!(
            "Cache-Auffrischung in library.json konnte nicht gespeichert werden – {e}"
        )));
    }
}

/// Baut für ein Pak, das in `config.entries` steht, aber (weil von Hand in
/// `mods/` abgelegt statt über `import_pak` importiert) noch keinen
/// `ModInfo`-Eintrag hat, einen minimalen Eintrag – siehe `persist()`s
/// Doc-Kommentar (Review-Punkt 1).
fn register_unknown_pak(
    mods_dir: &Path,
    pak: &str,
    disabled: bool,
    position: usize,
) -> sm2_core::Result<sm2_core::library::ModInfo> {
    use sm2_core::import::{now_rfc3339, strip_pak_suffix};
    use sm2_core::library::{hash_file, ModInfo};

    let path = mods_dir.join(pak);
    let metadata = std::fs::metadata(&path).map_err(|e| sm2_core::Error::io(&path, e))?;
    let hash = hash_file(&path)?;
    let mtime = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());

    // Dieselbe Ableitung wie `import_pak`s `display_name` (Groß-/
    // Kleinschreibung unabhängige, nicht wiederholte `.pak`-Endung über
    // `strip_pak_suffix`, dann Trennzeichen durch Leerzeichen) – nicht ein
    // eigenes `trim_end_matches(".pak")`, das bei `MOD.PAK` gar nicht griffe
    // und bei `a.pak.pak` mehrfach abschneiden würde, sonst könnten zwei
    // baugleich abgelegte Paks unterschiedliche Anzeigenamen bekommen, je
    // nachdem, ob sie importiert oder von Hand kopiert wurden.
    let name = strip_pak_suffix(pak).replace(['_', '-'], " ");

    Ok(ModInfo {
        pak: pak.to_string(),
        name,
        author: None,
        version: None,
        nexus_id: None,
        notes: None,
        hash,
        size: metadata.len(),
        imported_at: now_rfc3339(),
        source: None,
        last_known_disabled: disabled,
        last_known_position: Some(position),
        mtime,
        known_altered: false,
    })
}

/// Stellt fest, ob wir in `dir` schreiben können – bevor ein verändernder
/// Befehl Änderungen vornimmt, die dann erst beim Speichern scheitern.
/// Lädt Basisverzeichnisse und Einstellungen – der Teil von `open()`, der
/// auch ohne erkanntes Spielverzeichnis gelingt. Getrennt, damit die
/// grafische Oberfläche nach einer gescheiterten Spielerkennung immer noch
/// weiß, wohin sie ein vom Nutzer gewähltes Verzeichnis schreiben soll.
pub fn load_dirs_and_settings() -> Result<(AppDirs, Settings)> {
    let dirs = app_dirs().context("Basisverzeichnisse nicht ermittelbar")?;
    std::fs::create_dir_all(&dirs.config)
        .with_context(|| format!("{} konnte nicht angelegt werden", dirs.config.display()))?;
    std::fs::create_dir_all(&dirs.data)
        .with_context(|| format!("{} konnte nicht angelegt werden", dirs.data.display()))?;
    let settings = Settings::load(&dirs.config.join("settings.toml"))?;
    Ok((dirs, settings))
}

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
        notices: Vec::new(),
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
            last_known_position: Some(0),
            mtime: None,
            known_altered: false,
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
        reconcile_and_collect(&mut state.library, &mut state.config, &state.paths).unwrap();
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
        reconcile_and_collect(&mut state.library, &mut state.config, &state.paths).unwrap();

        let names: Vec<&str> = state.config.entries.iter().map(|e| e.pak.as_str()).collect();
        assert_eq!(names, vec!["a.pak", "b.pak", "c.pak"], "b.pak muss an seine alte Position zurückkehren");
        assert!(state.config.entries[1].disabled, "b.pak war deaktiviert und muss es wieder sein");
    }

    /// Review-Punkt 1: dieselbe Garantie wie oben, aber für ein Pak, das nie
    /// über `import_pak` importiert wurde – von Hand in `mods/` kopiert,
    /// ohne jeden `ModInfo`-Eintrag. Genau diese Population war zuvor von
    /// jeder Historie ausgeschlossen (`KnownState` kam ausschließlich aus
    /// `library.mods`, in das nur `import_pak` je etwas einträgt) und kam
    /// nach einem Verschwinden/Wiederauftauchen immer enabled-alphabetisch
    /// zurück statt an ihre vorherige Position.
    #[test]
    fn a_hand_copied_pak_without_any_prior_modinfo_also_gets_its_history_back() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        let mods_dir = state.paths.mods_dir();

        // Von Hand hineinkopiert: Datei liegt im Mods-Verzeichnis, aber es
        // gibt (anders als beim Import) keinen library.mods-Eintrag dafür.
        std::fs::write(mods_dir.join("hand.pak"), b"VON HAND KOPIERT").unwrap();
        assert!(state.library.mods.is_empty(), "Ausgangslage: der Bibliothek unbekannt");

        reconcile_and_collect(&mut state.library, &mut state.config, &state.paths).unwrap();
        assert_eq!(
            state.config.entries.iter().map(|e| e.pak.as_str()).collect::<Vec<_>>(),
            vec!["hand.pak"],
            "unbekanntes Pak wird zunächst wie gewohnt aktiv angehängt"
        );

        // Nutzer deaktiviert es explizit (z. B. `sm2 disable hand.pak`) und
        // ein verändernder Befehl schreibt die Konfiguration.
        state.config.entries[0].disabled = true;
        state.persist().unwrap();
        assert!(
            state.library.mods.contains_key("hand.pak"),
            "persist() muss dem bislang unbekannten Pak jetzt einen ModInfo-Eintrag geben"
        );

        // Steam-Update räumt den Mods-Ordner leer.
        std::fs::remove_file(mods_dir.join("hand.pak")).unwrap();
        reconcile_and_collect(&mut state.library, &mut state.config, &state.paths).unwrap();
        assert!(state.config.entries.is_empty());
        state.persist().unwrap();

        // Nutzer installiert die exakt gleiche Datei erneut von Hand.
        std::fs::write(mods_dir.join("hand.pak"), b"VON HAND KOPIERT").unwrap();
        reconcile_and_collect(&mut state.library, &mut state.config, &state.paths).unwrap();

        assert_eq!(state.config.entries.len(), 1);
        assert_eq!(state.config.entries[0].pak, "hand.pak");
        assert!(
            state.config.entries[0].disabled,
            "die vorherige Deaktivierung muss zurückkehren, nicht enabled-alphabetisch"
        );
    }

    /// Review-Punkt 4 (dritte Runde): der Anzeigename eines von Hand
    /// kopierten Pakets muss exakt derselben Ableitung folgen wie
    /// `import_pak`s `display_name` (`strip_pak_suffix` + Trennzeichen durch
    /// Leerzeichen) – nicht `trim_end_matches(".pak")`, das bei `MOD.PAK`
    /// gar nicht griffe und bei `a.pak.pak` mehrfach abschneiden würde.
    #[test]
    fn register_unknown_pak_derives_the_display_name_like_import_pak_does() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        let mods_dir = state.paths.mods_dir();

        for name in ["mein_mod.pak", "MOD.PAK", "a.pak.pak"] {
            std::fs::write(mods_dir.join(name), b"x").unwrap();
        }
        reconcile_and_collect(&mut state.library, &mut state.config, &state.paths).unwrap();
        state.persist().unwrap();

        assert_eq!(state.library.mods["mein_mod.pak"].name, "mein mod");
        assert_eq!(
            state.library.mods["MOD.PAK"].name, "MOD",
            "die .pak-Endung muss unabhängig von Groß-/Kleinschreibung erkannt werden"
        );
        assert_eq!(
            state.library.mods["a.pak.pak"].name, "a.pak",
            "nur die letzte .pak-Endung darf abgeschnitten werden, nicht wiederholt"
        );
    }

    /// Review-Punkt 3: schlägt `register_unknown_pak` fehl (hier: die Datei
    /// verschwindet zwischen `reconcile` und `persist` wieder, aber ebenso
    /// bei fehlenden Leserechten trotz weiterhin vorhandener Datei), darf das
    /// nicht stillschweigend passieren – der Nutzer muss erfahren, warum
    /// dieses Pak nie eine Historie bekommt.
    #[test]
    fn persist_survives_a_pak_that_cannot_be_registered_without_inventing_history() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        // In der Konfiguration, aber ohne ModInfo UND ohne Datei im
        // Mods-Verzeichnis – reproduziert register_unknown_paks Fehlerpfad
        // (Stat schlägt fehl), ohne auf Dateiberechtigungen angewiesen zu
        // sein.
        state.config.entries = vec![PakEntry { pak: "weg.pak".into(), disabled: false }];

        // Kein Panic, `persist()` selbst gelingt weiterhin (das Fehlen
        // dieses einen Eintrags ist kein hartes Erfordernis).
        state.persist().unwrap();

        assert!(
            !state.library.mods.contains_key("weg.pak"),
            "ohne lesbare Datei kann kein ModInfo entstehen"
        );
    }

    /// Review-Punkt 1: schlägt das Schreiben von `library.json` nach einer
    /// reinen Cache-Auffrischung fehl (z. B. Anwendungsdatenverzeichnis
    /// existiert, ist aber nicht beschreibbar), darf das nicht wie zuvor
    /// (`?`) den gesamten Aufruf scheitern lassen – bis dahin waren sowohl
    /// das Laden der Bibliothek als auch der Konfiguration bereits
    /// erfolgreich.
    #[cfg(unix)]
    #[test]
    fn save_library_cache_best_effort_warns_instead_of_failing_on_a_read_only_data_dir() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let library_path = data_dir.join("library.json");

        let mut perms = std::fs::metadata(&data_dir).unwrap().permissions();
        perms.set_mode(0o555);
        std::fs::set_permissions(&data_dir, perms.clone()).unwrap();

        let probe = data_dir.join(".probe");
        let bypassed = std::fs::write(&probe, b"").is_ok();
        let _ = std::fs::remove_file(&probe);

        if bypassed {
            perms.set_mode(0o755);
            std::fs::set_permissions(&data_dir, perms).unwrap();
            eprintln!("übersprungen: Prozess kann den Schreibschutz offenbar übergehen (root?)");
            return;
        }

        // Darf nicht abstürzen – die Funktion hat keinen Rückgabewert, über
        // den ein Aufrufer den Fehlschlag zum Abbruch machen könnte (siehe
        // deren Doc-Kommentar); das ist hier bewusst Teil des Vertrags. Der
        // Fehlschlag verschwindet aber nicht, sondern landet als Meldung im
        // Puffer.
        let mut notices = Vec::new();
        save_library_cache_best_effort(&Library::default(), &library_path, &mut notices);

        perms.set_mode(0o755);
        std::fs::set_permissions(&data_dir, perms).unwrap();

        assert!(!library_path.exists(), "das Schreiben muss tatsächlich gescheitert sein");
        assert_eq!(notices.len(), 1, "der Fehlschlag muss als genau eine Meldung erscheinen");
        assert_eq!(notices[0].kind, NoticeKind::Warning, "eine Cache-Pflege ist kein Fehler");
        assert!(
            notices[0].text.contains("library.json"),
            "die Meldung muss die betroffene Datei nennen: {}",
            notices[0].text
        );
    }

    /// Review-Punkt 4: ein `library.json` von vor `last_known_position`
    /// deserialisiert das Feld als `None` (siehe `ModInfo`s
    /// `#[serde(default)]`). Mehrere solche Alt-Einträge dürfen beim
    /// Wiederauftauchen nicht alle an Position 0 kollidieren (und dabei in
    /// umgekehrter Reihenfolge relativ zueinander landen) – sie müssen wie
    /// nie zuvor gesehene Paks behandelt werden: alphabetisch ans Ende.
    #[test]
    fn legacy_entries_without_a_known_position_do_not_collide_at_the_front() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        let mods_dir = state.paths.mods_dir();

        for name in ["b.pak", "a.pak"] {
            std::fs::write(mods_dir.join(name), b"x").unwrap();
            let mut info = minimal_mod_info(name);
            info.last_known_position = None; // wie ein Alt-Eintrag ohne dieses Feld
            state.library.mods.insert(name.to_string(), info);
        }

        reconcile_and_collect(&mut state.library, &mut state.config, &state.paths).unwrap();

        let names: Vec<&str> = state.config.entries.iter().map(|e| e.pak.as_str()).collect();
        assert_eq!(
            names,
            vec!["a.pak", "b.pak"],
            "ohne bekannte Position muss alphabetisch angehängt werden, nicht an Position 0 kollidiert"
        );
    }

    /// Review-Punkt 2: ein `library.json` von vor `ModInfo::mtime` liefert
    /// `None`, während die echte Datei ein tatsächliches `mtime` hat – der
    /// billige Vorfilter schlägt also beim ersten Lauf fehl und hasht
    /// einmal. Ohne das sofortige Neuschreiben von `library.json` in
    /// `reconcile_and_report`s Aufrufer würde das bei jedem weiteren, auch
    /// rein lesenden Aufruf erneut passieren.
    #[test]
    fn reconcile_and_report_reports_cache_refresh_so_open_can_persist_it_once() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        let mods_dir = state.paths.mods_dir();
        std::fs::write(mods_dir.join("a.pak"), b"INHALT").unwrap();
        let hash = sm2_core::library::hash_file(&mods_dir.join("a.pak")).unwrap();

        let mut legacy = minimal_mod_info("a.pak");
        legacy.hash = hash;
        legacy.size = std::fs::metadata(mods_dir.join("a.pak")).unwrap().len();
        legacy.mtime = None; // wie ein Alt-Eintrag ohne dieses Feld
        state.library.mods.insert("a.pak".to_string(), legacy);
        state.config.entries = vec![PakEntry { pak: "a.pak".into(), disabled: false }];

        let (first_run, _) =
            reconcile_and_collect(&mut state.library, &mut state.config, &state.paths).unwrap();
        assert!(first_run, "fehlendes mtime muss beim ersten Lauf als Auffrischung gemeldet werden");

        let (second_run, _) =
            reconcile_and_collect(&mut state.library, &mut state.config, &state.paths).unwrap();
        assert!(!second_run, "der aufgefrischte Cache muss beim zweiten Lauf bereits greifen");
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

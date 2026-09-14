use crate::atomic::write_atomic;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Metadaten zu einem importierten Mod. Die Pak-Datei selbst liegt im
/// Spielverzeichnis; hier steht nur, was wir darüber wissen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModInfo {
    pub pak: String,
    pub name: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub nexus_id: Option<u32>,
    #[serde(default)]
    pub notes: Option<String>,
    /// blake3 des Pak-Inhalts – erkennt Dubletten und Änderungen von außen.
    pub hash: String,
    pub size: u64,
    /// RFC-3339-Zeitstempel.
    pub imported_at: String,
    /// Pfad des Archivs oder der Datei, aus der importiert wurde.
    #[serde(default)]
    pub source: Option<String>,

    /// Aktivierungszustand, wie ihn `pak_config.yaml` beim letzten
    /// erfolgreichen `persist()` für dieses Pak trug. Dient
    /// `PakConfig::reconcile` dazu, ein zwischenzeitlich verschwundenes und
    /// wieder aufgetauchtes Pak (Spec §9 R3, z. B. nach einem Steam-Update)
    /// mit seinem vorherigen Zustand zurückzustellen, statt es wie ein nie
    /// zuvor gesehenes Pak aktiv ans Ende zu hängen. `#[serde(default)]`,
    /// damit ältere `library.json`-Dateien ohne dieses Feld weiter laden.
    #[serde(default)]
    pub last_known_disabled: bool,
    /// Position in `pak_config.yaml`, wie sie beim letzten erfolgreichen
    /// `persist()` für dieses Pak galt – siehe `last_known_disabled`.
    ///
    /// `Option`, nicht `usize`: ein `library.json` von vor diesem Feld (oder
    /// ein Pak, das noch nie Teil der Konfiguration war) muss als "keine
    /// Positions-Historie bekannt" ankommen, nicht als "Position 0". Mit
    /// einem bloßen `usize` und `#[serde(default)]` würde jeder Alt-Eintrag
    /// beim ersten `reconcile` nach einem Wiederauftauchen an den Anfang der
    /// Konfiguration springen (und mehrere solche Einträge nacheinander
    /// sogar in umgekehrter Reihenfolge) – eine stille Änderung der
    /// Ladereihenfolge, die niemand angefordert hat.
    #[serde(default)]
    pub last_known_position: Option<usize>,

    /// Größe und Änderungszeit der Pak-Datei zum Zeitpunkt der letzten
    /// Prüfung durch `detect_altered`. Dient als billiger Vorfilter (ein
    /// `stat`-Aufruf statt eines vollständigen Hashs über eine ggf. mehrere
    /// Gigabyte große Datei): weichen Größe oder Änderungszeit der Datei auf
    /// der Platte von diesen Werten ab, lohnt sich ein tatsächlicher
    /// Hash-Vergleich; stimmen beide überein, ist ein Hash-Vergleich
    /// unnötig. Ist der zuletzt geprüfte Inhalt als verändert bekannt (siehe
    /// `known_altered`), spiegeln diese Werte bewusst den *veränderten*
    /// Stand wider, nicht den ursprünglich importierten – nur so bleibt der
    /// Vorfilter auch für einen dauerhaft veränderten Pak wirksam.
    /// `#[serde(default)]`, damit ältere `library.json`-Dateien ohne dieses
    /// Feld weiter laden (der erste Lauf danach hasht dann einmalig, statt
    /// der Abweichung blind zu vertrauen).
    #[serde(default)]
    pub mtime: Option<u64>,

    /// `true`, wenn `detect_altered` den Inhalt zuletzt als vom
    /// ursprünglichen `hash` abweichend bestätigt hat ("außerhalb
    /// verändert", Spec §6.3). `hash` selbst bleibt dabei unangetastet – er
    /// bleibt der Fingerabdruck der ursprünglich importierten Version für
    /// `find_by_hash`s Dublettenerkennung, sonst würde ein späterer
    /// Re-Import genau dieser Originaldatei nicht mehr als Dublette erkannt.
    /// Zusammen mit dem auf den *veränderten* Stand aufgefrischten
    /// `size`/`mtime` erlaubt dieses Flag, die Warnung bei unverändert
    /// gebliebenem (aber weiterhin abweichendem) Inhalt aus dem Cache zu
    /// wiederholen, ohne die Datei bei jedem Lauf erneut zu hashen.
    /// `#[serde(default)]`, damit ältere `library.json`-Dateien ohne dieses
    /// Feld weiter laden.
    #[serde(default)]
    pub known_altered: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Library {
    /// Schlüssel ist der Pak-Dateiname.
    #[serde(default)]
    pub mods: BTreeMap<String, ModInfo>,
}

impl Library {
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| {
                Error::io(
                    path,
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("ungültiges JSON (Zeile {}, Spalte {})", e.line(), e.column()),
                    ),
                )
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        // Unerreichbar für die aktuellen Feldtypen (String, Option<_>, u64,
        // u32, BTreeMap<String, _> können nicht fehlschlagen); der rohe
        // Fehler wird bewusst verworfen, damit die Meldung rein deutsch bleibt.
        let json = serde_json::to_string_pretty(self).map_err(|_| {
            Error::io(
                path,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Bibliothek konnte nicht als JSON serialisiert werden",
                ),
            )
        })?;
        write_atomic(path, &format!("{json}\n"))
    }

    /// Liefert den ersten Eintrag mit diesem Hash. Bei geteiltem Hash
    /// zweier Paks (z. B. Import derselben Datei unter zwei Namen) ist das
    /// Ergebnis durch die Schlüsselreihenfolge von `BTreeMap` deterministisch
    /// der alphabetisch erste Pak-Dateiname.
    pub fn find_by_hash(&self, hash: &str) -> Option<&ModInfo> {
        self.mods.values().find(|m| m.hash == hash)
    }

    /// Erkennt Paks, deren Inhalt außerhalb des Loaders verändert wurde
    /// (Spec §6.3, dritter Abgleichsfall: Hash weicht von `library.json`
    /// ab). Nur Paks, die der Bibliothek bereits bekannt sind, werden
    /// geprüft – für ein unbekanntes Pak gibt es nichts, wogegen verglichen
    /// werden könnte (das behandelt bereits `PakConfig::reconcile`s
    /// `added`-Fall; ein von Hand hineinkopiertes Pak bekommt seine eigene
    /// Historie erst über `AppState::persist`, siehe dessen Doc-Kommentar).
    ///
    /// Ein vollständiger Hash über jede (ggf. mehrere Gigabyte große) Datei
    /// bei jedem einzelnen Aufruf ist nicht vertretbar. Deshalb zuerst ein
    /// billiger Vorfilter: Größe und Änderungszeit gegen die zuletzt
    /// bestätigten Werte (`ModInfo::size`/`ModInfo::mtime`) vergleichen –
    /// ein `stat`-Aufruf statt eines vollständigen Lesens. Nur wenn einer
    /// der beiden Werte abweicht, wird tatsächlich gehasht.
    ///
    /// Drei Ausgänge nach einem tatsächlichen Hash:
    /// - Hash bestätigt den ursprünglich importierten Inhalt (z. B. nach
    ///   einem `touch` ohne Inhaltsänderung, oder weil ein `library.json`
    ///   von vor `ModInfo::mtime` stammt und das Feld deshalb `None` trägt):
    ///   `size`/`mtime` werden aufgefrischt, `known_altered` wird (falls
    ///   gesetzt) zurückgenommen.
    /// - Hash weicht ab und das Pak galt bisher nicht als verändert: als
    ///   "außerhalb verändert" gemeldet, UND `size`/`mtime` werden auf den
    ///   *veränderten* Stand aufgefrischt und `known_altered` gesetzt.
    /// - Ein bereits als verändert bekanntes Pak (`known_altered`), dessen
    ///   Größe/Änderungszeit sich seit der letzten Prüfung nicht geändert
    ///   haben (der Vorfilter also gar nicht erst auslöst): wird ohne
    ///   erneuten Hash weiterhin gemeldet – siehe die eigene Prüfung dafür
    ///   direkt nach dem Vorfilter.
    ///
    /// In allen drei Fällen bleibt `hash` selbst unangetastet – er bleibt der
    /// Fingerabdruck der ursprünglich importierten Version für die
    /// Dublettenerkennung beim Import (`find_by_hash`). Ohne das Auffrischen
    /// von `size`/`mtime` auch im veränderten Fall (der eigentliche Grund für
    /// `known_altered`) würde ein dauerhaft dem Original abweichender Pak bei
    /// *jedem* Aufruf erneut vollständig gehasht, auf ewig – genau das
    /// bewusste Nicht-Erkennen dieses Falls war der ursprüngliche Fehler.
    ///
    /// `cache_refreshed` im Ergebnis meldet, ob sich am gespeicherten Zustand
    /// (Vorfilter-Werte oder `known_altered`) etwas geändert hat: der
    /// Aufrufer (`AppState::open`) schreibt `library.json` dann sofort neu,
    /// statt die Änderung nur im Speicher zu halten und bei jedem weiteren –
    /// auch rein lesenden – Aufruf erneut zu hashen.
    ///
    /// Eine Datei, die laut `present` existieren sollte, aber nicht (mehr)
    /// gelesen werden kann, wird stillschweigend übersprungen – das ist der
    /// Fall, den `PakConfig::reconcile`s `removed` bereits meldet. Jeder
    /// andere E/A-Fehler beim Prüfen (fehlende Leserechte, Hash-Fehlschlag)
    /// bricht die gesamte Prüfung nicht ab: Spec §6.3 beschreibt diesen
    /// dritten Fall ausdrücklich als Markierung, nicht als hartes
    /// Erfordernis – ein einzelnes unlesbares Pak darf nicht einmal
    /// schreibgeschützt lesende Befehle wie `paths` zum Scheitern bringen.
    /// Solche Fälle landen stattdessen als deutsche Meldung in `warnings`.
    pub fn detect_altered(&mut self, mods_dir: &Path, present: &[String]) -> Result<AlteredReport> {
        let mut altered = Vec::new();
        let mut warnings = Vec::new();
        let mut cache_refreshed = false;

        for pak in present {
            let Some(info) = self.mods.get(pak) else { continue };

            let path = mods_dir.join(pak);
            let metadata = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    warnings.push(format!(
                        "{pak}: Prüfung auf Veränderung übersprungen (nicht lesbar: {e})"
                    ));
                    continue;
                }
            };

            let size = metadata.len();
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs());

            if size == info.size && mtime == info.mtime {
                // Vorfilter meldet keine Änderung seit der letzten Prüfung.
                // War der Inhalt damals bereits als verändert bekannt, bleibt
                // er es – ohne erneuten Hash. Das ist der eigentliche Zweck
                // von `known_altered`: die Warnung erscheint bei jedem Lauf
                // weiter (sie ist das Signal an den Nutzer), aber der teure
                // Hash läuft nur einmal pro tatsächlicher Änderung.
                if info.known_altered {
                    altered.push(pak.clone());
                }
                continue;
            }

            let hash = match hash_file(&path) {
                Ok(h) => h,
                Err(e) => {
                    warnings.push(format!("{pak}: Prüfung auf Veränderung übersprungen ({e})"));
                    continue;
                }
            };
            if hash == info.hash {
                if let Some(entry) = self.mods.get_mut(pak) {
                    entry.size = size;
                    entry.mtime = mtime;
                    if entry.known_altered {
                        entry.known_altered = false;
                    }
                    cache_refreshed = true;
                }
            } else {
                altered.push(pak.clone());
                if let Some(entry) = self.mods.get_mut(pak) {
                    // `hash` bleibt der Fingerabdruck der ursprünglich
                    // importierten Version (Dublettenerkennung) – nur
                    // Größe/Änderungszeit werden auf den *veränderten*
                    // Stand aufgefrischt, damit der Vorfilter beim nächsten
                    // Lauf wieder greift (siehe Doc-Kommentar oben).
                    entry.size = size;
                    entry.mtime = mtime;
                    entry.known_altered = true;
                    cache_refreshed = true;
                }
            }
        }

        Ok(AlteredReport { altered, cache_refreshed, warnings })
    }
}

/// Ergebnis von `Library::detect_altered`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AlteredReport {
    /// Paks, deren Hash vom zuletzt bekannten Stand abweicht ("außerhalb
    /// verändert", Spec §6.3).
    pub altered: Vec<String>,
    /// `true`, wenn für mindestens ein Pak Größe/Änderungszeit im
    /// billigen Vorfilter nicht mehr stimmten, der Hash den Inhalt aber
    /// bestätigt hat – der Aufrufer sollte `library.json` dann neu
    /// schreiben, damit künftige Läufe den Vorfilter wieder nutzen können.
    pub cache_refreshed: bool,
    /// Deutsche Meldungen zu Paks, deren Prüfung selbst nicht möglich war
    /// (fehlende Leserechte o. Ä.) – informativ, kein Fehlschlag der
    /// gesamten Prüfung.
    pub warnings: Vec<String>,
}

/// blake3-Hash einer Datei, streamend gelesen – Paks können Gigabytes groß sein.
pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path).map_err(|e| Error::io(path, e))?;
    let mut hasher = blake3::Hasher::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| Error::io(path, e))?;
    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(pak: &str, hash: &str) -> ModInfo {
        ModInfo {
            pak: pak.to_string(),
            name: pak.trim_end_matches(".pak").to_string(),
            author: None,
            version: None,
            nexus_id: None,
            notes: None,
            hash: hash.to_string(),
            size: 42,
            imported_at: "2026-09-12T18:00:00Z".to_string(),
            source: None,
            last_known_disabled: false,
            last_known_position: Some(0),
            mtime: None,
            known_altered: false,
        }
    }

    #[test]
    fn hash_is_stable_and_distinguishes_content() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.bin");
        let b = dir.path().join("b.bin");
        std::fs::write(&a, b"identisch").unwrap();
        std::fs::write(&b, b"identisch").unwrap();

        assert_eq!(hash_file(&a).unwrap(), hash_file(&b).unwrap());

        std::fs::write(&b, b"anders").unwrap();
        assert_ne!(hash_file(&a).unwrap(), hash_file(&b).unwrap());
    }

    #[test]
    fn load_with_missing_file_yields_empty_library() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::load(&dir.path().join("gibt_es_nicht.json")).unwrap();
        assert!(lib.mods.is_empty());
    }

    #[test]
    fn save_and_load_are_inverses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        let mut lib = Library::default();
        lib.mods.insert("a.pak".into(), info("a.pak", "hash-a"));

        lib.save(&path).unwrap();

        assert_eq!(Library::load(&path).unwrap().mods, lib.mods);
    }

    #[test]
    fn finds_duplicate_by_hash() {
        let mut lib = Library::default();
        lib.mods.insert("a.pak".into(), info("a.pak", "hash-a"));

        assert_eq!(lib.find_by_hash("hash-a").map(|m| m.pak.as_str()), Some("a.pak"));
        assert!(lib.find_by_hash("unbekannt").is_none());
    }

    /// BTreeMap iteriert in Schlüsselreihenfolge; bei gemeinsamem Hash
    /// zweier Paks (z. B. derselbe Mod unter zwei Dateinamen importiert)
    /// ist das Ergebnis dadurch deterministisch der alphabetisch erste
    /// Pak-Dateiname statt einer zufälligen Auswahl.
    #[test]
    fn finds_first_pak_by_key_order_on_shared_hash() {
        let mut lib = Library::default();
        lib.mods.insert("z.pak".into(), info("z.pak", "gemeinsamer-hash"));
        lib.mods.insert("a.pak".into(), info("a.pak", "gemeinsamer-hash"));

        assert_eq!(
            lib.find_by_hash("gemeinsamer-hash").map(|m| m.pak.as_str()),
            Some("a.pak"),
            "bei gleichem Hash muss der alphabetisch erste Pak-Name gewinnen"
        );
    }

    #[test]
    fn corrupt_json_is_reported_as_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        std::fs::write(&path, "{kein json").unwrap();

        let err = Library::load(&path).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains(path.to_str().unwrap()),
            "Fehlermeldung muss den Pfad enthalten: {message}"
        );
        assert!(
            !message.contains("expected") && !message.contains("invalid"),
            "Fehlermeldung soll auf Deutsch sein, nicht die rohe serde_json-Meldung enthalten: {message}"
        );
    }

    // --- detect_altered ---------------------------------------------------

    fn write_pak(mods_dir: &Path, name: &str, content: &[u8]) -> std::fs::Metadata {
        std::fs::create_dir_all(mods_dir).unwrap();
        let path = mods_dir.join(name);
        std::fs::write(&path, content).unwrap();
        std::fs::metadata(&path).unwrap()
    }

    fn info_matching(pak: &str, metadata: &std::fs::Metadata, hash: &str) -> ModInfo {
        let mut m = info(pak, hash);
        m.size = metadata.len();
        m.mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        m
    }

    /// Beweist nicht nur, dass kein "verändert" gemeldet wird, sondern dass
    /// der billige Vorfilter tatsächlich verhindert, dass die Datei
    /// überhaupt gehasht wird: ohne Leserechte müsste ein tatsächlicher
    /// Hash-Versuch scheitern und eine Warnung hinterlassen (siehe
    /// `detect_altered_warns_instead_of_failing_on_an_unreadable_pak`
    /// unten) – bleibt `warnings` leer, wurde `hash_file` nie aufgerufen.
    /// Eine Assertion, die nur `altered.is_empty()` prüft, bliebe auch dann
    /// grün, wenn der Vorfilter versehentlich entfernt und jede Datei bei
    /// jedem Aufruf gehasht würde – genau die Eigenschaft, die diese
    /// Funktion laut ihrem eigenen Doc-Kommentar erst automatisierbar macht.
    #[test]
    fn detect_altered_ignores_an_unchanged_pak_without_hashing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.pak");
        let metadata = write_pak(dir.path(), "a.pak", b"INHALT");
        let hash = hash_file(&path).unwrap();

        let mut lib = Library::default();
        lib.mods.insert("a.pak".into(), info_matching("a.pak", &metadata, &hash));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
            if std::fs::read(&path).is_ok() {
                // Läuft der Test als root, übergeht der Kernel den
                // Leseschutz vollständig – die Beobachtbarkeit lässt sich
                // dann mit dieser Methode nicht herstellen.
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
                eprintln!("übersprungen: Prozess kann Leserechte offenbar übergehen (root?)");
                return;
            }
        }

        let report = lib.detect_altered(dir.path(), &["a.pak".into()]).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        }

        assert!(report.altered.is_empty());
        assert!(
            report.warnings.is_empty(),
            "ein tatsächlicher Hash-Versuch hätte an den entzogenen Leserechten scheitern \
             müssen und wäre als Warnung sichtbar geworden: {:?}",
            report.warnings
        );
    }

    /// Der eigentliche Zweck von `detect_altered`: ein Pak, dessen Inhalt
    /// außerhalb des Loaders ersetzt wurde (anderer Hash bei abweichender
    /// Größe/Änderungszeit), muss als verändert gemeldet werden (Spec §6.3,
    /// dritter Abgleichsfall).
    #[test]
    fn detect_altered_reports_a_pak_whose_content_was_replaced_outside_the_loader() {
        let dir = tempfile::tempdir().unwrap();
        let metadata = write_pak(dir.path(), "a.pak", b"URSPRUENGLICH");
        let hash = hash_file(&dir.path().join("a.pak")).unwrap();

        let mut lib = Library::default();
        lib.mods.insert("a.pak".into(), info_matching("a.pak", &metadata, &hash));

        // Von Hand ersetzt, ohne den Loader – Größe und Inhalt ändern sich.
        std::fs::write(dir.path().join("a.pak"), b"ERSETZT MIT ANDEREM INHALT").unwrap();

        let report = lib.detect_altered(dir.path(), &["a.pak".into()]).unwrap();

        assert_eq!(report.altered, vec!["a.pak"]);
        assert!(report.cache_refreshed, "die Erkennung selbst ist eine Zustandsänderung, die gespeichert werden muss");
        assert!(report.warnings.is_empty());
        assert_eq!(
            lib.mods["a.pak"].hash, hash,
            "der gespeicherte Hash bleibt der der zuletzt importierten Version, \
             sonst würde ein späterer Re-Import derselben Originaldatei nicht mehr \
             als Dublette erkannt"
        );
        assert!(lib.mods["a.pak"].known_altered, "der veränderte Zustand muss vermerkt werden");
        assert_eq!(
            lib.mods["a.pak"].size,
            std::fs::metadata(dir.path().join("a.pak")).unwrap().len(),
            "Größe/mtime werden auf den VERÄNDERTEN Stand aufgefrischt, sonst würde jeder \
             künftige Lauf erneut hashen (siehe detect_altered_repeats_the_advisory_...)"
        );
    }

    /// Der eigentliche Fix für Review-Punkt 2 (dritte Instanz): ein bereits
    /// als verändert erkanntes Pak muss die Warnung bei jedem weiteren Lauf
    /// wiederholen – sie ist das Signal an den Nutzer –, darf dafür aber
    /// nicht erneut gehasht werden, solange sich an Größe/Änderungszeit
    /// nichts ändert. Prüft beide Hälften: dass die Meldung bestehen bleibt,
    /// UND dass der zweite Lauf tatsächlich nicht mehr liest (nachgewiesen
    /// über entzogene Leserechte – ein tatsächlicher zweiter Hash-Versuch
    /// würde daran scheitern und als Warnung sichtbar werden, siehe
    /// `detect_altered_warns_instead_of_failing_on_an_unreadable_pak`).
    #[test]
    fn detect_altered_repeats_the_advisory_without_hashing_again_once_confirmed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.pak");
        let metadata = write_pak(dir.path(), "a.pak", b"URSPRUENGLICH");
        let original_hash = hash_file(&path).unwrap();

        let mut lib = Library::default();
        lib.mods.insert("a.pak".into(), info_matching("a.pak", &metadata, &original_hash));

        std::fs::write(&path, b"ERSETZT MIT ANDEREM INHALT").unwrap();

        // Erster Lauf: hasht tatsächlich und erkennt die Abweichung.
        let first = lib.detect_altered(dir.path(), &["a.pak".into()]).unwrap();
        assert_eq!(first.altered, vec!["a.pak"]);
        assert!(first.cache_refreshed);
        assert!(lib.mods["a.pak"].known_altered);
        assert_eq!(lib.mods["a.pak"].hash, original_hash, "Baseline-Hash bleibt für die Dublettenerkennung erhalten");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
            if std::fs::read(&path).is_ok() {
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
                eprintln!("übersprungen: Prozess kann Leserechte offenbar übergehen (root?)");
                return;
            }
        }

        let second = lib.detect_altered(dir.path(), &["a.pak".into()]).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        }

        assert_eq!(second.altered, vec!["a.pak"], "die Warnung muss bei jedem Lauf weiter erscheinen");
        assert!(!second.cache_refreshed, "ohne neue Erkenntnis gibt es nichts erneut aufzufrischen");
        assert!(
            second.warnings.is_empty(),
            "ein tatsächlicher zweiter Hash-Versuch hätte an den entzogenen Leserechten \
             scheitern müssen: {:?}",
            second.warnings
        );
    }

    /// Kehrt der Inhalt zum ursprünglich importierten Stand zurück (Hash
    /// stimmt wieder), muss `known_altered` zurückgenommen werden – sonst
    /// würde die Warnung fälschlich weiterlaufen, obwohl gar keine
    /// Abweichung mehr besteht.
    #[test]
    fn detect_altered_clears_known_altered_once_content_matches_the_original_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.pak");
        let metadata = write_pak(dir.path(), "a.pak", b"URSPRUENGLICH");
        let original_hash = hash_file(&path).unwrap();

        let mut lib = Library::default();
        lib.mods.insert("a.pak".into(), info_matching("a.pak", &metadata, &original_hash));

        std::fs::write(&path, b"ZWISCHENZEITLICH ANDERS").unwrap();
        let first = lib.detect_altered(dir.path(), &["a.pak".into()]).unwrap();
        assert_eq!(first.altered, vec!["a.pak"]);
        assert!(lib.mods["a.pak"].known_altered);

        // Originalinhalt (und – wichtig für den Vorfilter – auch die
        // ursprüngliche Größe) wird wiederhergestellt.
        std::fs::write(&path, b"URSPRUENGLICH").unwrap();
        let second = lib.detect_altered(dir.path(), &["a.pak".into()]).unwrap();

        assert!(second.altered.is_empty(), "der Originalinhalt ist wieder da – keine Abweichung mehr");
        assert!(!lib.mods["a.pak"].known_altered, "die Markierung muss zurückgenommen werden");
    }

    /// Ein Pak, das der Bibliothek unbekannt ist (nie importiert, z. B. von
    /// Hand hineinkopiert), hat nichts, wogegen verglichen werden könnte –
    /// das ist der `added`-Fall von `PakConfig::reconcile`, nicht dieser.
    #[test]
    fn detect_altered_skips_a_pak_unknown_to_the_library() {
        let dir = tempfile::tempdir().unwrap();
        write_pak(dir.path(), "fremd.pak", b"X");

        let mut lib = Library::default();
        let report = lib.detect_altered(dir.path(), &["fremd.pak".into()]).unwrap();

        assert!(report.altered.is_empty());
    }

    /// Eine veränderte Änderungszeit ohne veränderten Inhalt (z. B. durch
    /// `touch`, oder weil eine Kopie das Original bei gleichem Inhalt mit
    /// neuem Zeitstempel ersetzt hat) ist kein "außerhalb verändert" – der
    /// Hash bestätigt den unveränderten Inhalt. Der Vorfilter-Cache wird
    /// trotzdem aufgefrischt, damit künftige Läufe nicht erneut hashen.
    #[test]
    fn detect_altered_refreshes_the_cache_on_a_false_positive_from_the_cheap_prefilter() {
        let dir = tempfile::tempdir().unwrap();
        let metadata = write_pak(dir.path(), "a.pak", b"UNVERAENDERT");
        let hash = hash_file(&dir.path().join("a.pak")).unwrap();

        let mut lib = Library::default();
        let mut stale = info_matching("a.pak", &metadata, &hash);
        // Absichtlich veraltete Änderungszeit, wie sie ein `touch` oder eine
        // erneute Kopie mit unverändertem Inhalt hinterlassen könnte.
        stale.mtime = stale.mtime.map(|t| t.saturating_sub(3600));
        lib.mods.insert("a.pak".into(), stale);

        let report = lib.detect_altered(dir.path(), &["a.pak".into()]).unwrap();

        assert!(report.altered.is_empty(), "unveränderter Inhalt darf nicht als verändert gelten");
        assert!(report.cache_refreshed, "eine Vorfilter-Auffrischung muss gemeldet werden (2)");
        assert_eq!(
            lib.mods["a.pak"].mtime, metadata.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()),
            "der Vorfilter-Cache muss nach der Bestätigung aufgefrischt werden"
        );
    }

    /// Simuliert genau das Szenario aus Review-Punkt 2: ein `library.json`
    /// von vor `ModInfo::mtime` deserialisiert dieses Feld als `None` (siehe
    /// `#[serde(default)]`), während die echte Datei ein tatsächliches
    /// `mtime` hat. Ohne `cache_refreshed` würde das bei jedem einzelnen
    /// Aufruf – auch rein lesenden Befehlen wie `list`/`paths` – erneut zu
    /// einem vollständigen Hash führen, unbegrenzt oft, weil `library.json`
    /// von diesen Befehlen nie neu geschrieben wird.
    #[test]
    fn detect_altered_reports_cache_refresh_for_a_pre_mtime_library_entry() {
        let dir = tempfile::tempdir().unwrap();
        let metadata = write_pak(dir.path(), "a.pak", b"INHALT");
        let hash = hash_file(&dir.path().join("a.pak")).unwrap();

        let mut lib = Library::default();
        let mut legacy = info("a.pak", &hash);
        legacy.size = metadata.len();
        legacy.mtime = None; // wie ein Alt-Eintrag ohne dieses Feld
        lib.mods.insert("a.pak".into(), legacy);

        let first_run = lib.detect_altered(dir.path(), &["a.pak".into()]).unwrap();
        assert!(first_run.altered.is_empty());
        assert!(first_run.cache_refreshed, "fehlendes mtime muss als Vorfilter-Abweichung erkannt werden");

        // Nach dem (simulierten) Neuschreiben von library.json greift der
        // Vorfilter jetzt wieder: kein zweiter Hash-Versuch nötig.
        let second_run = lib.detect_altered(dir.path(), &["a.pak".into()]).unwrap();
        assert!(second_run.altered.is_empty());
        assert!(!second_run.cache_refreshed, "der Vorfilter muss beim zweiten Lauf bereits greifen");
    }

    /// Item 3: ein einzelnes unlesbares Pak darf die gesamte Prüfung nicht
    /// scheitern lassen (das würde selbst `sm2 paths` betreffen, das nie
    /// Pak-Inhalte liest) – Spec §6.3 beschreibt diesen Fall als Markierung,
    /// nicht als hartes Erfordernis.
    #[cfg(unix)]
    #[test]
    fn detect_altered_warns_instead_of_failing_on_an_unreadable_pak() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.pak");
        write_pak(dir.path(), "a.pak", b"INHALT");

        let mut lib = Library::default();
        // Größe/Hash weichen bewusst vom billigen Vorfilter ab, damit der
        // Codepfad tatsächlich bis zum (dann scheiternden) Hash-Versuch
        // kommt, statt schon vorher überzuspringen.
        let mut mismatched = info("a.pak", "irrelevant");
        mismatched.size = 0;
        mismatched.mtime = None;
        lib.mods.insert("a.pak".into(), mismatched);

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read(&path).is_ok() {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            eprintln!("übersprungen: Prozess kann Leserechte offenbar übergehen (root?)");
            return;
        }

        let result = lib.detect_altered(dir.path(), &["a.pak".into()]);

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let report = result.expect("ein unlesbares Pak darf die Prüfung nicht scheitern lassen");
        assert!(report.altered.is_empty(), "ohne lesbaren Inhalt kann nichts als verändert gelten");
        assert_eq!(report.warnings.len(), 1, "die Nichtlesbarkeit muss als Warnung sichtbar werden");
        assert!(report.warnings[0].contains("a.pak"));
    }

    /// Eine Datei, die laut Aufrufer vorhanden sein sollte, aber (z. B. in
    /// einer seltenen Race-Bedingung zwischen `list_paks` und diesem Aufruf)
    /// nicht mehr gelesen werden kann, darf `detect_altered` nicht scheitern
    /// lassen – dieser Fall gehört `PakConfig::reconcile`s `removed`.
    #[test]
    fn detect_altered_skips_a_pak_that_disappeared_since_being_listed() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::default();
        lib.mods.insert("weg.pak".into(), info("weg.pak", "irrelevant"));

        let report = lib.detect_altered(dir.path(), &["weg.pak".into()]).unwrap();

        assert!(report.altered.is_empty());
    }

    // --- last_known_position: Option statt usize (Review-Punkt 4) --------

    /// Ein `library.json` von vor `last_known_position` lässt das Feld ganz
    /// weg – nicht nur mit dem Wert 0. Deserialisiert es zu `Some(0)` statt
    /// `None`, würde ein solcher Alt-Eintrag beim nächsten `reconcile` nach
    /// einem Wiederauftauchen fälschlich an die erste Position springen.
    #[test]
    fn legacy_json_without_last_known_position_yields_none_not_zero() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        std::fs::write(
            &path,
            r#"{"mods":{"a.pak":{"pak":"a.pak","name":"a","hash":"h","size":1,"imported_at":"2026-01-01T00:00:00Z"}}}"#,
        )
        .unwrap();

        let lib = Library::load(&path).unwrap();

        assert_eq!(
            lib.mods["a.pak"].last_known_position, None,
            "fehlende Historie darf nicht als Position 0 erscheinen"
        );
        assert!(!lib.mods["a.pak"].last_known_disabled);
    }
}

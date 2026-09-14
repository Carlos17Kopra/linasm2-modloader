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
    #[serde(default)]
    pub last_known_position: usize,

    /// Größe und Änderungszeit der Pak-Datei zum Zeitpunkt, als `hash`
    /// zuletzt bestätigt wurde. Dient `detect_altered` als billigem
    /// Vorfilter (ein `stat`-Aufruf statt eines vollständigen Hashs über
    /// eine ggf. mehrere Gigabyte große Datei): weichen Größe oder
    /// Änderungszeit der Datei auf der Platte von diesen Werten ab, lohnt
    /// sich ein tatsächlicher Hash-Vergleich; stimmen beide überein, ist ein
    /// Hash-Vergleich unnötig. `#[serde(default)]`, damit ältere
    /// `library.json`-Dateien ohne dieses Feld weiter laden (der erste Lauf
    /// danach hasht dann einmalig, statt der Abweichung blind zu vertrauen).
    #[serde(default)]
    pub mtime: Option<u64>,
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
    /// `added`-Fall).
    ///
    /// Ein vollständiger Hash über jede (ggf. mehrere Gigabyte große) Datei
    /// bei jedem einzelnen Aufruf ist nicht vertretbar. Deshalb zuerst ein
    /// billiger Vorfilter: Größe und Änderungszeit gegen die zuletzt
    /// bestätigten Werte (`ModInfo::size`/`ModInfo::mtime`) vergleichen –
    /// ein `stat`-Aufruf statt eines vollständigen Lesens. Nur wenn einer
    /// der beiden Werte abweicht, wird tatsächlich gehasht. Bestätigt der
    /// Hash trotzdem den unveränderten Inhalt (z. B. nach einem `touch` ohne
    /// Inhaltsänderung), wird die Vorfilter-Information aufgefrischt, damit
    /// künftige Läufe nicht erneut hashen müssen – der eigentliche `hash`
    /// bleibt dabei unangetastet, er bleibt der Fingerabdruck der zuletzt
    /// importierten/bestätigten Version für die Dublettenerkennung beim
    /// Import.
    ///
    /// Eine Datei, die laut `present` existieren sollte, aber nicht (mehr)
    /// gelesen werden kann, wird stillschweigend übersprungen – das ist der
    /// Fall, den `PakConfig::reconcile`s `removed` bereits meldet.
    pub fn detect_altered(&mut self, mods_dir: &Path, present: &[String]) -> Result<Vec<String>> {
        let mut altered = Vec::new();

        for pak in present {
            let Some(info) = self.mods.get(pak) else { continue };

            let path = mods_dir.join(pak);
            let metadata = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(Error::io(&path, e)),
            };

            let size = metadata.len();
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs());

            if size == info.size && mtime == info.mtime {
                continue;
            }

            let hash = hash_file(&path)?;
            if hash == info.hash {
                if let Some(entry) = self.mods.get_mut(pak) {
                    entry.size = size;
                    entry.mtime = mtime;
                }
            } else {
                altered.push(pak.clone());
            }
        }

        Ok(altered)
    }
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
            last_known_position: 0,
            mtime: None,
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

    #[test]
    fn detect_altered_ignores_an_unchanged_pak_without_hashing() {
        let dir = tempfile::tempdir().unwrap();
        let metadata = write_pak(dir.path(), "a.pak", b"INHALT");
        let hash = hash_file(&dir.path().join("a.pak")).unwrap();

        let mut lib = Library::default();
        lib.mods.insert("a.pak".into(), info_matching("a.pak", &metadata, &hash));

        let altered = lib.detect_altered(dir.path(), &["a.pak".into()]).unwrap();

        assert!(altered.is_empty());
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

        let altered = lib.detect_altered(dir.path(), &["a.pak".into()]).unwrap();

        assert_eq!(altered, vec!["a.pak"]);
        assert_eq!(
            lib.mods["a.pak"].hash, hash,
            "der gespeicherte Hash bleibt der der zuletzt importierten Version, \
             sonst würde ein späterer Re-Import derselben Originaldatei nicht mehr \
             als Dublette erkannt"
        );
    }

    /// Ein Pak, das der Bibliothek unbekannt ist (nie importiert, z. B. von
    /// Hand hineinkopiert), hat nichts, wogegen verglichen werden könnte –
    /// das ist der `added`-Fall von `PakConfig::reconcile`, nicht dieser.
    #[test]
    fn detect_altered_skips_a_pak_unknown_to_the_library() {
        let dir = tempfile::tempdir().unwrap();
        write_pak(dir.path(), "fremd.pak", b"X");

        let mut lib = Library::default();
        let altered = lib.detect_altered(dir.path(), &["fremd.pak".into()]).unwrap();

        assert!(altered.is_empty());
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

        let altered = lib.detect_altered(dir.path(), &["a.pak".into()]).unwrap();

        assert!(altered.is_empty(), "unveränderter Inhalt darf nicht als verändert gelten");
        assert_eq!(
            lib.mods["a.pak"].mtime, metadata.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()),
            "der Vorfilter-Cache muss nach der Bestätigung aufgefrischt werden"
        );
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

        let altered = lib.detect_altered(dir.path(), &["weg.pak".into()]).unwrap();

        assert!(altered.is_empty());
    }
}

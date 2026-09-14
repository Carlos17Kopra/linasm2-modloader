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
        let json = serde_json::to_string_pretty(self).map_err(|e| {
            Error::io(
                path,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Bibliothek konnte nicht serialisiert werden: {e}"),
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
        }
    }

    #[test]
    fn hash_ist_stabil_und_unterscheidet_inhalte() {
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
    fn load_bei_fehlender_datei_ergibt_leere_bibliothek() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::load(&dir.path().join("gibt_es_nicht.json")).unwrap();
        assert!(lib.mods.is_empty());
    }

    #[test]
    fn save_und_load_sind_zueinander_invers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        let mut lib = Library::default();
        lib.mods.insert("a.pak".into(), info("a.pak", "hash-a"));

        lib.save(&path).unwrap();

        assert_eq!(Library::load(&path).unwrap().mods, lib.mods);
    }

    #[test]
    fn findet_dublette_ueber_den_hash() {
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
    fn findet_bei_geteiltem_hash_deterministisch_den_ersten_pak_nach_schluesselreihenfolge() {
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
    fn beschaedigte_json_datei_wird_als_fehler_gemeldet() {
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
}

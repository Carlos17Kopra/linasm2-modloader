use crate::atomic::write_atomic;
use crate::error::{Error, Result};
use crate::pak_config::{PakConfig, PakEntry};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Ein Eintrag in einem Profil: welches Pak, und ob es aktiv ist.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileEntry {
    pub pak: String,
    #[serde(default)]
    pub disabled: bool,
}

/// Eine benannte Zusammenstellung: Mod-Auswahl plus Ladereihenfolge.
/// Genau die Information, die pak_config.yaml braucht – nur ein paar hundert Byte.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub entries: Vec<ProfileEntry>,
}

impl Profile {
    pub fn from_config(name: &str, cfg: &PakConfig) -> Self {
        Self {
            name: name.to_string(),
            entries: cfg
                .entries
                .iter()
                .map(|e| ProfileEntry { pak: e.pak.clone(), disabled: e.disabled })
                .collect(),
        }
    }

    /// Baut die Konfiguration für dieses Profil. Fehlende Paks (im Profil
    /// gelistet, aber keine Datei mehr im Verzeichnis) werden übersprungen
    /// und zurückgemeldet; das Profil selbst bleibt unverändert.
    ///
    /// Vorhandene, dem Profil unbekannte Paks kommen deaktiviert ans Ende
    /// der neuen Konfiguration – die Engine lädt jedes Pak, das im
    /// Verzeichnis liegt, unabhängig davon, ob es in pak_config.yaml steht;
    /// ein nicht gelistetes Pak würde also sonst ungesteuert und zuerst
    /// geladen. Ein doppelt gemeldetes unbekanntes Pak in `present` wird nur
    /// einmal aufgenommen (siehe `PakConfig::reconcile`, gleiche Regel).
    pub fn apply(&self, present: &[String]) -> (PakConfig, Vec<String>) {
        let present_set: HashSet<&str> = present.iter().map(String::as_str).collect();

        let mut missing = Vec::new();
        let mut entries = Vec::new();
        let mut known: HashSet<&str> = HashSet::new();

        for entry in &self.entries {
            if present_set.contains(entry.pak.as_str()) {
                entries.push(PakEntry { pak: entry.pak.clone(), disabled: entry.disabled });
                known.insert(entry.pak.as_str());
            } else {
                missing.push(entry.pak.clone());
            }
        }

        let mut unknown: Vec<&String> = Vec::new();
        for pak in present {
            if known.insert(pak.as_str()) {
                unknown.push(pak);
            }
        }
        unknown.sort();

        for pak in unknown {
            entries.push(PakEntry { pak: pak.clone(), disabled: true });
        }

        (PakConfig { entries }, missing)
    }

    /// Leitet einen stabilen, eindeutigen Dateinamen aus dem Profilnamen ab.
    ///
    /// Der Profilname selbst steht im Dateiinhalt (`name`-Feld) – der
    /// Dateiname muss also nur stabil und eindeutig sein, nicht umkehrbar.
    /// Ein reines "ersetze Sonderzeichen durch '-'"-Schema kann das nicht
    /// garantieren: ein Name, der nur aus Sonderzeichen besteht (z. B.
    /// "???"), würde zu einem leeren Stamm (versteckte Datei ohne Namen),
    /// und zwei unterschiedliche Namen wie "Mein Profil" und "mein-profil"
    /// würden auf dieselbe Datei abgebildet und sich gegenseitig
    /// überschreiben. Deshalb hängt der Dateiname zusätzlich die ersten 8
    /// Hex-Zeichen des blake3-Hashs des *ungekürzten* Originalnamens an: das
    /// hält den Namen lesbar, macht den Stamm nie leer (der Hash-Teil ist
    /// immer da) und lässt unterschiedliche Namen nie kollidieren (ein
    /// Hash-Zusammenstoß ist praktisch ausgeschlossen).
    fn file_stem(name: &str) -> String {
        let slug: String = name
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
            .collect::<String>()
            .trim_matches('-')
            .to_lowercase();

        let digest = blake3::hash(name.as_bytes()).to_hex();
        let suffix = &digest.as_str()[..8];

        if slug.is_empty() {
            suffix.to_string()
        } else {
            format!("{slug}-{suffix}")
        }
    }

    fn file_name(&self) -> String {
        format!("{}.toml", Self::file_stem(&self.name))
    }

    /// Der Pfad, unter dem `save(dir)` dieses Profil ablegt bzw. abgelegt
    /// hat. Öffentlich, damit ein Aufrufer (z. B. `profile delete` in der
    /// CLI), der ein bereits über `list_profiles` geladenes `Profile` vor
    /// sich hat, dessen Datei ansprechen kann, ohne das
    /// Namens-zu-Dateiname-Schema (`file_stem`) selbst nachzubauen.
    pub fn path_in(&self, dir: &Path) -> PathBuf {
        dir.join(self.file_name())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
        toml::from_str(&text).map_err(|e| {
            let message = crate::error::describe_toml_error(&text, &e);
            Error::io(path, std::io::Error::new(std::io::ErrorKind::InvalidData, message))
        })
    }

    pub fn save(&self, dir: &Path) -> Result<PathBuf> {
        let path = dir.join(self.file_name());
        let text = toml::to_string_pretty(self).map_err(|e| {
            Error::io(&path, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        write_atomic(&path, &text)?;
        Ok(path)
    }
}

/// Listet alle Profile in `dir` alphabetisch nach Namen.
///
/// Nicht-`.toml`-Dateien werden ignoriert. Eine `.toml`-Datei, die nicht
/// gelesen oder nicht als `Profile` interpretiert werden kann (kaputtes
/// TOML, oder gültiges TOML ohne die erwarteten Felder), wird stillschweigend
/// übersprungen statt die ganze Auflistung scheitern zu lassen – eine
/// einzelne beschädigte Datei soll nicht alle anderen Profile unsichtbar
/// machen. Das bedeutet allerdings auch: ein Profil kann so aus der Liste
/// verschwinden, ohne dass eine Fehlermeldung erscheint. Ein Pfad, der kein
/// Verzeichnis ist (fehlt, oder ist eine Datei), ergibt eine leere Liste.
pub fn list_profiles(dir: &Path) -> Result<Vec<Profile>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut profiles: Vec<Profile> = std::fs::read_dir(dir)
        .map_err(|e| Error::io(dir, e))?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("toml"))
        .filter_map(|path| Profile::load(&path).ok())
        .collect();

    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(profiles)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(entries: &[(&str, bool)]) -> PakConfig {
        PakConfig {
            entries: entries
                .iter()
                .map(|(pak, disabled)| PakEntry { pak: pak.to_string(), disabled: *disabled })
                .collect(),
        }
    }

    #[test]
    fn from_config_preserves_order_and_state() {
        let profile = Profile::from_config("Astartes", &config(&[("z.pak", false), ("a.pak", true)]));

        assert_eq!(profile.name, "Astartes");
        assert_eq!(profile.entries[0].pak, "z.pak");
        assert!(!profile.entries[0].disabled);
        assert!(profile.entries[1].disabled);
    }

    #[test]
    fn apply_restores_order_and_state() {
        let profile = Profile::from_config("P", &config(&[("z.pak", false), ("a.pak", true)]));
        let (result, missing) = profile.apply(&["a.pak".into(), "z.pak".into()]);

        let names: Vec<&str> = result.entries.iter().map(|e| e.pak.as_str()).collect();
        assert_eq!(names, vec!["z.pak", "a.pak"]);
        assert!(result.entries[1].disabled);
        assert!(missing.is_empty());
    }

    #[test]
    fn apply_reports_missing_paks_and_skips_them() {
        let profile = Profile::from_config("P", &config(&[("weg.pak", false), ("da.pak", false)]));
        let (result, missing) = profile.apply(&["da.pak".into()]);

        assert_eq!(missing, vec!["weg.pak"]);
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].pak, "da.pak");
    }

    #[test]
    fn apply_includes_unknown_paks_disabled() {
        // Ein Pak im Verzeichnis, das das Profil nicht kennt, würde sonst
        // ungesteuert zuerst geladen (Engine-Regel).
        let profile = Profile::from_config("P", &config(&[("bekannt.pak", false)]));
        let (result, _) = profile.apply(&["bekannt.pak".into(), "fremd.pak".into()]);

        assert_eq!(result.entries.len(), 2);
        let fremd = result.entries.iter().find(|e| e.pak == "fremd.pak").unwrap();
        assert!(fremd.disabled, "unbekannte Paks dürfen nicht stillschweigend aktiv sein");
    }

    #[test]
    fn apply_deduplicates_repeated_unknown_paks_in_present() {
        let profile = Profile::from_config("P", &config(&[]));
        let (result, _) =
            profile.apply(&["fremd.pak".into(), "fremd.pak".into(), "a.pak".into()]);

        let count = result.entries.iter().filter(|e| e.pak == "fremd.pak").count();
        assert_eq!(count, 1, "ein doppelt gemeldetes unbekanntes Pak darf nur einmal auftauchen");
        assert_eq!(result.entries.len(), 2);
    }

    #[test]
    fn apply_ignores_repeated_present_entries_for_known_paks() {
        let profile = Profile::from_config("P", &config(&[("a.pak", false)]));
        let (result, missing) = profile.apply(&["a.pak".into(), "a.pak".into()]);

        assert_eq!(result.entries.len(), 1, "ein doppelt gemeldetes bekanntes Pak darf nicht dupliziert werden");
        assert!(missing.is_empty());
    }

    #[test]
    fn apply_preserves_duplicate_entries_already_in_the_profile() {
        // Ein von Hand bearbeitetes Profil kann ein Pak bereits doppelt
        // enthalten. apply darf das nicht "reparieren" (siehe
        // `PakConfig::reconcile`, gleiche Designentscheidung) – es gibt hier
        // keine sinnvolle Regel, welcher der beiden Zustände gewinnen sollte.
        let profile = Profile {
            name: "P".into(),
            entries: vec![
                ProfileEntry { pak: "a.pak".into(), disabled: false },
                ProfileEntry { pak: "a.pak".into(), disabled: true },
            ],
        };
        let (result, missing) = profile.apply(&["a.pak".into()]);

        assert_eq!(
            result.entries,
            vec![
                PakEntry { pak: "a.pak".into(), disabled: false },
                PakEntry { pak: "a.pak".into(), disabled: true },
            ]
        );
        assert!(missing.is_empty());
    }

    #[test]
    fn apply_on_empty_profile_disables_all_present_paks() {
        let profile = Profile::from_config("Leer", &config(&[]));
        let (result, missing) = profile.apply(&["b.pak".into(), "a.pak".into()]);

        let names: Vec<&str> = result.entries.iter().map(|e| e.pak.as_str()).collect();
        assert_eq!(names, vec!["a.pak", "b.pak"], "unbekannte Paks werden alphabetisch angehängt");
        assert!(result.entries.iter().all(|e| e.disabled));
        assert!(missing.is_empty());
    }

    #[test]
    fn save_and_load_are_inverses() {
        let dir = tempfile::tempdir().unwrap();
        let profile = Profile::from_config("Mein Profil", &config(&[("a.pak", true)]));

        let path = profile.save(dir.path()).unwrap();

        assert_eq!(Profile::load(&path).unwrap(), profile);
    }

    #[test]
    fn path_in_matches_the_path_save_actually_used() {
        let dir = tempfile::tempdir().unwrap();
        let profile = Profile::from_config("Mein Profil", &config(&[]));

        let saved_path = profile.save(dir.path()).unwrap();

        assert_eq!(profile.path_in(dir.path()), saved_path);
    }

    #[test]
    fn save_creates_missing_parent_directory() {
        let dir = tempfile::tempdir().unwrap();
        let profiles_dir = dir.path().join("profiles").join("nested");
        let profile = Profile::from_config("P", &config(&[]));

        let path = profile.save(&profiles_dir).unwrap();

        assert!(path.exists());
    }

    #[test]
    fn list_is_alphabetical_and_ignores_foreign_files() {
        let dir = tempfile::tempdir().unwrap();
        Profile::from_config("Zulu", &config(&[])).save(dir.path()).unwrap();
        Profile::from_config("Alpha", &config(&[])).save(dir.path()).unwrap();
        std::fs::write(dir.path().join("notizen.txt"), b"egal").unwrap();

        let names: Vec<String> =
            list_profiles(dir.path()).unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(names, vec!["Alpha", "Zulu"]);
    }

    #[test]
    fn list_skips_toml_files_that_are_not_profiles() {
        let dir = tempfile::tempdir().unwrap();
        Profile::from_config("Gut", &config(&[])).save(dir.path()).unwrap();
        // Gültiges TOML, aber ohne das erforderliche Feld "name".
        std::fs::write(dir.path().join("fremd.toml"), b"irgendwas = 1\n").unwrap();

        let names: Vec<String> =
            list_profiles(dir.path()).unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(names, vec!["Gut"]);
    }

    #[test]
    fn list_on_a_file_path_yields_empty_list_instead_of_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("keine_ahnung.toml");
        std::fs::write(&file, b"name = \"X\"\n").unwrap();

        assert_eq!(list_profiles(&file).unwrap(), Vec::new());
    }

    #[test]
    fn list_on_missing_directory_yields_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(list_profiles(&dir.path().join("gibt_es_nicht")).unwrap(), Vec::new());
    }

    /// Analog zu `settings.rs`s gleichnamigem Test: `Profile::load` muss
    /// einen kaputten TOML-Inhalt mit Pfad und auf Deutsch melden, nicht mit
    /// der rohen (englischen, mehrzeiligen) `toml::de::Error`-Meldung.
    #[test]
    fn corrupt_file_is_reported_with_path_in_german() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kaputt.toml");
        std::fs::write(&path, b"das ist kein toml : : :").unwrap();

        let err = Profile::load(&path).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains(path.to_str().unwrap()),
            "Fehlermeldung muss den Pfad enthalten: {message}"
        );
        assert!(
            !message.contains("expected") && !message.contains("invalid"),
            "Fehlermeldung soll auf Deutsch sein, nicht die rohe toml-Meldung enthalten: {message}"
        );
    }

    #[test]
    fn file_names_do_not_collide_for_names_that_sanitize_to_the_same_slug() {
        let dir = tempfile::tempdir().unwrap();
        let a = Profile::from_config("Mein Profil", &config(&[])).save(dir.path()).unwrap();
        let b = Profile::from_config("mein-profil", &config(&[])).save(dir.path()).unwrap();

        assert_ne!(a, b, "unterschiedliche Namen dürfen nicht dieselbe Datei treffen");
        assert_eq!(Profile::load(&a).unwrap().name, "Mein Profil");
        assert_eq!(Profile::load(&b).unwrap().name, "mein-profil");
    }

    #[test]
    fn file_name_is_never_empty_for_a_punctuation_only_name() {
        let dir = tempfile::tempdir().unwrap();
        let profile = Profile::from_config("???", &config(&[]));

        let path = profile.save(dir.path()).unwrap();

        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap();
        assert!(!stem.is_empty(), "der Dateistamm darf nie leer sein");
        assert_eq!(Profile::load(&path).unwrap().name, "???");
    }

    #[test]
    fn load_returns_the_original_name_even_when_the_filename_scheme_maps_it_away() {
        let dir = tempfile::tempdir().unwrap();
        let profile = Profile::from_config("Ä ö ü !!!", &config(&[]));

        let path = profile.save(dir.path()).unwrap();

        assert_eq!(Profile::load(&path).unwrap().name, "Ä ö ü !!!");
    }
}

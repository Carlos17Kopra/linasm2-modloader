use crate::atomic::write_atomic;
use crate::error::{Error, Result};
use crate::pak_config::{PakConfig, PakEntry};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// An entry in a profile: which pak, and whether it is active.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileEntry {
    pub pak: String,
    #[serde(default)]
    pub disabled: bool,
}

/// A named set-up: mod selection plus load order. Exactly the information
/// pak_config.yaml needs — only a few hundred bytes.
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

    /// Builds the configuration for this profile. Missing paks (listed in
    /// the profile, but no longer a file in the directory) are skipped and
    /// reported back; the profile itself stays unchanged.
    ///
    /// Paks that are present but unknown to the profile are appended
    /// disabled at the end of the new configuration. The engine loads every
    /// pak lying in the directory regardless of whether it appears in
    /// pak_config.yaml, so an unlisted pak would otherwise be loaded first
    /// and uncontrolled. An unknown pak reported twice in `present` is taken
    /// in only once (see `PakConfig::reconcile`, same rule).
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

    /// Derives a stable, unique file name from the profile name.
    ///
    /// The profile name itself lives in the file's content (the `name`
    /// field), so the file name only has to be stable and unique, not
    /// reversible. A plain "replace special characters with '-'" scheme
    /// cannot guarantee that: a name made up entirely of special characters
    /// (for example "???") would yield an empty stem (a hidden file without
    /// a name), and two different names such as "Mein Profil" and
    /// "mein-profil" would map to the same file and overwrite each other.
    /// That is why the file name additionally appends the first 8 hex
    /// characters of the blake3 hash of the *unshortened* original name:
    /// this keeps the name readable, never lets the stem be empty (the hash
    /// part is always there), and never lets different names collide (a
    /// hash collision is practically impossible).
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

    /// The path where `save(dir)` puts, or has put, this profile. Public so
    /// that a caller holding a `Profile` already loaded through
    /// `list_profiles` (`profile delete` in the CLI, say) can address its
    /// file without rebuilding the name-to-file-name scheme (`file_stem`)
    /// itself.
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

/// Lists all profiles in `dir`, alphabetically by name.
///
/// Non-`.toml` files are ignored. A `.toml` file that cannot be read, or
/// cannot be interpreted as a `Profile` (broken TOML, or valid TOML without
/// the expected fields), is skipped silently instead of failing the whole
/// listing — a single damaged file should not make every other profile
/// invisible. That does mean, though, that a profile can vanish from the
/// list without any error message appearing. A path that is not a directory
/// (missing, or a file) yields an empty list.
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
        // A pak in the directory that the profile does not know would
        // otherwise be loaded first and uncontrolled (engine rule).
        let profile = Profile::from_config("P", &config(&[("bekannt.pak", false)]));
        let (result, _) = profile.apply(&["bekannt.pak".into(), "fremd.pak".into()]);

        assert_eq!(result.entries.len(), 2);
        let fremd = result.entries.iter().find(|e| e.pak == "fremd.pak").unwrap();
        assert!(fremd.disabled, "unknown paks must not be silently active");
    }

    #[test]
    fn apply_deduplicates_repeated_unknown_paks_in_present() {
        let profile = Profile::from_config("P", &config(&[]));
        let (result, _) =
            profile.apply(&["fremd.pak".into(), "fremd.pak".into(), "a.pak".into()]);

        let count = result.entries.iter().filter(|e| e.pak == "fremd.pak").count();
        assert_eq!(count, 1, "an unknown pak reported twice may only show up once");
        assert_eq!(result.entries.len(), 2);
    }

    #[test]
    fn apply_ignores_repeated_present_entries_for_known_paks() {
        let profile = Profile::from_config("P", &config(&[("a.pak", false)]));
        let (result, missing) = profile.apply(&["a.pak".into(), "a.pak".into()]);

        assert_eq!(result.entries.len(), 1, "a known pak reported twice must not be duplicated");
        assert!(missing.is_empty());
    }

    #[test]
    fn apply_preserves_duplicate_entries_already_in_the_profile() {
        // A hand-edited profile may already contain a pak twice. apply must
        // not "repair" that (see `PakConfig::reconcile`, same design
        // decision) — there is no sensible rule here for which of the two
        // states should win.
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
        assert_eq!(names, vec!["a.pak", "b.pak"], "unknown paks are appended alphabetically");
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
        // Valid TOML, but without the required "name" field.
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

    /// Same as the test of that name in `settings.rs`: `Profile::load` must
    /// report broken TOML content with the path, not with the raw (English,
    /// multi-line) `toml::de::Error` message.
    #[test]
    fn corrupt_file_error_names_the_path_without_leaking_the_raw_toml_message() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kaputt.toml");
        let garbage = "das ist kein toml : : :";
        std::fs::write(&path, garbage).unwrap();

        let err = Profile::load(&path).unwrap_err();
        let message = err.to_string();
        // See the comment on the equivalent assertion in `settings.rs` for
        // why this compares against the raw message itself rather than a
        // fixed word.
        let raw = toml::from_str::<toml::Value>(garbage).unwrap_err().to_string();

        assert!(
            message.contains(path.to_str().unwrap()),
            "the error message must contain the path: {message}"
        );
        assert!(
            !message.contains(&raw),
            "the message must be composed from the catalogue, not the raw toml::de::Error text: {message}"
        );
    }

    #[test]
    fn file_names_do_not_collide_for_names_that_sanitize_to_the_same_slug() {
        let dir = tempfile::tempdir().unwrap();
        let a = Profile::from_config("Mein Profil", &config(&[])).save(dir.path()).unwrap();
        let b = Profile::from_config("mein-profil", &config(&[])).save(dir.path()).unwrap();

        assert_ne!(a, b, "different names must not end up in the same file");
        assert_eq!(Profile::load(&a).unwrap().name, "Mein Profil");
        assert_eq!(Profile::load(&b).unwrap().name, "mein-profil");
    }

    #[test]
    fn file_name_is_never_empty_for_a_punctuation_only_name() {
        let dir = tempfile::tempdir().unwrap();
        let profile = Profile::from_config("???", &config(&[]));

        let path = profile.save(dir.path()).unwrap();

        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap();
        assert!(!stem.is_empty(), "the file stem must never be empty");
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

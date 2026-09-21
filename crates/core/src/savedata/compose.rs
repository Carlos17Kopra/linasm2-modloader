//! Composing a new backup out of several existing ones.
//!
//! One backup is the base and supplies everything; a replacement names
//! a part and the backup it should come from instead. Nothing is
//! written until every replacement has gone through, so a refusal
//! anywhere leaves no half-composed backup behind — not even one to
//! clean up.
//!
//! Files the catalogue knows nothing about, and files that no
//! replacement touches, are carried over from the base byte for byte.
//! Only a file that actually changes is encoded again. That keeps the
//! promise the tests check: a composition without a single replacement
//! is the base, file for file.

use crate::error::{Error, Result, SaveDataDefect};
use crate::savedata::catalogue::{self, Documents};
use crate::savedata::merge;
use crate::savedata::ssf1;
use crate::saves::{self, BackupEntry, Composition};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Builds a new backup from `base`, with each named part taken from the
/// backup beside it.
pub fn compose(
    base: &BackupEntry,
    replacements: &[(String, BackupEntry)],
    backup_root: &Path,
    label: Option<&str>,
) -> Result<BackupEntry> {
    saves::verify(base)?;
    let mut files = saves::read_files(base)?;
    let mut documents = catalogue::documents(&files)?;

    // Every source is read once, however many parts come from it.
    let mut sources: BTreeMap<String, Documents> = BTreeMap::new();
    let mut touched: BTreeSet<String> = BTreeSet::new();
    let mut recorded: BTreeMap<String, String> = BTreeMap::new();

    for (part_id, source_entry) in replacements {
        let part = catalogue::part_by_id(&documents, part_id)?;

        if !sources.contains_key(&source_entry.created_at) {
            saves::verify(source_entry)?;
            let source_files = saves::read_files(source_entry)?;
            sources.insert(source_entry.created_at.clone(), catalogue::documents(&source_files)?);
        }
        let source = &sources[&source_entry.created_at];

        merge::apply(&mut documents, &part, source)?;
        touched.insert(part.file.to_string());
        recorded.insert(part_id.clone(), source_entry.created_at.clone());
    }

    for name in &touched {
        let json = serde_json::to_vec(&documents[name]).expect("a decoded document serialises");
        let encoded = ssf1::encode(&json);
        // The encoder is exercised on every composition, so it is worth
        // reading the result back before it becomes a backup: a file
        // that does not decode to what went in never reaches the disk.
        verify_round_trip(name, &encoded, &json)?;
        files.insert(name.clone(), encoded);
    }

    saves::write_composition(
        &files,
        backup_root,
        label,
        &base.archive,
        Composition { base: base.created_at.clone(), parts: recorded },
    )
}

/// Refuses a container whose bytes do not decode back to `expected`.
///
/// Split out from `compose` so the refusal itself — not the codec,
/// which round-trips correctly by construction and cannot honestly be
/// made to fail from the outside — can be exercised directly, by
/// handing it an `encoded` that was never produced from `expected`.
/// A `debug_assert!` would not do here: it compiles out of a release
/// build, exactly where this guarantee has to hold.
fn verify_round_trip(file: &str, encoded: &[u8], expected: &[u8]) -> Result<()> {
    let read_back = ssf1::decode(encoded)?;
    if read_back != expected {
        return Err(Error::UnreadableSaveData(SaveDataDefect::RoundTripFailed {
            file: file.to_string(),
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::savedata::ssf1;

    /// A backup holding one progression file with `level` for the
    /// Bulwark, plus one file the catalogue knows nothing about.
    fn backup_fixture(
        root: &std::path::Path,
        label: &str,
        level: u64,
        stranger: &[u8],
    ) -> BackupEntry {
        let save_dir = root.join(format!("save-{label}"));
        std::fs::create_dir_all(save_dir.join("config")).unwrap();
        let json = format!(
            r#"{{"UserProgression":{{"systemVersion":{level},"UserMastery":{{"masteryStates":{{"PVE_TANK":{{"json_version":3,"currentLevel":{level}}}}}}}}}}}"#
        );
        std::fs::write(
            save_dir.join("config/user_progression.cfg"),
            ssf1::encode(json.as_bytes()),
        )
        .unwrap();
        std::fs::write(save_dir.join("config/unknown_to_us.cfg"), stranger).unwrap();
        crate::saves::backup(&save_dir, &root.join("backups"), Some(label)).unwrap()
    }

    fn level_of(entry: &BackupEntry) -> u64 {
        let files = crate::saves::read_files(entry).unwrap();
        let json = ssf1::decode(&files["config/user_progression.cfg"]).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&json).unwrap();
        value["UserProgression"]["UserMastery"]["masteryStates"]["PVE_TANK"]["currentLevel"]
            .as_u64()
            .unwrap()
    }

    #[test]
    fn a_composition_without_replacements_reproduces_the_base_file_for_file() {
        let temp = tempfile::tempdir().unwrap();
        let base = backup_fixture(temp.path(), "base", 5, b"opaque");

        let composed =
            compose(&base, &[], &temp.path().join("backups"), Some("copy")).unwrap();

        assert_eq!(
            crate::saves::read_files(&base).unwrap(),
            crate::saves::read_files(&composed).unwrap()
        );
    }

    #[test]
    fn a_replaced_part_comes_from_the_source_backup() {
        let temp = tempfile::tempdir().unwrap();
        let base = backup_fixture(temp.path(), "base", 5, b"opaque");
        let other = backup_fixture(temp.path(), "other", 42, b"different");

        let composed = compose(
            &base,
            &[("class_level:PVE_TANK".to_string(), other.clone())],
            &temp.path().join("backups"),
            Some("mixed"),
        )
        .unwrap();

        assert_eq!(level_of(&composed), 42);
    }

    #[test]
    fn a_file_the_catalogue_does_not_know_is_carried_over_from_the_base() {
        let temp = tempfile::tempdir().unwrap();
        let base = backup_fixture(temp.path(), "base", 5, b"opaque");
        let other = backup_fixture(temp.path(), "other", 42, b"different");

        let composed = compose(
            &base,
            &[("class_level:PVE_TANK".to_string(), other)],
            &temp.path().join("backups"),
            Some("mixed"),
        )
        .unwrap();

        let files = crate::saves::read_files(&composed).unwrap();
        assert_eq!(files["config/unknown_to_us.cfg"], b"opaque".to_vec());
    }

    #[test]
    fn the_composed_backup_says_what_it_was_made_of() {
        let temp = tempfile::tempdir().unwrap();
        let base = backup_fixture(temp.path(), "base", 5, b"opaque");
        let other = backup_fixture(temp.path(), "other", 42, b"different");

        let composed = compose(
            &base,
            &[("class_level:PVE_TANK".to_string(), other.clone())],
            &temp.path().join("backups"),
            Some("mixed"),
        )
        .unwrap();

        let recorded = crate::saves::composition_of(&composed).unwrap().unwrap();
        assert_eq!(recorded.base, base.created_at);
        assert_eq!(recorded.parts["class_level:PVE_TANK"], other.created_at);
    }

    #[test]
    fn an_unknown_part_id_is_refused_before_anything_is_written() {
        let temp = tempfile::tempdir().unwrap();
        let base = backup_fixture(temp.path(), "base", 5, b"opaque");
        let other = backup_fixture(temp.path(), "other", 42, b"different");
        let backups = temp.path().join("backups");
        let before = crate::saves::list_backups(&backups).unwrap().len();

        let error =
            compose(&base, &[("no_such_part".to_string(), other)], &backups, None).unwrap_err();

        assert!(matches!(error, Error::UnknownPart { .. }), "got {error:?}");
        assert_eq!(crate::saves::list_backups(&backups).unwrap().len(), before);
    }

    #[test]
    fn a_container_that_does_not_decode_back_to_what_went_in_is_refused() {
        // The codec itself round-trips correctly by construction, so
        // the honest way to reach this branch is to hand the check an
        // `encoded` that was never produced from `expected`, rather
        // than to contort `ssf1::encode` into producing broken bytes.
        let encoded = ssf1::encode(br#"{"a":1}"#);

        let error = verify_round_trip("config/x.cfg", &encoded, br#"{"a":2}"#).unwrap_err();

        assert!(
            matches!(
                error,
                Error::UnreadableSaveData(crate::error::SaveDataDefect::RoundTripFailed { .. })
            ),
            "got {error:?}"
        );
    }
}

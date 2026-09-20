//! Merging one part of a savegame into another.
//!
//! Everything that can refuse lives here, and it refuses before it
//! writes: `apply` reads and checks the source first, so a rejected
//! part leaves the base exactly as it was. That matters because a
//! composition applies many parts in a row and the caller reports the
//! first refusal — a half-applied base would silently become the
//! result.

use crate::error::{Error, Result};
use crate::savedata::catalogue::{system_key, Documents, Part};
use serde_json::Value;

/// The schema version a node carries, if it carries one.
fn schema_version(node: &Value) -> Option<u64> {
    node.get("json_version")?.as_u64()
}

/// Takes one part out of `source` and puts it into `base`.
///
/// The schema versions have to match. `json_version` sits on the node
/// itself, so two backups from different game builds can disagree about
/// one part while agreeing about every other — which is why this is
/// decided per part and not per file.
pub fn apply(base: &mut Documents, part: &Part, source: &Documents) -> Result<()> {
    let source_document = source
        .get(part.file)
        .ok_or_else(|| Error::PartMissingInSource { part: part.id.clone() })?;
    let incoming = part
        .selector
        .get(source_document)
        .ok_or_else(|| Error::PartMissingInSource { part: part.id.clone() })?
        .clone();

    let base_document = base
        .get(part.file)
        .ok_or_else(|| Error::PartMissingInSource { part: part.id.clone() })?;
    let present = part
        .selector
        .get(base_document)
        .ok_or_else(|| Error::PartMissingInSource { part: part.id.clone() })?;

    if schema_version(present) != schema_version(&incoming) {
        return Err(Error::UnmergeablePart {
            part: part.id.clone(),
            base: schema_version(present).map_or_else(|| "-".to_string(), |v| v.to_string()),
            source: schema_version(&incoming).map_or_else(|| "-".to_string(), |v| v.to_string()),
        });
    }

    let base_document = base.get_mut(part.file).expect("checked above");
    if !part.selector.set(base_document, incoming) {
        return Err(Error::PartMissingInSource { part: part.id.clone() });
    }
    Ok(())
}

/// Lifts a file's `systemVersion` to the highest of base and source.
///
/// The counter is the engine's "how new is this" marker — it complains
/// that provided data is older than what it holds. A merged file is at
/// least as new as everything that went into it, so the highest value
/// wins. A file without the field is left alone.
pub fn raise_system_version(base: &mut Documents, file: &str, source: &Documents) {
    let Some(incoming) = source
        .get(file)
        .and_then(|document| {
            let key = system_key(document)?;
            document.get(key)?.get("systemVersion")?.as_u64()
        })
    else {
        return;
    };
    let Some(document) = base.get_mut(file) else {
        return;
    };
    let Some(key) = system_key(document).map(str::to_string) else {
        return;
    };
    let Some(slot) = document.get_mut(&key).and_then(|system| system.get_mut("systemVersion"))
    else {
        return;
    };
    if slot.as_u64().is_some_and(|present| present < incoming) {
        *slot = Value::from(incoming);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::savedata::catalogue::{part_by_id, Documents};
    use serde_json::json;

    fn progression(version: u64, level: u64, schema: u64) -> Documents {
        let mut documents = Documents::new();
        documents.insert(
            "config/user_progression.cfg".to_string(),
            json!({"UserProgression": {"systemVersion": version, "UserMastery": {
                "masteryStates": {
                    "PVE_TANK": {"json_version": schema, "currentLevel": level},
                    "PVE_SOLDIER": {"json_version": schema, "currentLevel": 1}
                }}}}),
        );
        documents
    }

    #[test]
    fn a_part_is_taken_from_the_source_and_nothing_else_is() {
        let mut base = progression(700, 5, 3);
        let source = progression(701, 42, 3);
        let part = part_by_id(&base, "class_level:PVE_TANK").unwrap();

        apply(&mut base, &part, &source).unwrap();

        let states = &base["config/user_progression.cfg"]["UserProgression"]["UserMastery"]["masteryStates"];
        assert_eq!(states["PVE_TANK"]["currentLevel"], json!(42));
        assert_eq!(states["PVE_SOLDIER"]["currentLevel"], json!(1), "the neighbour moved");
    }

    #[test]
    fn a_schema_mismatch_is_refused_instead_of_merged() {
        let mut base = progression(700, 5, 3);
        let source = progression(701, 42, 4);
        let part = part_by_id(&base, "class_level:PVE_TANK").unwrap();

        let error = apply(&mut base, &part, &source).unwrap_err();
        assert!(matches!(error, Error::UnmergeablePart { .. }), "got {error:?}");

        let states = &base["config/user_progression.cfg"]["UserProgression"]["UserMastery"]["masteryStates"];
        assert_eq!(states["PVE_TANK"]["currentLevel"], json!(5), "base was changed anyway");
    }

    #[test]
    fn a_part_the_source_does_not_hold_is_refused() {
        let mut base = progression(700, 5, 3);
        let mut source = progression(701, 42, 3);
        source.remove("config/user_progression.cfg");
        let part = part_by_id(&base, "class_level:PVE_TANK").unwrap();

        assert!(matches!(
            apply(&mut base, &part, &source).unwrap_err(),
            Error::PartMissingInSource { .. }
        ));
    }

    #[test]
    fn the_system_version_becomes_the_highest_of_the_two() {
        let mut base = progression(700, 5, 3);
        let source = progression(890, 42, 3);
        raise_system_version(&mut base, "config/user_progression.cfg", &source);
        assert_eq!(base["config/user_progression.cfg"]["UserProgression"]["systemVersion"], json!(890));
    }

    #[test]
    fn a_lower_system_version_in_the_source_leaves_the_base_alone() {
        let mut base = progression(900, 5, 3);
        let source = progression(700, 42, 3);
        raise_system_version(&mut base, "config/user_progression.cfg", &source);
        assert_eq!(base["config/user_progression.cfg"]["UserProgression"]["systemVersion"], json!(900));
    }
}

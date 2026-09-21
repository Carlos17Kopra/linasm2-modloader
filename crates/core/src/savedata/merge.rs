//! Merging one part of a savegame into another.
//!
//! Everything that can refuse lives here, and it refuses before it
//! writes: `apply` reads and checks the source first, so a rejected
//! part leaves the base exactly as it was. That matters because a
//! composition applies many parts in a row and the caller reports the
//! first refusal — a half-applied base would silently become the
//! result.

use crate::error::{Error, Result};
use crate::savedata::catalogue::{self, Documents, Part, Selector};
use serde_json::Value;

/// The schema version a node carries, if it carries one.
fn schema_version(node: &Value) -> Option<u64> {
    node.get("json_version")?.as_u64()
}

/// How many nodes a part's selector names in `document`.
///
/// Only a list item can name more than one: a member is an object key,
/// and a whole file is the single system object. The catalogue refuses
/// an ambiguous id when it reads the base, but it never sees a source —
/// so this is where the source's side of that rule is kept.
fn named_nodes(selector: &Selector, document: &Value) -> usize {
    let Selector::ListItem { container, key_field, id } = selector else {
        return usize::from(selector.get(document).is_some());
    };
    let Some(elements) = document.pointer(container).and_then(Value::as_array) else {
        return 0;
    };
    elements
        .iter()
        .filter(|element| element.get(key_field).and_then(Value::as_str) == Some(id.as_str()))
        .count()
}

/// Takes one part out of `source` and puts it into `base`.
///
/// This is the whole merge, `systemVersion` included, and deliberately
/// not a pair of calls a caller could order the wrong way round: a
/// whole-file part overwrites the very object that counter sits in, so
/// anything raising it afterwards would already be comparing the
/// source's value with itself.
///
/// The schema versions have to match, and `json_version` sits on the
/// node the selector points at — so two backups from different game
/// builds can disagree about one part while agreeing about every other,
/// which is why this is decided per part and not per file.
///
/// What that check does not cover is the whole-file parts. Their node
/// is the file's system object, and no system object carries a
/// `json_version`: in the saves this was built from, all thirteen
/// catalogued files keep the field on their leaves and the system
/// object holds nothing but `systemVersion` and the payload. Both sides
/// therefore answer `None`, the versions "match", and the part goes
/// through unexamined. Inventing a comparison the data cannot support
/// would be worse than saying so.
///
/// It is tolerable because a whole-file part cannot be half-merged: the
/// file arrives entire from one save, nothing of the base's is left
/// inside it, and it is as internally consistent as it was in the save
/// it came from. What still has to be true of it is that it does not
/// claim to be older than the composition around it — and that is what
/// `systemVersion` below guards.
///
/// The invariant the base-side refusals rest on: `compose` resolves
/// every part through `catalogue::part_by_id` against the base's *own*
/// documents, and `get` and `set` share one walk, so a part that
/// resolved for reading resolves for writing. They are therefore
/// unreachable from the one caller there is. They stay because `apply`
/// takes any part with any two document sets — and because a defect in
/// the base reported against the source sends the reader off to swap
/// backups that were never at fault.
pub(crate) fn apply(base: &mut Documents, part: &Part, source: &Documents) -> Result<()> {
    let source_document = source
        .get(part.file)
        .ok_or_else(|| Error::PartMissingInSource { part: part.id.clone() })?;
    if named_nodes(&part.selector, source_document) > 1 {
        return Err(Error::AmbiguousPartInSource { part: part.id.clone() });
    }
    let incoming = part
        .selector
        .get(source_document)
        .ok_or_else(|| Error::PartMissingInSource { part: part.id.clone() })?
        .clone();

    let base_document = base
        .get(part.file)
        .ok_or_else(|| Error::PartMissingInBase { part: part.id.clone() })?;
    let present = part
        .selector
        .get(base_document)
        .ok_or_else(|| Error::PartMissingInBase { part: part.id.clone() })?;

    if schema_version(present) != schema_version(&incoming) {
        return Err(Error::UnmergeablePart {
            part: part.id.clone(),
            base: schema_version(present).map_or_else(|| "-".to_string(), |v| v.to_string()),
            source: schema_version(&incoming).map_or_else(|| "-".to_string(), |v| v.to_string()),
        });
    }

    // Read before the write, and from both sides: a whole-file part
    // takes the system object with it, counter included, so afterwards
    // there is nothing left to compare the source against.
    let highest = catalogue::system_version(base, part.file).max(catalogue::system_version(source, part.file));

    let base_document = base.get_mut(part.file).expect("checked above");
    if !part.selector.set(base_document, incoming) {
        return Err(Error::PartMissingInBase { part: part.id.clone() });
    }

    if let Some(version) = highest {
        raise_system_version(base, part.file, version);
    }
    Ok(())
}

/// Lifts a file's `systemVersion` to `version`.
///
/// The counter is the engine's "how new is this" marker — it complains
/// that provided data is older than what it holds. A merged file is at
/// least as new as everything that went into it, so the highest value
/// wins. A file without the field is left alone: the field is never
/// created, because a value this code invented would claim an age the
/// save never had.
fn raise_system_version(base: &mut Documents, file: &str, version: u64) {
    let Some(document) = base.get_mut(file) else {
        return;
    };
    let Some(slot) =
        Selector::Whole.get_mut(document).and_then(|system| system.get_mut("systemVersion"))
    else {
        return;
    };
    if slot.as_u64().is_some_and(|present| present < version) {
        *slot = Value::from(version);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::savedata::catalogue::{part_by_id, Documents};
    use serde_json::json;

    /// A backup holding one whole-file group, so that the part under
    /// test replaces the very object the counter sits in.
    fn economy(version: u64, credits: u64) -> Documents {
        let mut documents = Documents::new();
        documents.insert(
            "config/economy.cfg".to_string(),
            json!({"Economy": {"systemVersion": version, "credits": credits}}),
        );
        documents
    }

    /// A backup holding one list-item group, the only kind whose id can
    /// name more than one node.
    fn loadouts(sets: Value) -> Documents {
        let mut documents = Documents::new();
        documents.insert(
            "config/loadouts.cfg".to_string(),
            json!({"Loadouts": {"Sets": {"loadoutSets": sets}}}),
        );
        documents
    }

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

    /// What the schema guard does *not* cover, pinned so that nobody
    /// reads `apply`'s refusal as cover it does not give. A whole-file
    /// part selects the file's system object, which carries no
    /// `json_version` in any save this was built from — both sides
    /// answer `None`, and the part goes through unexamined. The reason
    /// that is acceptable is written down at `apply`.
    #[test]
    fn a_whole_file_part_is_not_examined_by_the_schema_check_at_all() {
        let mut base = economy(900, 5);
        let source = economy(900, 42);
        let part = part_by_id(&base, "economy").unwrap();

        assert_eq!(schema_version(part.selector.get(&base["config/economy.cfg"]).unwrap()), None);
        assert_eq!(schema_version(part.selector.get(&source["config/economy.cfg"]).unwrap()), None);

        apply(&mut base, &part, &source).unwrap();

        assert_eq!(base["config/economy.cfg"]["Economy"]["credits"], json!(42));
    }

    /// A list the source holds twice, where the base holds it once.
    /// The catalogue's own ambiguity guard only ever reads the base, so
    /// without this one the source's first entry is taken by position
    /// and the second silently dropped — a guess, where this module's
    /// rule is to refuse.
    #[test]
    fn a_part_the_source_holds_twice_is_refused() {
        let mut base = loadouts(json!([{"masteryUid": "STORY_TITUS", "slot": 1}]));
        let source = loadouts(json!([
            {"masteryUid": "STORY_TITUS", "slot": 2},
            {"masteryUid": "STORY_TITUS", "slot": 3}
        ]));
        let part = part_by_id(&base, "loadout:STORY_TITUS").unwrap();

        let error = apply(&mut base, &part, &source).unwrap_err();
        assert!(matches!(error, Error::AmbiguousPartInSource { .. }), "got {error:?}");

        let sets = &base["config/loadouts.cfg"]["Loadouts"]["Sets"]["loadoutSets"];
        assert_eq!(sets[0]["slot"], json!(1), "base was changed anyway");
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

    /// The same absence on the other side is a different report. A user
    /// told the *source* is at fault goes on swapping source backups
    /// that were never the problem, so the two sides cannot share a
    /// variant.
    #[test]
    fn a_part_the_base_does_not_hold_blames_the_base_and_not_the_source() {
        let mut base = progression(700, 5, 3);
        let source = progression(701, 42, 3);
        let part = part_by_id(&source, "class_level:PVE_TANK").unwrap();
        base.remove("config/user_progression.cfg");

        assert!(matches!(
            apply(&mut base, &part, &source).unwrap_err(),
            Error::PartMissingInBase { .. }
        ));
    }

    #[test]
    fn a_node_the_base_file_does_not_hold_blames_the_base_as_well() {
        let mut base = progression(700, 5, 3);
        let source = progression(701, 42, 3);
        let part = part_by_id(&source, "class_level:PVE_TANK").unwrap();
        base.get_mut("config/user_progression.cfg").unwrap()["UserProgression"]["UserMastery"]
            ["masteryStates"]
            .as_object_mut()
            .unwrap()
            .remove("PVE_TANK");

        assert!(matches!(
            apply(&mut base, &part, &source).unwrap_err(),
            Error::PartMissingInBase { .. }
        ));
    }

    #[test]
    fn the_system_version_becomes_the_highest_of_the_two() {
        let mut base = progression(700, 5, 3);
        let source = progression(890, 42, 3);
        let part = part_by_id(&base, "class_level:PVE_TANK").unwrap();

        apply(&mut base, &part, &source).unwrap();

        assert_eq!(base["config/user_progression.cfg"]["UserProgression"]["systemVersion"], json!(890));
    }

    #[test]
    fn a_lower_system_version_in_the_source_leaves_the_base_alone() {
        let mut base = progression(900, 5, 3);
        let source = progression(700, 42, 3);
        let part = part_by_id(&base, "class_level:PVE_TANK").unwrap();

        apply(&mut base, &part, &source).unwrap();

        assert_eq!(base["config/user_progression.cfg"]["UserProgression"]["systemVersion"], json!(900));
    }

    /// The direction that was wrong: a whole-file part replaces the very
    /// object the counter sits in, so by the time anything could compare
    /// the two values the base's own is already gone. Reading it before
    /// the write is the only order that can answer this.
    #[test]
    fn a_whole_file_part_does_not_lower_the_base_system_version() {
        let mut base = economy(900, 5);
        let source = economy(700, 42);
        let part = part_by_id(&base, "economy").unwrap();

        apply(&mut base, &part, &source).unwrap();

        let file = &base["config/economy.cfg"]["Economy"];
        assert_eq!(file["systemVersion"], json!(900));
        assert_eq!(file["credits"], json!(42), "the payload still comes from the source");
    }

    #[test]
    fn a_whole_file_part_takes_the_higher_system_version_of_the_source() {
        let mut base = economy(700, 5);
        let source = economy(900, 42);
        let part = part_by_id(&base, "economy").unwrap();

        apply(&mut base, &part, &source).unwrap();

        assert_eq!(base["config/economy.cfg"]["Economy"]["systemVersion"], json!(900));
    }
}

//! One short figure per part, so an id means something to a reader.
//!
//! The rule is a field of the part's own node, named per group. A field
//! that is not there yields nothing at all rather than a zero: a backup
//! from an older build may simply not record it, and "level 0" would be
//! a lie about the save.

use crate::savedata::catalogue::{Documents, Part};

/// A group's summary rule: which field it sums up, and how the value is
/// worded.
type Rule = (&'static str, fn(u64) -> String);

/// Which field sums a group's parts up, and how it is worded.
fn rule(group: &str) -> Option<Rule> {
    match group {
        "class_level" => Some(("currentLevel", |value| crate::t!("savedata.summary.level", level = value))),
        "weapon" => {
            Some(("masteryPoints", |value| crate::t!("savedata.summary.mastery_points", points = value)))
        }
        "heraldry" => {
            Some(("victoriesCount", |value| crate::t!("savedata.summary.victories", count = value)))
        }
        _ => None,
    }
}

/// A short figure for this part, if there is one worth showing.
pub fn summarize(part: &Part, documents: &Documents) -> Option<String> {
    let (field, word) = rule(part.group)?;
    let node = part.selector.get(documents.get(part.file)?)?;
    Some(word(node.get(field)?.as_u64()?))
}

/// The name of a group, for a list a person reads.
pub fn group_name(group: &str) -> String {
    match group {
        "class_level" => crate::t!("savedata.group.class_level"),
        "armour_loyalist" => crate::t!("savedata.group.armour_loyalist"),
        "armour_chaos" => crate::t!("savedata.group.armour_chaos"),
        "weapon" => crate::t!("savedata.group.weapon"),
        "heraldry" => crate::t!("savedata.group.heraldry"),
        "loadout" => crate::t!("savedata.group.loadout"),
        "challenges" => crate::t!("savedata.group.challenges"),
        "story" => crate::t!("savedata.group.story"),
        "economy" => crate::t!("savedata.group.economy"),
        "pve_state" => crate::t!("savedata.group.pve_state"),
        "horde_mode" => crate::t!("savedata.group.horde_mode"),
        "tutorial" => crate::t!("savedata.group.tutorial"),
        "mutators" => crate::t!("savedata.group.mutators"),
        "achievements" => crate::t!("savedata.group.achievements"),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::savedata::catalogue::{part_by_id, Documents};
    use serde_json::json;

    fn documents_fixture() -> Documents {
        let mut documents = Documents::new();
        documents.insert(
            "config/user_progression.cfg".to_string(),
            json!({"UserProgression": {"systemVersion": 767, "UserMastery": {"masteryStates": {
                "PVE_TANK": {"json_version": 3, "currentLevel": 21}
            }}}}),
        );
        documents.insert(
            "config/weapon_progression.cfg".to_string(),
            json!({"WeaponProgression": {"systemVersion": 758, "WeaponMastery": {"weaponStates": {
                "hgun_volkite_pistol": {"json_version": 2, "masteryPoints": 4}
            }}}}),
        );
        documents
    }

    /// `group_name` falls back to the raw id, so a group added to
    /// `GROUPS` without an arm here prints `pve_state` where "Operations"
    /// belongs, and nothing else notices: the i18n test proves that every
    /// key used in the sources exists, never that every group uses one.
    /// This is that missing half — the same decision
    /// `requires_exclusive_access` makes impossible to forget by refusing
    /// to compile.
    #[test]
    fn every_group_in_the_table_has_a_name_of_its_own() {
        for group in crate::savedata::catalogue::GROUPS {
            let key = std::format!("savedata.group.{}", group.id);
            assert!(crate::i18n::has_key(&key), "no catalogue entry {key}");
            assert_ne!(
                group_name(group.id),
                group.id,
                "group_name has no arm for '{}' and prints its id",
                group.id
            );
        }
    }

    #[test]
    fn a_class_part_is_summed_up_by_its_level() {
        let _guard = crate::i18n::language_test_lock();
        crate::i18n::set_language(crate::i18n::Language::English);
        let documents = documents_fixture();
        let part = part_by_id(&documents, "class_level:PVE_TANK").unwrap();
        assert_eq!(summarize(&part, &documents).unwrap(), "level 21");
    }

    #[test]
    fn a_weapon_part_is_summed_up_by_its_mastery_points() {
        let _guard = crate::i18n::language_test_lock();
        crate::i18n::set_language(crate::i18n::Language::English);
        let documents = documents_fixture();
        let part = part_by_id(&documents, "weapon:hgun_volkite_pistol").unwrap();
        assert_eq!(summarize(&part, &documents).unwrap(), "4 mastery points");
    }

    #[test]
    fn a_part_whose_field_is_absent_is_summed_up_as_nothing_rather_than_zero() {
        let _guard = crate::i18n::language_test_lock();
        crate::i18n::set_language(crate::i18n::Language::English);
        let mut documents = documents_fixture();
        documents.get_mut("config/user_progression.cfg").unwrap()["UserProgression"]["UserMastery"]
            ["masteryStates"]["PVE_TANK"]
            .as_object_mut()
            .unwrap()
            .remove("currentLevel");
        let part = part_by_id(&documents, "class_level:PVE_TANK").unwrap();
        assert_eq!(summarize(&part, &documents), None);
    }
}

//! What a part is, and which parts a backup offers.
//!
//! The groups below are a static table: which file a kind of data lives
//! in, where inside it, and what identifies one entry. The ids
//! themselves — 45 weapons, 29 classes — are read out of the backup
//! being looked at, so a game update that adds one needs no code change
//! here, and a backup from an older build simply offers fewer parts.
//!
//! The one rule this table has to keep is that no part contains
//! another. A file either has a whole-file part or finer ones, never
//! both, and two finer parts never select the same node. Without that
//! rule two selections could contradict each other and the result would
//! depend on the order they were applied in.

use crate::error::{Error, Result, SaveDataDefect};
use crate::savedata::ssf1;
use serde_json::Value;
use std::collections::BTreeMap;

/// The decoded savegame files of one backup, keyed by the path relative
/// to the save directory.
pub type Documents = BTreeMap<String, Value>;

/// Where a part's data sits inside one file.
#[derive(Debug, Clone, PartialEq)]
pub enum Selector {
    /// The file's single system object — everything below the root.
    Whole,
    /// A member of the object at `container`, by its key.
    Member { container: &'static str, key: String },
    /// The element of the array at `container` whose `key_field` is `id`.
    ListItem { container: &'static str, key_field: &'static str, id: String },
}

/// How the parts of one file are found.
#[derive(Debug, Clone, Copy)]
pub enum GroupKind {
    /// The file is one part.
    WholeFile,
    /// Every member of the object at this pointer is a part.
    Members { container: &'static str },
    /// Every element of the array at this pointer is a part, identified
    /// by the named field.
    ListItems { container: &'static str, key_field: &'static str },
}

/// A kind of data the user can take from another backup.
#[derive(Debug, Clone, Copy)]
pub struct Group {
    pub id: &'static str,
    pub file: &'static str,
    pub kind: GroupKind,
}

/// One selectable piece of a backup.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub id: String,
    pub group: &'static str,
    pub file: &'static str,
    pub selector: Selector,
}

/// Everything a composition can move between backups.
///
/// Settings, agreements, achievements bookkeeping and the platform's
/// own record are deliberately absent: they are not progression, and
/// mixing them between saves buys nothing.
pub const GROUPS: &[Group] = &[
    Group {
        id: "class_level",
        file: "config/user_progression.cfg",
        kind: GroupKind::Members { container: "/UserProgression/UserMastery/masteryStates" },
    },
    Group {
        id: "armour_loyalist",
        file: "config/character_customization_progression.cfg",
        kind: GroupKind::Members {
            container: "/CharacterCustomizationProgression/CharacterCustomization/teamStates/LOYALIST/outfitStates",
        },
    },
    Group {
        id: "armour_chaos",
        file: "config/character_customization_progression.cfg",
        kind: GroupKind::Members {
            container: "/CharacterCustomizationProgression/CharacterCustomization/teamStates/CHAOS/outfitStates",
        },
    },
    Group {
        id: "weapon",
        file: "config/weapon_progression.cfg",
        kind: GroupKind::Members { container: "/WeaponProgression/WeaponMastery/weaponStates" },
    },
    Group {
        id: "heraldry",
        file: "config/heraldry_progression.cfg",
        kind: GroupKind::Members { container: "/HeraldryProgression/Armor/characterArmorInfos" },
    },
    Group {
        id: "loadout",
        file: "config/loadouts.cfg",
        kind: GroupKind::ListItems {
            container: "/Loadouts/Sets/loadoutSets",
            key_field: "masteryUid",
        },
    },
    Group { id: "challenges", file: "config/challenge_progression.cfg", kind: GroupKind::WholeFile },
    Group { id: "story", file: "config/story_progression.cfg", kind: GroupKind::WholeFile },
    Group { id: "economy", file: "config/economy.cfg", kind: GroupKind::WholeFile },
    Group { id: "pve_state", file: "config/pve_state.cfg", kind: GroupKind::WholeFile },
    Group { id: "horde_mode", file: "config/hordemode_state.cfg", kind: GroupKind::WholeFile },
    Group { id: "tutorial", file: "config/tutorial.cfg", kind: GroupKind::WholeFile },
    Group { id: "mutators", file: "config/mutator_challenges.cfg", kind: GroupKind::WholeFile },
    Group { id: "achievements", file: "config/achievements.cfg", kind: GroupKind::WholeFile },
];

/// The key of a document's single system object, e.g. `UserProgression`.
pub fn system_key(document: &Value) -> Option<&str> {
    let object = document.as_object()?;
    if object.len() != 1 {
        return None;
    }
    object.keys().next().map(String::as_str)
}

impl Selector {
    /// The value this selector points at, or `None` when it is absent —
    /// which is the normal answer for a backup from an older build, not
    /// an error.
    pub fn get<'a>(&self, document: &'a Value) -> Option<&'a Value> {
        match self {
            Selector::Whole => document.get(system_key(document)?),
            Selector::Member { container, key } => document.pointer(container)?.get(key),
            Selector::ListItem { container, key_field, id } => document
                .pointer(container)?
                .as_array()?
                .iter()
                .find(|element| element.get(key_field).and_then(Value::as_str) == Some(id)),
        }
    }

    /// Replaces that value. `false` means the target is not there and
    /// nothing was written — no node is ever created.
    pub fn set(&self, document: &mut Value, value: Value) -> bool {
        match self {
            Selector::Whole => {
                let Some(key) = system_key(document).map(str::to_string) else {
                    return false;
                };
                match document.get_mut(&key) {
                    Some(slot) => {
                        *slot = value;
                        true
                    }
                    None => false,
                }
            }
            Selector::Member { container, key } => {
                match document.pointer_mut(container).and_then(|node| node.get_mut(key)) {
                    Some(slot) => {
                        *slot = value;
                        true
                    }
                    None => false,
                }
            }
            Selector::ListItem { container, key_field, id } => {
                let Some(array) = document.pointer_mut(container).and_then(Value::as_array_mut)
                else {
                    return false;
                };
                match array
                    .iter_mut()
                    .find(|element| element.get(key_field).and_then(Value::as_str) == Some(id))
                {
                    Some(slot) => {
                        *slot = value;
                        true
                    }
                    None => false,
                }
            }
        }
    }
}

/// Decodes every savegame file of a backup that the catalogue knows.
///
/// Files outside the catalogue are left as bytes: a composition copies
/// them through untouched and never needs to understand them.
pub fn documents(files: &BTreeMap<String, Vec<u8>>) -> Result<Documents> {
    let mut documents = Documents::new();
    for group in GROUPS {
        if documents.contains_key(group.file) {
            continue;
        }
        let Some(bytes) = files.get(group.file) else {
            continue;
        };
        let json = ssf1::decode(bytes)?;
        let value: Value = serde_json::from_slice(&json)
            .map_err(|_| Error::UnreadableSaveData(SaveDataDefect::NotJson))?;
        documents.insert(group.file.to_string(), value);
    }
    Ok(documents)
}

/// Every part these documents offer, in the order of `GROUPS`.
pub fn parts(documents: &Documents) -> Vec<Part> {
    let mut parts = Vec::new();
    for group in GROUPS {
        let Some(document) = documents.get(group.file) else {
            continue;
        };
        match group.kind {
            GroupKind::WholeFile => parts.push(Part {
                id: group.id.to_string(),
                group: group.id,
                file: group.file,
                selector: Selector::Whole,
            }),
            GroupKind::Members { container } => {
                let Some(object) = document.pointer(container).and_then(Value::as_object) else {
                    continue;
                };
                for key in object.keys() {
                    parts.push(Part {
                        id: format!("{}:{key}", group.id),
                        group: group.id,
                        file: group.file,
                        selector: Selector::Member { container, key: key.clone() },
                    });
                }
            }
            GroupKind::ListItems { container, key_field } => {
                let Some(array) = document.pointer(container).and_then(Value::as_array) else {
                    continue;
                };
                for element in array {
                    let Some(id) = element.get(key_field).and_then(Value::as_str) else {
                        continue;
                    };
                    parts.push(Part {
                        id: format!("{}:{id}", group.id),
                        group: group.id,
                        file: group.file,
                        selector: Selector::ListItem {
                            container,
                            key_field,
                            id: id.to_string(),
                        },
                    });
                }
            }
        }
    }
    parts
}

/// The part with this id, if these documents offer it.
pub fn part_by_id(documents: &Documents, id: &str) -> Option<Part> {
    parts(documents).into_iter().find(|part| part.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn documents_fixture() -> Documents {
        let mut documents = Documents::new();
        documents.insert(
            "config/user_progression.cfg".to_string(),
            json!({"UserProgression": {"systemVersion": 767, "UserMastery": {
                "masteryStates": {
                    "PVE_TANK": {"json_version": 3, "currentLevel": 5},
                    "PVE_SOLDIER": {"json_version": 3, "currentLevel": 19}
                }}}}),
        );
        documents.insert(
            "config/loadouts.cfg".to_string(),
            json!({"Loadouts": {"systemVersion": 1277, "Sets": {"loadoutSets": [
                {"json_version": 2, "masteryUid": "STORY_TITUS"},
                {"json_version": 2, "masteryUid": "STORY_GADRIEL"}
            ]}}}),
        );
        documents.insert(
            "config/economy.cfg".to_string(),
            json!({"Economy": {"systemVersion": 700, "States": {}}}),
        );
        documents
    }

    #[test]
    fn the_catalogue_offers_one_part_per_id_found_in_the_data() {
        let parts = parts(&documents_fixture());
        let ids: Vec<&str> = parts.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"class_level:PVE_TANK"));
        assert!(ids.contains(&"class_level:PVE_SOLDIER"));
        assert!(ids.contains(&"loadout:STORY_TITUS"));
        assert!(ids.contains(&"economy"));
    }

    #[test]
    fn a_file_that_is_absent_offers_no_parts_instead_of_failing() {
        let mut documents = documents_fixture();
        documents.remove("config/user_progression.cfg");
        let parts = parts(&documents);
        assert!(!parts.iter().any(|p| p.group == "class_level"));
        assert!(parts.iter().any(|p| p.id == "economy"));
    }

    #[test]
    fn no_part_contains_another() {
        // The rule the whole feature rests on: two selections can never
        // contradict each other, because no two parts overlap. Parts of
        // the same file must therefore differ in their selector, and a
        // whole-file part must be the only part of its file.
        let documents = documents_fixture();
        let parts = parts(&documents);
        for (i, a) in parts.iter().enumerate() {
            for b in parts.iter().skip(i + 1) {
                if a.file != b.file {
                    continue;
                }
                assert!(
                    !matches!(a.selector, Selector::Whole) && !matches!(b.selector, Selector::Whole),
                    "{} and {} share a file and one takes all of it",
                    a.id,
                    b.id
                );
                assert_ne!(a.selector, b.selector, "{} and {} select the same node", a.id, b.id);
            }
        }
    }

    #[test]
    fn no_group_in_the_table_can_ever_contradict_another() {
        // Same rule as `no_part_contains_another`, but over the real
        // catalogue rather than the fixture: a fixture-only test would
        // leave `GROUPS` itself unchecked, and this is the table every
        // backup on disk is read through.
        for (i, a) in GROUPS.iter().enumerate() {
            for b in GROUPS.iter().skip(i + 1) {
                if a.file != b.file {
                    continue;
                }
                assert!(
                    !matches!(a.kind, GroupKind::WholeFile) && !matches!(b.kind, GroupKind::WholeFile),
                    "{} and {} share a file and one takes all of it",
                    a.id,
                    b.id
                );
                let container = |kind: &GroupKind| match kind {
                    GroupKind::WholeFile => None,
                    GroupKind::Members { container } => Some(*container),
                    GroupKind::ListItems { container, .. } => Some(*container),
                };
                assert_ne!(
                    container(&a.kind),
                    container(&b.kind),
                    "{} and {} select the same node",
                    a.id,
                    b.id
                );
            }
        }
    }

    #[test]
    fn a_member_selector_reads_and_replaces_exactly_its_node() {
        let documents = documents_fixture();
        let part = part_by_id(&documents, "class_level:PVE_TANK").unwrap();
        let mut document = documents["config/user_progression.cfg"].clone();

        assert_eq!(part.selector.get(&document).unwrap()["currentLevel"], json!(5));
        assert!(part.selector.set(&mut document, json!({"json_version": 3, "currentLevel": 42})));

        let states = &document["UserProgression"]["UserMastery"]["masteryStates"];
        assert_eq!(states["PVE_TANK"]["currentLevel"], json!(42));
        assert_eq!(states["PVE_SOLDIER"]["currentLevel"], json!(19), "the neighbour moved");
    }

    #[test]
    fn a_list_selector_finds_its_element_by_key_not_by_position() {
        let documents = documents_fixture();
        let part = part_by_id(&documents, "loadout:STORY_GADRIEL").unwrap();
        // The same set, but stored first instead of second.
        let mut document = json!({"Loadouts": {"systemVersion": 1, "Sets": {"loadoutSets": [
            {"json_version": 2, "masteryUid": "STORY_GADRIEL"},
            {"json_version": 2, "masteryUid": "STORY_TITUS"}
        ]}}});
        assert!(part.selector.set(&mut document, json!({"json_version": 2, "masteryUid": "STORY_GADRIEL", "marked": true})));

        let sets = document["Loadouts"]["Sets"]["loadoutSets"].as_array().unwrap();
        assert_eq!(sets[0]["marked"], json!(true));
        assert_eq!(sets[1]["masteryUid"], json!("STORY_TITUS"), "the neighbour moved");
    }

    #[test]
    fn a_whole_file_selector_reads_and_replaces_the_system_object() {
        let documents = documents_fixture();
        let part = part_by_id(&documents, "economy").unwrap();
        let mut document = documents["config/economy.cfg"].clone();

        assert_eq!(part.selector.get(&document).unwrap()["systemVersion"], json!(700));
        assert!(part.selector.set(&mut document, json!({"systemVersion": 701, "States": {}})));
        assert_eq!(document["Economy"]["systemVersion"], json!(701));
    }

    #[test]
    fn a_selector_whose_node_is_absent_reports_it_rather_than_inventing_one() {
        let documents = documents_fixture();
        let part = part_by_id(&documents, "class_level:PVE_TANK").unwrap();
        let mut document = json!({"UserProgression": {"UserMastery": {"masteryStates": {}}}});
        assert!(part.selector.get(&document).is_none());
        assert!(!part.selector.set(&mut document, json!({})));
    }
}

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
        // The save counts class levels from zero, the game shows them
        // from one: a class nobody has played stores 0 and reads as
        // level 1 on the screen, and the cap of 25 is stored as 24.
        // Reporting the stored number would have someone compare a
        // backup against a figure the game never showed them.
        // `saturating_add` because the number comes off a disk.
        "class_level" => Some(("currentLevel", |value: u64| {
            crate::t!("savedata.summary.level", level = value.saturating_add(1))
        })),
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

/// The mode an id carries in front of it, and how it reads behind the
/// name. Upper case throughout, which is what keeps the campaign's
/// weapons (`hwpn_story_heavy_bolter`) out of it.
const MODES: &[(&str, &str)] = &[("PVE_", "PvE"), ("PVP_", "PvP"), ("STORY_", "Story")];

/// The categories a weapon id carries in front of its name.
///
/// Only these are dropped. A first word that is not in this list stays
/// where it is: a category the game adds in a later build would
/// otherwise be swallowed, and an id that loses a word reads as a
/// different weapon — worse than reading awkwardly.
const WEAPON_CATEGORIES: &[&str] =
    &["arifle", "brifle", "equipment", "hgun", "hwpn", "melee", "pc", "pwpn", "shotgun", "smg"];

/// A part's name, for a list a person reads.
///
/// The game's own words, taken apart and tidied — never replaced. A
/// table of invented names would have to claim which class
/// `CHARACTER_MOD_2` is, and a wrong claim beside a level sends someone
/// to the wrong backup; the id says less and never lies. That is also
/// why nothing here is a catalogue entry: this is data out of the save,
/// like the id below it, and the mode in brackets reads the same in
/// every language.
pub fn display_name(part: &Part) -> String {
    // The eight whole-file groups carry their group's id as their own.
    // Its translated name is already the right wording, and "Economy"
    // would throw that translation away.
    if part.id == part.group {
        return group_name(part.group);
    }

    let bare = part.id.split_once(':').map_or(part.id.as_str(), |(_, rest)| rest);
    let (mut bare, mode) = MODES
        .iter()
        .find_map(|(prefix, mode)| bare.strip_prefix(prefix).map(|rest| (rest, Some(*mode))))
        .unwrap_or((bare, None));
    // Two ids carry two categories (`melee_pc_helbrute_hammer`).
    while let Some((head, rest)) = bare.split_once('_') {
        if !WEAPON_CATEGORIES.contains(&head) {
            break;
        }
        bare = rest;
    }

    let mut name = capitalised(bare);
    if let Some(mode) = mode {
        name.push_str(" (");
        name.push_str(mode);
        name.push(')');
    }
    name
}

/// `thunder_hammer` and `CHARACTER_MOD_1` both become `Thunder Hammer`
/// and `Character Mod 1`.
fn capitalised(bare: &str) -> String {
    bare.split('_')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut letters = word.chars();
            match letters.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &letters.as_str().to_lowercase()
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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

    /// The save counts class levels from zero and the game shows them
    /// from one, so a stored 21 is level 22 on the screen. Reporting the
    /// stored number would have someone compare a backup against a
    /// figure the game never showed them.
    #[test]
    fn a_class_part_is_summed_up_by_the_level_the_game_shows() {
        let _guard = crate::i18n::language_test_lock();
        crate::i18n::set_language(crate::i18n::Language::English);
        let documents = documents_fixture();
        let part = part_by_id(&documents, "class_level:PVE_TANK").unwrap();
        assert_eq!(summarize(&part, &documents).unwrap(), "level 22");
    }

    /// The common case, and the one that would look wrong first: a class
    /// nobody has played is level 1 in the game, not level 0.
    #[test]
    fn a_class_that_was_never_played_is_level_one() {
        let _guard = crate::i18n::language_test_lock();
        crate::i18n::set_language(crate::i18n::Language::English);
        let mut documents = documents_fixture();
        documents.get_mut("config/user_progression.cfg").unwrap()["UserProgression"]["UserMastery"]
            ["masteryStates"]["PVE_TANK"]["currentLevel"] = json!(0);
        let part = part_by_id(&documents, "class_level:PVE_TANK").unwrap();
        assert_eq!(summarize(&part, &documents).unwrap(), "level 1");
    }

    /// Mastery points and victories are counts, not levels — nothing is
    /// added to those.
    #[test]
    fn a_count_is_reported_as_it_stands() {
        let _guard = crate::i18n::language_test_lock();
        crate::i18n::set_language(crate::i18n::Language::English);
        let documents = documents_fixture();
        let part = part_by_id(&documents, "weapon:hgun_volkite_pistol").unwrap();
        assert_eq!(summarize(&part, &documents).unwrap(), "4 mastery points");
    }

    #[test]
    fn a_weapon_part_is_summed_up_by_its_mastery_points() {
        let _guard = crate::i18n::language_test_lock();
        crate::i18n::set_language(crate::i18n::Language::English);
        let documents = documents_fixture();
        let part = part_by_id(&documents, "weapon:hgun_volkite_pistol").unwrap();
        assert_eq!(summarize(&part, &documents).unwrap(), "4 mastery points");
    }

    /// A part with nothing in it but the two fields a name is made of.
    fn named(id: &str, group: &'static str) -> Part {
        Part {
            id: id.to_string(),
            group,
            file: "config/user_progression.cfg",
            selector: crate::savedata::catalogue::Selector::Whole,
        }
    }

    #[test]
    fn a_class_part_reads_as_its_class_with_the_mode_behind_it() {
        assert_eq!(display_name(&named("class_level:PVE_TANK", "class_level")), "Tank (PvE)");
        assert_eq!(display_name(&named("class_level:PVP_SNIPER", "class_level")), "Sniper (PvP)");
        assert_eq!(display_name(&named("loadout:STORY_GADRIEL", "loadout")), "Gadriel (Story)");
    }

    #[test]
    fn a_weapon_loses_the_category_its_id_carries() {
        assert_eq!(
            display_name(&named("weapon:melee_thunder_hammer", "weapon")),
            "Thunder Hammer"
        );
        assert_eq!(display_name(&named("weapon:hgun_volkite_pistol", "weapon")), "Volkite Pistol");
    }

    /// Two of the forty-five weapon ids carry two categories, and one
    /// carries a word that only looks like one: `hwpn_story_…` is the
    /// campaign's version of a weapon, not a category, and has to
    /// survive.
    #[test]
    fn a_weapon_loses_every_category_in_front_of_its_name() {
        assert_eq!(
            display_name(&named("weapon:melee_pc_helbrute_hammer", "weapon")),
            "Helbrute Hammer"
        );
        assert_eq!(
            display_name(&named("weapon:pc_helbrute_plasma_cannon", "weapon")),
            "Helbrute Plasma Cannon"
        );
        assert_eq!(
            display_name(&named("weapon:hwpn_story_heavy_bolter", "weapon")),
            "Story Heavy Bolter"
        );
    }

    /// A category the game adds in a later build must not be mistaken
    /// for a known one and swallowed: an id that loses its first word
    /// reads as a different weapon, which is worse than reading
    /// awkwardly.
    #[test]
    fn an_unknown_category_stays_in_the_name() {
        assert_eq!(display_name(&named("weapon:xyz_new_gun", "weapon")), "Xyz New Gun");
    }

    #[test]
    fn an_id_with_neither_mode_nor_category_is_only_tidied_up() {
        assert_eq!(display_name(&named("heraldry:TANK", "heraldry")), "Tank");
        assert_eq!(
            display_name(&named("class_level:PVE_CHARACTER_MOD_1", "class_level")),
            "Character Mod 1 (PvE)"
        );
    }

    /// The eight groups that are a single part carry the group's id as
    /// their own. Their translated group name is already the right
    /// wording, and "Economy" would throw a translation away.
    #[test]
    fn a_part_that_is_its_whole_group_keeps_the_translated_group_name() {
        let _guard = crate::i18n::language_test_lock();
        crate::i18n::set_language(crate::i18n::Language::German);
        assert_eq!(display_name(&named("economy", "economy")), group_name("economy"));
        assert_ne!(display_name(&named("economy", "economy")), "Economy");
        crate::i18n::set_language(crate::i18n::Language::English);
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


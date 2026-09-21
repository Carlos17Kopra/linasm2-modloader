//! The state behind the "compose a save" tab.
//!
//! Everything here can be decided without a screen — which rows are
//! visible, which parts are replaced, what the footer says — and is
//! tested that way. `compose_page` draws it.

use sm2_core::savedata::catalogue::{Documents, Part};
use sm2_core::saves::BackupEntry;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc;
use std::sync::Arc;

/// Which half of the savegame page is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum SavesTab {
    #[default]
    Backups,
    Compose,
}

/// Which dropdown is open, and what a pick in it means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Picker {
    /// The base backup.
    Base,
    /// The source of one part.
    Part(String),
    /// The source of every part of one group at once.
    Group(&'static str),
}

#[derive(Default)]
pub(super) struct ComposeUi {
    /// `created_at` of the base backup.
    pub(super) base: Option<String>,
    /// Part id -> `created_at` of the backup it comes from. This is
    /// `compose::compose`'s `replacements` argument in the form the
    /// interface keeps it, so there is no translation step in between.
    pub(super) sources: BTreeMap<String, String>,
    /// `created_at` -> the decoded files of that backup. `Arc` because
    /// a redraw hands them out and must not copy 3.6 MB of JSON per
    /// frame.
    pub(super) decoded: BTreeMap<String, Arc<Documents>>,
    /// Which backup is being read right now, and the channel it will
    /// arrive on.
    pub(super) loading: Option<String>,
    pub(super) incoming: Option<mpsc::Receiver<Loaded>>,
    /// Backups whose read failed, so the next frame does not ask for
    /// them again — that would be a loop hammering a damaged archive.
    pub(super) failed: BTreeSet<String>,
    /// The parts the base offers, from `catalogue::parts`. Filled when
    /// the base has been read, cleared when the base changes.
    pub(super) parts: Vec<Part>,
    pub(super) open_groups: BTreeSet<&'static str>,
    pub(super) part_filter: String,
    pub(super) only_replaced: bool,
    pub(super) picker: Option<Picker>,
    pub(super) picker_filter: String,
    pub(super) label: String,
    /// The last reading failure, shown above the table until the user
    /// acts. Not a toast: it belongs to a decision being made right
    /// now and must not fade after four seconds.
    pub(super) error: Option<String>,
}

/// What the reading thread sends back.
pub(super) struct Loaded {
    pub(super) created_at: String,
    pub(super) result: Result<Documents, String>,
}

/// One line of the part table.
pub(super) enum Row {
    Group(GroupRow),
    Part(PartRow),
}

/// The head of a group that holds more than one part.
pub(super) struct GroupRow {
    pub(super) group: &'static str,
    pub(super) total: usize,
    pub(super) replaced: usize,
    pub(super) source: GroupSource,
    pub(super) open: bool,
}

/// What a group header's dropdown reads.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum GroupSource {
    /// Every part comes from the base.
    Base,
    /// Every part comes from this one backup.
    One(String),
    /// The parts come from more than one backup.
    Mixed,
}

pub(super) struct PartRow {
    pub(super) id: String,
    pub(super) group: &'static str,
    pub(super) file: &'static str,
    /// The backup this part comes from, or `None` for the base.
    pub(super) source: Option<String>,
    /// Drawn below a group header rather than as a group of its own.
    pub(super) indented: bool,
}

impl ComposeUi {
    /// Sets the base and, when it actually changes, drops every chosen
    /// source with it — see the test for why.
    pub(super) fn set_base(&mut self, created_at: String) {
        if self.base.as_deref() == Some(created_at.as_str()) {
            return;
        }
        self.base = Some(created_at);
        self.sources.clear();
        self.parts.clear();
        self.picker = None;
        self.picker_filter.clear();
    }

    /// The chosen sources as `compose::compose` wants them.
    ///
    /// A timestamp that no longer names a backup is dropped rather than
    /// carried into the core: between opening the tab and pressing the
    /// button, another window may have deleted it.
    pub(super) fn replacements(&self, backups: &[BackupEntry]) -> Vec<(String, BackupEntry)> {
        self.sources
            .iter()
            .filter_map(|(part, created_at)| {
                let entry = backups.iter().find(|b| &b.created_at == created_at)?;
                Some((part.clone(), entry.clone()))
            })
            .collect()
    }

    /// Is the table currently narrowed down?
    ///
    /// While it is, a group with a surviving part is drawn open whatever
    /// `open_groups` says — and `open_groups` is left alone, so clearing
    /// the filter puts the table back the way the user left it.
    fn narrowed(&self) -> bool {
        !self.part_filter.trim().is_empty() || self.only_replaced
    }

    fn shown(&self, part: &Part) -> bool {
        let needle = self.part_filter.trim().to_lowercase();
        let matches = needle.is_empty() || part.id.to_lowercase().contains(&needle);
        matches && (!self.only_replaced || self.sources.contains_key(&part.id))
    }

    /// The table, in the catalogue's own order: `parts` comes out of
    /// `catalogue::parts`, which walks `GROUPS`, so grouping by runs of
    /// equal `group` keeps the order the rest of the program uses.
    pub(super) fn rows(&self, parts: &[Part]) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut index = 0;
        while index < parts.len() {
            let group = parts[index].group;
            let end = parts[index..].iter().take_while(|p| p.group == group).count() + index;
            let members = &parts[index..end];
            index = end;

            let visible: Vec<&Part> = members.iter().filter(|part| self.shown(part)).collect();
            if visible.is_empty() {
                continue;
            }

            // One part is one row. A collapsible section holding a
            // single line is noise.
            if members.len() == 1 {
                rows.push(Row::Part(self.part_row(&members[0], false)));
                continue;
            }

            let open = self.open_groups.contains(group) || self.narrowed();
            rows.push(Row::Group(GroupRow {
                group,
                total: members.len(),
                replaced: members.iter().filter(|p| self.sources.contains_key(&p.id)).count(),
                source: self.group_source(members),
                open,
            }));
            if open {
                rows.extend(visible.into_iter().map(|part| Row::Part(self.part_row(part, true))));
            }
        }
        rows
    }

    fn part_row(&self, part: &Part, indented: bool) -> PartRow {
        PartRow {
            id: part.id.clone(),
            group: part.group,
            file: part.file,
            source: self.sources.get(&part.id).cloned(),
            indented,
        }
    }

    fn group_source(&self, members: &[Part]) -> GroupSource {
        let mut seen: BTreeSet<Option<&String>> = BTreeSet::new();
        for part in members {
            seen.insert(self.sources.get(&part.id));
        }
        match seen.len() {
            1 => match seen.into_iter().next().flatten() {
                Some(created_at) => GroupSource::One(created_at.clone()),
                None => GroupSource::Base,
            },
            _ => GroupSource::Mixed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sm2_core::saves::BackupEntry;
    use std::path::PathBuf;

    /// A `BackupEntry` with nothing in it but the timestamp the
    /// interface keys on — enough for every decision made here, none of
    /// which opens an archive.
    fn entry(created_at: &str) -> BackupEntry {
        BackupEntry {
            created_at: created_at.to_string(),
            label: None,
            archive: PathBuf::from(format!("/nowhere/{created_at}.zip")),
            manifest: PathBuf::from(format!("/nowhere/{created_at}.json")),
        }
    }

    #[test]
    fn without_a_single_chosen_source_there_is_nothing_to_replace() {
        let mut ui = ComposeUi::default();
        ui.set_base("2026-09-20_100000".to_string());

        assert!(ui.replacements(&[entry("2026-09-20_100000")]).is_empty());
    }

    #[test]
    fn replacements_pair_each_part_with_the_backup_it_comes_from() {
        let backups = [entry("2026-09-20_100000"), entry("2026-09-19_080000")];
        let mut ui = ComposeUi::default();
        ui.set_base("2026-09-20_100000".to_string());
        ui.sources.insert(
            "class_level:PVE_TANK".to_string(),
            "2026-09-19_080000".to_string(),
        );

        let replacements = ui.replacements(&backups);

        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].0, "class_level:PVE_TANK");
        assert_eq!(replacements[0].1.created_at, "2026-09-19_080000");
    }

    /// A backup deleted in another window while this tab was open would
    /// otherwise reach `compose` as a timestamp naming nothing.
    #[test]
    fn a_source_that_is_no_longer_a_backup_drops_out_of_the_replacements() {
        let mut ui = ComposeUi::default();
        ui.set_base("2026-09-20_100000".to_string());
        ui.sources
            .insert("class_level:PVE_TANK".to_string(), "2026-09-01_000000".to_string());

        assert!(ui.replacements(&[entry("2026-09-20_100000")]).is_empty());
    }

    /// Changing the base clears the chosen sources: a part id the new
    /// base does not offer would be refused by `compose`, and it would
    /// be refused at the click on "create backup" rather than at the
    /// click that caused it.
    #[test]
    fn changing_the_base_clears_the_chosen_sources() {
        let mut ui = ComposeUi::default();
        ui.set_base("2026-09-20_100000".to_string());
        ui.sources
            .insert("class_level:PVE_TANK".to_string(), "2026-09-19_080000".to_string());

        ui.set_base("2026-09-18_070000".to_string());

        assert!(ui.sources.is_empty());
        assert_eq!(ui.base.as_deref(), Some("2026-09-18_070000"));
    }

    /// Setting the base it already has must not throw away work — the
    /// picker sends the action on every click, including on the row
    /// that is already ticked.
    #[test]
    fn setting_the_same_base_again_keeps_the_chosen_sources() {
        let mut ui = ComposeUi::default();
        ui.set_base("2026-09-20_100000".to_string());
        ui.sources
            .insert("class_level:PVE_TANK".to_string(), "2026-09-19_080000".to_string());

        ui.set_base("2026-09-20_100000".to_string());

        assert_eq!(ui.sources.len(), 1);
    }

    use sm2_core::savedata::catalogue::Selector;

    /// Two parts of one multi-part group and one single-part group —
    /// the two shapes the table has to draw differently.
    pub(crate) fn sample_parts() -> Vec<Part> {
        vec![
            Part {
                id: "class_level:PVE_TANK".to_string(),
                group: "class_level",
                file: "config/user_progression.cfg",
                selector: Selector::Member {
                    container: "/UserProgression/UserMastery/masteryStates",
                    key: "PVE_TANK".to_string(),
                },
            },
            Part {
                id: "class_level:PVE_SNIPER".to_string(),
                group: "class_level",
                file: "config/user_progression.cfg",
                selector: Selector::Member {
                    container: "/UserProgression/UserMastery/masteryStates",
                    key: "PVE_SNIPER".to_string(),
                },
            },
            Part {
                id: "economy".to_string(),
                group: "economy",
                file: "config/economy.cfg",
                selector: Selector::Whole,
            },
        ]
    }

    fn ui_with_base() -> ComposeUi {
        let mut ui = ComposeUi::default();
        ui.set_base("2026-09-20_100000".to_string());
        ui
    }

    #[test]
    fn a_collapsed_group_shows_its_header_and_none_of_its_parts() {
        let ui = ui_with_base();

        let rows = ui.rows(&sample_parts());

        // The class_level header, then economy as a plain row.
        assert_eq!(rows.len(), 2);
        let Row::Group(header) = &rows[0] else { panic!("first row is the group header") };
        assert_eq!(header.group, "class_level");
        assert_eq!(header.total, 2);
        assert!(!header.open);
        let Row::Part(part) = &rows[1] else { panic!("a single-part group is drawn as a row") };
        assert_eq!(part.id, "economy");
        assert!(!part.indented);
    }

    #[test]
    fn an_open_group_shows_its_parts_below_the_header() {
        let mut ui = ui_with_base();
        ui.open_groups.insert("class_level");

        let rows = ui.rows(&sample_parts());

        assert_eq!(rows.len(), 4);
        assert!(matches!(&rows[0], Row::Group(header) if header.open));
        assert!(matches!(&rows[1], Row::Part(part) if part.indented));
    }

    #[test]
    fn a_header_counts_the_parts_of_its_group_that_are_replaced() {
        let mut ui = ui_with_base();
        ui.sources
            .insert("class_level:PVE_TANK".to_string(), "2026-09-19_080000".to_string());

        let rows = ui.rows(&sample_parts());

        let Row::Group(header) = &rows[0] else { panic!("first row is the group header") };
        assert_eq!(header.replaced, 1);
        assert_eq!(header.source, GroupSource::Mixed);
    }

    #[test]
    fn a_header_names_the_one_source_all_its_parts_share() {
        let mut ui = ui_with_base();
        for id in ["class_level:PVE_TANK", "class_level:PVE_SNIPER"] {
            ui.sources.insert(id.to_string(), "2026-09-19_080000".to_string());
        }

        let rows = ui.rows(&sample_parts());

        let Row::Group(header) = &rows[0] else { panic!("first row is the group header") };
        assert_eq!(header.source, GroupSource::One("2026-09-19_080000".to_string()));
    }

    /// Without this a filter that matches only parts inside collapsed
    /// groups looks like a filter that does nothing.
    #[test]
    fn a_filter_draws_a_matching_group_open_without_changing_what_is_collapsed() {
        let mut ui = ui_with_base();
        ui.part_filter = "SNIPER".to_string();

        let rows = ui.rows(&sample_parts());

        assert_eq!(rows.len(), 2);
        assert!(matches!(&rows[0], Row::Group(header) if header.open && header.total == 2));
        assert!(matches!(&rows[1], Row::Part(part) if part.id == "class_level:PVE_SNIPER"));
        assert!(ui.open_groups.is_empty(), "the collapsed state is remembered, not overwritten");
    }

    #[test]
    fn a_group_without_a_matching_part_disappears_entirely() {
        let mut ui = ui_with_base();
        ui.part_filter = "economy".to_string();

        let rows = ui.rows(&sample_parts());

        assert_eq!(rows.len(), 1);
        assert!(matches!(&rows[0], Row::Part(part) if part.id == "economy"));
    }

    #[test]
    fn only_replaced_hides_every_part_that_comes_from_the_base() {
        let mut ui = ui_with_base();
        ui.only_replaced = true;
        ui.sources
            .insert("class_level:PVE_TANK".to_string(), "2026-09-19_080000".to_string());

        let rows = ui.rows(&sample_parts());

        assert_eq!(rows.len(), 2);
        assert!(matches!(&rows[0], Row::Group(header) if header.open));
        assert!(matches!(&rows[1], Row::Part(part) if part.id == "class_level:PVE_TANK"));
    }
}

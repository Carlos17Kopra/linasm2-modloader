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
}

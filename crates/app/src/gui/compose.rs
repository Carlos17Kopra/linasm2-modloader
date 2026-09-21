//! The state behind the "compose a save" tab.
//!
//! Everything here can be decided without a screen — which rows are
//! visible, which parts are replaced, what the footer says — and is
//! tested that way. `compose_page` draws it.

use sm2_core::savedata::catalogue;
use sm2_core::savedata::catalogue::{system_version, Documents, Part};
use sm2_core::saves;
use sm2_core::saves::BackupEntry;
use sm2_core::t;
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

    /// Does this part come from a build older than the base's?
    ///
    /// Both numbers are the `systemVersion` of the same file, which is
    /// the number `merge` raises to the base's when it writes the
    /// result. A source that has not been read yet answers `false`:
    /// a badge on a guess sends someone hunting for a problem that is
    /// not there.
    pub(super) fn is_older(&self, part: &PartRow) -> bool {
        let Some(source) = &part.source else { return false };
        let Some(base) = &self.base else { return false };
        let (Some(base_documents), Some(source_documents)) =
            (self.decoded.get(base), self.decoded.get(source))
        else {
            return false;
        };
        match (
            system_version(base_documents, part.file),
            system_version(source_documents, part.file),
        ) {
            (Some(base_version), Some(source_version)) => source_version < base_version,
            _ => false,
        }
    }

    /// The one line under the table when any chosen part comes from an
    /// older build.
    pub(super) fn version_warning(&self, parts: &[Part]) -> Option<String> {
        let older = parts
            .iter()
            .filter(|part| self.is_older(&self.part_row(part, false)))
            .count();
        match older {
            0 => None,
            1 => Some(t!("gui.compose.version_warning_one")),
            count => Some(t!("gui.compose.version_warning_many", count = count)),
        }
    }

    /// What the footer says about the composition as it stands.
    ///
    /// Four whole sentences rather than fragments assembled from
    /// counts: "1 Bestandteil aus einem anderen Backup" and "2
    /// Bestandteile aus 1 anderen Backup" do not share a shape, and
    /// English does not share one with either.
    pub(super) fn summary(&self) -> String {
        let parts = self.sources.len();
        if parts == 0 {
            return t!("gui.compose.summary_none");
        }
        if parts == 1 {
            return t!("gui.compose.summary_one");
        }
        let backups: BTreeSet<&String> = self.sources.values().collect();
        if backups.len() == 1 {
            return t!("gui.compose.summary_many_one_source", parts = parts);
        }
        t!("gui.compose.summary_many", parts = parts, backups = backups.len())
    }

    /// The next backup that has to be read, if any.
    ///
    /// The base comes first: without it there is no part list, and a
    /// source is only ever chosen for a part the base offers. One read
    /// at a time — two threads decoding 3.6 MB of JSON each buy nothing
    /// and the second one is usually for a choice already superseded.
    pub(super) fn needs(&self, backups: &[BackupEntry]) -> Option<BackupEntry> {
        if self.loading.is_some() {
            return None;
        }
        let wanted = std::iter::once(self.base.as_ref()?)
            .chain(self.sources.values())
            .find(|created_at| {
                !self.decoded.contains_key(*created_at) && !self.failed.contains(*created_at)
            })?;
        backups.iter().find(|entry| &entry.created_at == wanted).cloned()
    }

    /// Reads one backup on a worker thread.
    ///
    /// `verify` first, and that is not optional: `read_files` checks the
    /// bytes against nothing, the manifest is what says they are still
    /// the ones that were backed up. Without it a damaged backup offers
    /// a part list that looks perfectly ordinary, and `compose`, which
    /// does verify, refuses the very ids this table just offered. The
    /// order is `cli::part_lines`'s.
    pub(super) fn start_read(&mut self, entry: &BackupEntry, ctx: &egui::Context) {
        let (sender, receiver) = mpsc::channel();
        let entry = entry.clone();
        let created_at = entry.created_at.clone();
        let ctx = ctx.clone();

        self.loading = Some(created_at.clone());
        self.incoming = Some(receiver);
        std::thread::spawn(move || {
            let result = saves::verify(&entry)
                .and_then(|()| saves::read_files(&entry))
                .and_then(|files| catalogue::documents(&files))
                .map_err(|e| e.to_string());
            // The receiver is gone if the window was closed meanwhile —
            // not an error, just a result nobody picks up any more.
            let _ = sender.send(Loaded { created_at, result });
            ctx.request_repaint();
        });
    }

    /// Takes a finished read, if one has arrived. `true` when something
    /// changed and the caller should redraw.
    pub(super) fn poll(&mut self) -> bool {
        let Some(incoming) = &self.incoming else { return false };
        match incoming.try_recv() {
            Ok(loaded) => {
                self.incoming = None;
                self.accept(loaded);
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.incoming = None;
                self.loading = None;
                true
            }
        }
    }

    fn accept(&mut self, loaded: Loaded) {
        self.loading = None;
        match loaded.result {
            Ok(documents) => {
                // The base is also what the part list comes from, and
                // it is the only backup that may fill it: a source
                // offers parts the base does not have, and offering
                // those would produce ids `compose` refuses.
                if self.base.as_deref() == Some(loaded.created_at.as_str()) {
                    self.parts = catalogue::parts(&documents);
                }
                self.decoded.insert(loaded.created_at, Arc::new(documents));
                self.error = None;
            }
            Err(detail) => {
                // Every part that named this backup goes back to the
                // base. Leaving them pointing at a backup that cannot
                // be read would offer a button that is certain to fail.
                self.sources.retain(|_, created_at| created_at != &loaded.created_at);
                self.failed.insert(loaded.created_at);
                self.error = Some(t!("gui.compose.read_failed", detail = detail));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::language_test_lock;
    use sm2_core::i18n::{set_language, Language};
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

    /// A `Documents` holding one file at one version.
    ///
    /// Built through the core's own encoder and decoder rather than
    /// `serde_json::json!`: `crates/app` does not depend on `serde_json`
    /// and must not start to for a fixture.
    fn documents_at(file: &str, root: &str, version: u64) -> Documents {
        let text = format!(r#"{{"{root}":{{"systemVersion":{version}}}}}"#);
        let files =
            BTreeMap::from([(file.to_string(), sm2_core::savedata::ssf1::encode(text.as_bytes()))]);
        sm2_core::savedata::catalogue::documents(&files).expect("the fixture must decode")
    }

    #[test]
    fn a_part_from_a_source_with_an_older_version_is_marked() {
        let mut ui = ui_with_base();
        let file = "config/user_progression.cfg";
        ui.decoded.insert(
            "2026-09-20_100000".to_string(),
            Arc::new(documents_at(file, "UserProgression", 900)),
        );
        ui.decoded.insert(
            "2026-09-19_080000".to_string(),
            Arc::new(documents_at(file, "UserProgression", 890)),
        );
        ui.sources
            .insert("class_level:PVE_TANK".to_string(), "2026-09-19_080000".to_string());
        ui.open_groups.insert("class_level");

        let rows = ui.rows(&sample_parts());
        let Row::Part(part) = &rows[1] else { panic!("the open group's first part") };

        assert!(ui.is_older(part));
    }

    #[test]
    fn a_part_from_the_base_is_never_marked_as_older() {
        let mut ui = ui_with_base();
        ui.decoded.insert(
            "2026-09-20_100000".to_string(),
            Arc::new(documents_at("config/user_progression.cfg", "UserProgression", 900)),
        );
        ui.open_groups.insert("class_level");

        let rows = ui.rows(&sample_parts());
        let Row::Part(part) = &rows[1] else { panic!("the open group's first part") };

        assert!(!ui.is_older(part));
    }

    /// A source that has not been read yet, or a file without a version,
    /// says nothing rather than "older" — a badge on a guess would send
    /// someone hunting for a problem that is not there.
    #[test]
    fn an_unread_source_is_not_marked_as_older() {
        let mut ui = ui_with_base();
        ui.sources
            .insert("class_level:PVE_TANK".to_string(), "2026-09-19_080000".to_string());
        ui.open_groups.insert("class_level");

        let rows = ui.rows(&sample_parts());
        let Row::Part(part) = &rows[1] else { panic!("the open group's first part") };

        assert!(!ui.is_older(part));
    }

    #[test]
    fn the_warning_appears_once_for_any_part_from_an_older_build() {
        let _held = language_test_lock();
        set_language(Language::English);
        let mut ui = ui_with_base();
        let file = "config/user_progression.cfg";
        ui.decoded.insert(
            "2026-09-20_100000".to_string(),
            Arc::new(documents_at(file, "UserProgression", 900)),
        );
        ui.decoded.insert(
            "2026-09-19_080000".to_string(),
            Arc::new(documents_at(file, "UserProgression", 890)),
        );
        ui.sources
            .insert("class_level:PVE_TANK".to_string(), "2026-09-19_080000".to_string());

        assert_eq!(
            ui.version_warning(&sample_parts()).as_deref(),
            Some("One part comes from an older game version. It is written at the base's version.")
        );
    }

    #[test]
    fn without_an_older_source_there_is_no_warning() {
        let ui = ui_with_base();

        assert_eq!(ui.version_warning(&sample_parts()), None);
    }

    #[test]
    fn the_summary_says_nothing_is_replaced_in_both_languages() {
        let _held = language_test_lock();
        let ui = ui_with_base();

        set_language(Language::English);
        assert_eq!(ui.summary(), "Everything comes from the base save.");
        set_language(Language::German);
        assert_eq!(ui.summary(), "Alles stammt aus dem Basis-Save.");
        set_language(Language::English);
    }

    #[test]
    fn the_summary_counts_parts_and_the_backups_they_come_from() {
        let _held = language_test_lock();
        let mut ui = ui_with_base();
        ui.sources
            .insert("class_level:PVE_TANK".to_string(), "2026-09-19_080000".to_string());
        ui.sources
            .insert("class_level:PVE_SNIPER".to_string(), "2026-09-18_070000".to_string());

        set_language(Language::English);
        assert_eq!(ui.summary(), "2 parts from 2 other backups.");
        set_language(Language::German);
        assert_eq!(ui.summary(), "2 Bestandteile aus 2 anderen Backups.");
        set_language(Language::English);
    }

    /// Two parts out of one backup: the sentence counts backups, not
    /// picks, so the same source twice is still one backup.
    #[test]
    fn the_summary_counts_a_backup_once_however_many_parts_come_from_it() {
        let _held = language_test_lock();
        let mut ui = ui_with_base();
        for id in ["class_level:PVE_TANK", "class_level:PVE_SNIPER"] {
            ui.sources.insert(id.to_string(), "2026-09-19_080000".to_string());
        }

        set_language(Language::English);
        assert_eq!(ui.summary(), "2 parts from 1 other backup.");
        set_language(Language::German);
        assert_eq!(ui.summary(), "2 Bestandteile aus 1 anderen Backup.");
        set_language(Language::English);
    }

    /// One part from one backup: both counts are singular, and German and
    /// English word that differently enough that a composed sentence would
    /// go wrong — hence four complete sentences rather than fragments.
    #[test]
    fn the_summary_has_its_own_sentence_for_a_single_part() {
        let _held = language_test_lock();
        let mut ui = ui_with_base();
        ui.sources
            .insert("class_level:PVE_TANK".to_string(), "2026-09-19_080000".to_string());

        set_language(Language::English);
        assert_eq!(ui.summary(), "1 part from another backup.");
        set_language(Language::German);
        assert_eq!(ui.summary(), "1 Bestandteil aus einem anderen Backup.");
        set_language(Language::English);
    }

    #[test]
    fn the_base_is_what_is_wanted_first() {
        let backups = [entry("2026-09-20_100000"), entry("2026-09-19_080000")];
        let mut ui = ComposeUi::default();
        ui.set_base("2026-09-20_100000".to_string());

        assert_eq!(ui.needs(&backups).map(|e| e.created_at), Some("2026-09-20_100000".to_string()));
    }

    #[test]
    fn a_chosen_source_is_wanted_once_the_base_is_there() {
        let backups = [entry("2026-09-20_100000"), entry("2026-09-19_080000")];
        let mut ui = ComposeUi::default();
        ui.set_base("2026-09-20_100000".to_string());
        ui.decoded.insert("2026-09-20_100000".to_string(), Arc::new(Documents::new()));
        ui.sources
            .insert("class_level:PVE_TANK".to_string(), "2026-09-19_080000".to_string());

        assert_eq!(ui.needs(&backups).map(|e| e.created_at), Some("2026-09-19_080000".to_string()));
    }

    #[test]
    fn nothing_is_wanted_while_a_read_is_running() {
        let backups = [entry("2026-09-20_100000")];
        let mut ui = ComposeUi::default();
        ui.set_base("2026-09-20_100000".to_string());
        ui.loading = Some("2026-09-20_100000".to_string());

        assert!(ui.needs(&backups).is_none());
    }

    /// A backup that failed to read must not be asked for again on the very
    /// next frame — that would be a loop hammering a damaged archive.
    #[test]
    fn a_backup_that_failed_is_not_wanted_again() {
        let backups = [entry("2026-09-20_100000")];
        let mut ui = ComposeUi::default();
        ui.set_base("2026-09-20_100000".to_string());
        ui.failed.insert("2026-09-20_100000".to_string());

        assert!(ui.needs(&backups).is_none());
    }

    #[test]
    fn a_failed_read_puts_the_part_back_on_the_base_and_states_why() {
        let _held = language_test_lock();
        set_language(Language::English);
        let mut ui = ComposeUi::default();
        ui.set_base("2026-09-20_100000".to_string());
        ui.decoded.insert("2026-09-20_100000".to_string(), Arc::new(Documents::new()));
        ui.sources
            .insert("class_level:PVE_TANK".to_string(), "2026-09-19_080000".to_string());
        ui.loading = Some("2026-09-19_080000".to_string());

        ui.accept(Loaded {
            created_at: "2026-09-19_080000".to_string(),
            result: Err("archive damaged".to_string()),
        });

        assert!(ui.sources.is_empty(), "the part goes back to the base");
        assert_eq!(
            ui.error.as_deref(),
            Some("This backup could not be read: archive damaged")
        );
        assert!(ui.loading.is_none());
    }

    #[test]
    fn a_successful_read_clears_the_error_and_keeps_the_documents() {
        let mut ui = ComposeUi::default();
        ui.set_base("2026-09-20_100000".to_string());
        ui.loading = Some("2026-09-20_100000".to_string());
        ui.error = Some("something earlier".to_string());

        ui.accept(Loaded {
            created_at: "2026-09-20_100000".to_string(),
            result: Ok(Documents::new()),
        });

        assert!(ui.decoded.contains_key("2026-09-20_100000"));
        assert!(ui.error.is_none());
        assert!(ui.loading.is_none());
    }
}

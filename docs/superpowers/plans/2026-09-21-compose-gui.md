# Compose GUI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the window the "Save zusammenstellen" tab, so a
composition can be assembled and written without the command line.

**Architecture:** `Section::Saves` grows a tab strip. All state and every
decision that does not need a screen lives in `gui/compose.rs`
(`ComposeUi`) and is unit-tested; `gui/compose_page.rs` draws it.
Decoding a backup happens on demand on a worker thread with its own
channel, so it never occupies `App.task` and never disables the page;
writing the composition goes through `tasks::spawn` like every other job
that changes something on disk.

**Tech Stack:** Rust, egui 0.36, `sm2-core` (`savedata::catalogue`,
`savedata::compose`, `savedata::summary`, `saves`). No new dependencies.

**Spec:** `docs/superpowers/specs/2026-09-21-compose-gui-design.md`

## Global Constraints

- Code, comments, test names and `assert!` messages are English. Every
  sentence a user sees is a catalogue entry, reached with `t!("area.key")`.
- New keys go into **both** `crates/core/i18n/en.toml` and
  `crates/core/i18n/de.toml` with exactly the same key set and the same
  placeholders. Five tests enforce this; `cargo test` is the check.
- Comment prose wraps at 78 columns including the `///` prefix.
  Comments say *why*, not *what*.
- `crates/core` MSRV 1.85, `crates/app` MSRV 1.95. No new dependencies
  in either crate — `crates/app` has no `serde_json` and must not gain
  one, which is why Task 1 puts the version lookup in core.
- Any test whose assertion depends on the active language holds
  `crate::app_state::language_test_lock()` for the whole assertion.
- Test-first: write the failing test, watch it fail, then implement.
- `cargo test` and `cargo clippy --all-targets` are clean at the end of
  every task.
- Commit after every task, one commit per task, message in English,
  ending with the `Co-Authored-By` line this repository uses.

---

### Task 1: The game version of one file, in public

`merge` reads a file's `systemVersion` through a private helper. The
interface has to compare the same number to mark a part whose source
comes from an older build, and `crates/app` cannot reach into `merge`
(it is `pub(crate)`) nor name `serde_json::Value` (not a dependency).
Move the helper into the catalogue, where the rest of the public reading
surface already is, and let `merge` call it.

**Files:**
- Modify: `crates/core/src/savedata/catalogue.rs` (new public function
  beside `system_key`, around line 118)
- Modify: `crates/core/src/savedata/merge.rs:120-124` (delete the
  private `system_version`, call the catalogue's)

**Interfaces:**
- Consumes: `catalogue::Documents`, `catalogue::Selector`
- Produces: `pub fn catalogue::system_version(documents: &Documents,
  file: &str) -> Option<u64>`

- [ ] **Step 1: Write the failing test**

In `crates/core/src/savedata/catalogue.rs`, inside `mod tests`:

```rust
#[test]
fn system_version_reads_the_version_of_one_file() {
    let documents: Documents = BTreeMap::from([(
        "config/economy.cfg".to_string(),
        json!({"Economy": {"systemVersion": 890, "credits": 12}}),
    )]);

    assert_eq!(system_version(&documents, "config/economy.cfg"), Some(890));
}

/// A file an older build wrote may carry no version at all, and a file
/// that is not in this backup is not a defect either — both yield
/// nothing rather than a zero, which a caller would read as "ancient".
#[test]
fn system_version_yields_nothing_for_a_missing_file_or_version() {
    let documents: Documents = BTreeMap::from([(
        "config/economy.cfg".to_string(),
        json!({"Economy": {"credits": 12}}),
    )]);

    assert_eq!(system_version(&documents, "config/economy.cfg"), None);
    assert_eq!(system_version(&documents, "config/nothing.cfg"), None);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p sm2-core system_version`
Expected: FAIL, `cannot find function system_version in this scope`

- [ ] **Step 3: Implement**

In `crates/core/src/savedata/catalogue.rs`, below `system_key`:

```rust
/// The `systemVersion` a file's system object carries, if it carries
/// one.
///
/// Public because the interface compares a source's version against
/// the base's to mark a part that comes from an older build — the same
/// number `merge` raises when it writes one.
pub fn system_version(documents: &Documents, file: &str) -> Option<u64> {
    Selector::Whole.get(documents.get(file)?)?.get("systemVersion")?.as_u64()
}
```

In `crates/core/src/savedata/merge.rs`, delete the private
`system_version` (lines 120-124 including its doc comment) and replace
every call to it with `catalogue::system_version`. `merge.rs` already
has `use crate::savedata::catalogue::{...}`; add `system_version` to
that list if it is not there.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p sm2-core && cargo clippy --all-targets`
Expected: PASS, no warnings. The existing merge tests must still pass —
they are what says the move changed no behaviour.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/savedata/catalogue.rs crates/core/src/savedata/merge.rs
git commit -m "$(cat <<'EOF'
refactor(savedata): read a file's version through the catalogue

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: `ComposeUi` — the state and what the core is asked for

The state struct, the two enums the interface needs, and the one
function that turns the state into `compose::compose`'s argument. No
drawing, no threads.

**Files:**
- Create: `crates/app/src/gui/compose.rs`
- Modify: `crates/app/src/gui/mod.rs` (add `mod compose;` beside the
  other page modules near the top, and the `compose: compose::ComposeUi`
  field to `App` plus `compose: compose::ComposeUi::default()` in
  `App::blank`)

**Interfaces:**
- Consumes: `sm2_core::saves::BackupEntry`,
  `sm2_core::savedata::catalogue::Documents`
- Produces:
  - `pub(super) enum SavesTab { Backups, Compose }`
  - `pub(super) enum Picker { Base, Part(String), Group(&'static str) }`
  - `pub(super) struct ComposeUi` with the fields listed below
  - `pub(super) fn ComposeUi::set_base(&mut self, created_at: String)`
  - `pub(super) fn ComposeUi::replacements(&self, backups: &[BackupEntry])
    -> Vec<(String, BackupEntry)>`

- [ ] **Step 1: Write the failing test**

Create `crates/app/src/gui/compose.rs` with only this test module at the
bottom (the file starts empty apart from it, so the test cannot pass by
accident):

```rust
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p lina-sm2 compose`
Expected: FAIL, `cannot find type ComposeUi in this scope`

- [ ] **Step 3: Implement**

Write the head of `crates/app/src/gui/compose.rs` above the test module:

```rust
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
```

In `crates/app/src/gui/mod.rs`: add `mod compose;` beside the other page
modules, the field `compose: compose::ComposeUi,` to `App` after
`saves_blocked`/`steam_users`, and `compose: compose::ComposeUi::default(),`
to `App::blank`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p lina-sm2 compose && cargo clippy --all-targets`
Expected: PASS. Dead-code warnings for fields nothing reads yet are
expected here and gone by Task 9; if clippy is configured to deny them,
put `#[allow(dead_code)]` on `ComposeUi` and **remove it again in Task
9** — leaving it there would hide a field the drawing forgot.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/gui/compose.rs crates/app/src/gui/mod.rs
git commit -m "$(cat <<'EOF'
feat(gui): the state behind composing a backup

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: The rows — groups, collapsing, filtering

163 parts in 14 groups. The table shows group headers that collapse, and
the eight groups holding a single part are drawn as plain rows without a
header. A filter or "only replaced" draws a matching group open without
overwriting what the user collapsed.

**Files:**
- Modify: `crates/app/src/gui/compose.rs`

**Interfaces:**
- Consumes: `ComposeUi` from Task 2, `catalogue::{parts, Part}`
- Produces:
  - `pub(super) enum Row { Group(GroupRow), Part(PartRow) }`
  - `pub(super) struct GroupRow { pub group: &'static str, pub total: usize,
    pub replaced: usize, pub source: GroupSource, pub open: bool }`
  - `pub(super) enum GroupSource { Base, One(String), Mixed }`
  - `pub(super) struct PartRow { pub id: String, pub group: &'static str,
    pub file: &'static str, pub source: Option<String>, pub indented: bool }`
  - `pub(super) fn ComposeUi::rows(&self, parts: &[Part]) -> Vec<Row>`

- [ ] **Step 1: Write the failing test**

Add to the test module in `crates/app/src/gui/compose.rs`:

```rust
use sm2_core::savedata::catalogue::{Part, Selector};

/// Two parts of one multi-part group and one single-part group —
/// the two shapes the table has to draw differently.
fn sample_parts() -> Vec<Part> {
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p lina-sm2 compose`
Expected: FAIL, `cannot find type Row in this scope`

- [ ] **Step 3: Implement**

Add to `crates/app/src/gui/compose.rs` (`Part` is already imported from
Task 2):

```rust
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
                rows.push(Row::Part(self.part_row(members[0], false)));
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p lina-sm2 compose && cargo clippy --all-targets`
Expected: PASS, no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/gui/compose.rs
git commit -m "$(cat <<'EOF'
feat(gui): group, collapse and filter the part table

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: The version badge and the mixed-version warning

A part whose source wrote its file with an older `systemVersion` than
the base carries a badge, and the footer warns once when any chosen
source does. Both read the same number `merge` raises.

**Files:**
- Modify: `crates/app/src/gui/compose.rs`
- Modify: `crates/core/i18n/en.toml`, `crates/core/i18n/de.toml`

**Interfaces:**
- Consumes: `catalogue::system_version` (Task 1), `ComposeUi::decoded`
- Produces:
  - `pub(super) fn ComposeUi::is_older(&self, part: &PartRow) -> bool`
  - `pub(super) fn ComposeUi::version_warning(&self, parts: &[Part]) -> Option<String>`

- [ ] **Step 1: Write the failing test**

Add to the test module in `crates/app/src/gui/compose.rs`:

```rust
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
```

The test module needs `use crate::app_state::language_test_lock;` and
`use sm2_core::i18n::{set_language, Language};` at its top.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p lina-sm2 compose`
Expected: FAIL, `no method named is_older`

- [ ] **Step 3: Implement**

Add to `crates/app/src/gui/compose.rs`:

```rust
use sm2_core::savedata::catalogue::system_version;
use sm2_core::t;

impl ComposeUi {
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
}
```

Add to `crates/core/i18n/en.toml` under a new `[gui.compose]` section
(place it after `[gui.saves]`):

```toml
version_warning_one = "One part comes from an older game version. It is written at the base's version."
version_warning_many = "{count} parts come from an older game version. They are written at the base's version."
```

And to `crates/core/i18n/de.toml`, in the same place:

```toml
version_warning_one = "Ein Bestandteil stammt aus einer älteren Spielversion. Er wird auf der Version des Basis-Saves geschrieben."
version_warning_many = "{count} Bestandteile stammen aus einer älteren Spielversion. Sie werden auf der Version des Basis-Saves geschrieben."
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test && cargo clippy --all-targets`
Expected: PASS — the whole suite, because the i18n tests
(`every_language_has_exactly_the_english_keys` and the placeholder one)
are what says the two files still agree.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/gui/compose.rs crates/core/i18n/en.toml crates/core/i18n/de.toml
git commit -m "$(cat <<'EOF'
feat(gui): mark a part that comes from an older build

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: The footer summary

One sentence saying what has been chosen, in both languages.

**Files:**
- Modify: `crates/app/src/gui/compose.rs`
- Modify: `crates/core/i18n/en.toml`, `crates/core/i18n/de.toml`

**Interfaces:**
- Produces: `pub(super) fn ComposeUi::summary(&self) -> String`

- [ ] **Step 1: Write the failing test**

```rust
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p lina-sm2 summary`
Expected: FAIL, `no method named summary`

- [ ] **Step 3: Implement**

```rust
impl ComposeUi {
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
}
```

`en.toml`, under `[gui.compose]`:

```toml
summary_none = "Everything comes from the base save."
summary_one = "1 part from another backup."
summary_many_one_source = "{parts} parts from 1 other backup."
summary_many = "{parts} parts from {backups} other backups."
```

`de.toml`:

```toml
summary_none = "Alles stammt aus dem Basis-Save."
summary_one = "1 Bestandteil aus einem anderen Backup."
summary_many_one_source = "{parts} Bestandteile aus 1 anderen Backup."
summary_many = "{parts} Bestandteile aus {backups} anderen Backups."
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test && cargo clippy --all-targets`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/gui/compose.rs crates/core/i18n/en.toml crates/core/i18n/de.toml
git commit -m "$(cat <<'EOF'
feat(gui): say in one line what a composition takes from where

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: Reading a backup on demand

The tab needs the decoded files of the base and of every backup chosen
as a source. This does not go through `App.task`: that slot disables the
whole interface and occupies the status bar, which is right for "make a
backup" and wrong for "read a source".

**Files:**
- Modify: `crates/app/src/gui/compose.rs`
- Modify: `crates/core/i18n/en.toml`, `crates/core/i18n/de.toml`

**Interfaces:**
- Consumes: `saves::verify`, `saves::read_files`, `catalogue::documents`
- Produces:
  - `pub(super) fn ComposeUi::needs(&self, backups: &[BackupEntry]) -> Option<BackupEntry>`
  - `pub(super) fn ComposeUi::start_read(&mut self, entry: &BackupEntry, ctx: &egui::Context)`
  - `pub(super) fn ComposeUi::poll(&mut self) -> bool`

- [ ] **Step 1: Write the failing test**

```rust
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p lina-sm2 compose`
Expected: FAIL, `no method named needs`

- [ ] **Step 3: Implement**

`ComposeUi` already has the `failed` and `parts` fields from Task 2.
Add:

```rust
impl ComposeUi {
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
```

Imports to add at the top of the file: `use sm2_core::savedata::catalogue;`
and `use sm2_core::saves;`.

`en.toml`, under `[gui.compose]`:

```toml
read_failed = "This backup could not be read: {detail}"
```

`de.toml`:

```toml
read_failed = "Dieses Backup ließ sich nicht lesen: {detail}"
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test && cargo clippy --all-targets`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/gui/compose.rs crates/core/i18n/en.toml crates/core/i18n/de.toml
git commit -m "$(cat <<'EOF'
feat(gui): read a backup's parts on demand, without locking the page

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 7: Writing the composition

The button. This one *does* go through `tasks::spawn`, like every job
that changes something on disk.

**Files:**
- Modify: `crates/app/src/gui/tasks.rs` (new `Outcome::Composed`, new
  `start_compose`, new arm in `finish`)
- Modify: `crates/core/i18n/en.toml`, `crates/core/i18n/de.toml`

**Interfaces:**
- Consumes: `ComposeUi::replacements` (Task 2), `savedata::compose::compose`
- Produces: `pub(super) fn App::start_compose(&mut self)`

- [ ] **Step 1: Write the failing test**

In the test module of `crates/app/src/gui/tasks.rs`:

```rust
/// The two sentences the composition ends in, in both languages —
/// the outcome arm picks one of them and nothing else checks the
/// wording.
#[test]
fn the_composition_result_reads_correctly_in_both_languages() {
    let _held = language_test_lock();
    set_language(Language::English);
    assert_eq!(
        t!("gui.message.compose_done", created_at = "2026-09-21_120000"),
        "Composed backup 2026-09-21_120000 created."
    );
    set_language(Language::German);
    assert_eq!(
        t!("gui.message.compose_failed", detail = "keine Basis"),
        "Zusammenstellen fehlgeschlagen: keine Basis"
    );
    set_language(Language::English);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p lina-sm2 composition_result`
Expected: FAIL — the key is missing, so `t!` yields the key itself and
the assertion does not match.

- [ ] **Step 3: Implement**

`en.toml`, under `[gui.message]`:

```toml
compose_done = "Composed backup {created_at} created."
compose_failed = "Composing failed: {detail}"
compose_running = "Composing the backup …"
```

`de.toml`:

```toml
compose_done = "Zusammengestelltes Backup {created_at} erstellt."
compose_failed = "Zusammenstellen fehlgeschlagen: {detail}"
compose_running = "Backup wird zusammengestellt …"
```

In `crates/app/src/gui/tasks.rs`, add to `enum Outcome`:

```rust
    /// A backup composed from several others.
    Composed(Result<BackupEntry, String>),
```

Add to `fn finish`:

```rust
            Outcome::Composed(Ok(entry)) => {
                self.refresh_backups();
                // The result belongs in the list, which is also where it
                // is restored from — so the tab that made it steps aside.
                self.saves_tab = SavesTab::Backups;
                self.set_status(t!("gui.message.compose_done", created_at = entry.created_at));
            }
            Outcome::Composed(Err(error)) => {
                // The composed state stays as it is: the message names
                // the one disputed part or source, and changing it and
                // pressing again is the obvious next move.
                self.set_warning(t!("gui.message.compose_failed", detail = error));
            }
```

And the starter, beside `start_backup`:

```rust
    /// Writes the composition the compose tab describes.
    ///
    /// Unlike `start_backup` this needs no save directory: it reads
    /// backups and writes a backup, and never touches the game's own
    /// directory. That is why the tab stays usable while the savegame
    /// functions are locked.
    pub(super) fn start_compose(&mut self) {
        let Some(base_at) = self.compose.base.clone() else { return };
        let Some(base) = self.backups.iter().find(|e| e.created_at == base_at).cloned() else {
            return;
        };
        let Some(backups_dir) = self.backups_dir() else { return };
        let replacements = self.compose.replacements(&self.backups);
        let label = self.compose.label.trim().to_owned();
        let label = (!label.is_empty()).then_some(label);
        let ctx = self.egui_ctx.clone();

        self.set_busy(t!("gui.message.compose_running"));
        self.task = Some(spawn(ctx, false, move |_cancel, progress| {
            report(progress, 0.3, t!("gui.message.reading_hashing_saves"));
            let result =
                compose::compose(&base, &replacements, &backups_dir, label.as_deref())
                    .map_err(|e| e.to_string());
            Outcome::Composed(result)
        }));
    }
```

Add `use sm2_core::savedata::compose;` and `use super::compose::SavesTab;`
to the imports of `tasks.rs`, and `use crate::app_state::language_test_lock;`
plus `use sm2_core::i18n::{set_language, Language};` to its test module if
they are not there yet.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test && cargo clippy --all-targets`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/gui/tasks.rs crates/core/i18n/en.toml crates/core/i18n/de.toml
git commit -m "$(cat <<'EOF'
feat(gui): write the composed backup as a background job

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 8: Actions and wiring

The actions the page pushes and what `App::apply` does with them, plus
the per-frame poll of the reader.

**Files:**
- Modify: `crates/app/src/gui/mod.rs` (the `Action` enum around line
  241, `App::apply` around line 693, the `saves_tab` field, and the
  `poll_task` call site so the reader is polled too)

**Interfaces:**
- Consumes: everything from Tasks 2-7
- Produces: the `Action` variants listed below, handled in `apply`

- [ ] **Step 1: Write the failing test**

In `mod.rs`'s test module:

```rust
/// Picking a base and then a source for a part leaves exactly the
/// state `compose` is asked for — the path a click takes, without a
/// screen.
#[test]
fn picking_a_base_and_a_source_builds_the_composition() {
    let mut app = App::blank(egui::Context::default());

    app.apply(Action::PickComposeBase("2026-09-20_100000".to_string()));
    app.apply(Action::PickPartSource {
        part: "class_level:PVE_TANK".to_string(),
        backup: "2026-09-19_080000".to_string(),
    });

    assert_eq!(app.compose.base.as_deref(), Some("2026-09-20_100000"));
    assert_eq!(
        app.compose.sources.get("class_level:PVE_TANK").map(String::as_str),
        Some("2026-09-19_080000")
    );
    assert!(app.compose.picker.is_none(), "a pick closes the dropdown");
}

#[test]
fn resetting_one_part_puts_it_back_on_the_base() {
    let mut app = App::blank(egui::Context::default());
    app.apply(Action::PickComposeBase("2026-09-20_100000".to_string()));
    app.apply(Action::PickPartSource {
        part: "class_level:PVE_TANK".to_string(),
        backup: "2026-09-19_080000".to_string(),
    });

    app.apply(Action::ResetPart("class_level:PVE_TANK".to_string()));

    assert!(app.compose.sources.is_empty());
}

/// A group's dropdown assigns the whole group at once — the point of
/// having one.
#[test]
fn picking_a_group_source_assigns_every_part_of_that_group() {
    let mut app = App::blank(egui::Context::default());
    app.apply(Action::PickComposeBase("2026-09-20_100000".to_string()));
    // What a read of the base would have filled in (Task 6): the group
    // dropdown assigns every part the base offers in that group.
    app.compose.parts = crate::gui::compose::tests::sample_parts();

    app.apply(Action::PickGroupSource {
        group: "class_level",
        backup: "2026-09-19_080000".to_string(),
    });

    assert_eq!(app.compose.sources.len(), 2, "both class levels, not the economy part");
}
```

`sample_parts` lives in `compose.rs`'s test module (Task 3); mark that
module `pub(crate)` and the function `pub(crate)` so this test can reach
it, rather than writing the same three parts out a second time.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p lina-sm2 compose`
Expected: FAIL, `no variant named PickComposeBase`

- [ ] **Step 3: Implement**

Add to `enum Action`:

```rust
    ShowSavesTab(compose::SavesTab),
    PickComposeBase(String),
    PickPartSource { part: String, backup: String },
    PickGroupSource { group: &'static str, backup: String },
    ResetPart(String),
    ResetComposition,
    SetPartFilter(String),
    /// The filter inside an open dropdown, which is a different field
    /// from the table's filter and must not share it.
    SetPickerFilter(String),
    ToggleOnlyReplaced,
    ToggleGroup(&'static str),
    OpenComposePicker(compose::Picker),
    CloseComposePicker,
    SetComposeLabel(String),
    Compose,
```

Add to `App`: `saves_tab: compose::SavesTab,` (and its default in
`blank`). Add to `apply`:

```rust
            Action::ShowSavesTab(tab) => {
                self.saves_tab = tab;
                self.compose.picker = None;
            }
            Action::PickComposeBase(created_at) => {
                self.compose.set_base(created_at);
            }
            Action::PickPartSource { part, backup } => self.compose.set_source(part, backup),
            Action::PickGroupSource { group, backup } => {
                self.compose.set_group_source(group, backup);
            }
            Action::ResetPart(part) => {
                self.compose.sources.remove(&part);
                self.compose.picker = None;
            }
            Action::ResetComposition => {
                self.compose.sources.clear();
                self.compose.picker = None;
            }
            Action::SetPartFilter(filter) => self.compose.part_filter = filter,
            Action::SetPickerFilter(filter) => self.compose.picker_filter = filter,
            Action::ToggleOnlyReplaced => {
                self.compose.only_replaced = !self.compose.only_replaced;
            }
            Action::ToggleGroup(group) => {
                if !self.compose.open_groups.remove(group) {
                    self.compose.open_groups.insert(group);
                }
            }
            Action::OpenComposePicker(picker) => {
                // Clicking the open dropdown closes it again.
                self.compose.picker =
                    (self.compose.picker.as_ref() != Some(&picker)).then_some(picker);
                self.compose.picker_filter.clear();
            }
            Action::CloseComposePicker => self.compose.picker = None,
            Action::SetComposeLabel(label) => self.compose.label = label,
            Action::Compose => self.start_compose(),
```

And in `compose.rs`:

```rust
impl ComposeUi {
    /// The source of one part. Choosing the base itself is how a part
    /// goes back to the base, so it removes rather than inserts.
    pub(super) fn set_source(&mut self, part: String, backup: String) {
        if self.base.as_deref() == Some(backup.as_str()) {
            self.sources.remove(&part);
        } else {
            self.sources.insert(part, backup);
        }
        self.picker = None;
        self.picker_filter.clear();
    }

    /// The source of every part of one group at once — "all my weapons
    /// from the September backup" is what a group dropdown is for.
    pub(super) fn set_group_source(&mut self, group: &'static str, backup: String) {
        let ids: Vec<String> =
            self.parts.iter().filter(|p| p.group == group).map(|p| p.id.clone()).collect();
        for id in ids {
            self.set_source(id, backup.clone());
        }
        self.picker = None;
        self.picker_filter.clear();
    }
}
```

Finally, in the frame loop next to `self.poll_task(ctx)`, add the
reader's poll and its kick-off:

```rust
        self.poll_task(ctx);
        if self.compose.poll() {
            ctx.request_repaint();
        }
        if self.section == Section::Saves && self.saves_tab == compose::SavesTab::Compose {
            // The base defaults to the newest backup, which is what the
            // Backups tab shows first too.
            if self.compose.base.is_none() {
                if let Some(newest) = self.backups.first() {
                    self.compose.set_base(newest.created_at.clone());
                }
            }
            if let Some(entry) = self.compose.needs(&self.backups) {
                self.compose.start_read(&entry, ctx);
            }
        }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test && cargo clippy --all-targets`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/gui/mod.rs crates/app/src/gui/compose.rs
git commit -m "$(cat <<'EOF'
feat(gui): wire the composition actions into the app state

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 9: Drawing the tab

The last task, and the only one with no new logic: the tab strip, the
base picker, the table, the dropdown popover, the warning and the
footer. Everything it needs to decide has been decided and tested in
Tasks 2-8.

**Files:**
- Create: `crates/app/src/gui/compose_page.rs`
- Modify: `crates/app/src/gui/saves_page.rs` (tab strip at the top of
  `show`, dispatch to `compose_page::show`)
- Modify: `crates/app/src/gui/mod.rs` (`mod compose_page;`; remove the
  `#[allow(dead_code)]` from Task 2 if it was needed)
- Modify: `crates/core/i18n/en.toml`, `crates/core/i18n/de.toml`

**Interfaces:**
- Consumes: `ComposeUi::{rows, summary, version_warning, is_older}`,
  `widgets::{button, text_field, columns, truncated, badge, ButtonStyle,
  Column, Icon}`, `super::{draw_card, draw_column_head, empty_hint,
  page_heading}`, `summary::{group_name, summarize}`
- Produces: `pub fn compose_page::show(app: &App, ui: &mut Ui,
  actions: &mut Vec<Action>)`

- [ ] **Step 1: Write the failing test**

Drawing is checked by eye; what a test can hold is the wording the page
puts on screen. In `crates/app/src/gui/compose_page.rs`:

```rust
#[cfg(test)]
mod tests {
    use crate::app_state::language_test_lock;
    use sm2_core::i18n::{set_language, Language};
    use sm2_core::t;

    /// The tab strip and the explanatory line under it, in both
    /// languages — the two pieces of copy that say what this tab is
    /// for, and the first thing a translator gets wrong.
    #[test]
    fn the_tab_introduces_itself_in_both_languages() {
        let _held = language_test_lock();
        set_language(Language::English);
        assert_eq!(t!("gui.compose.tab"), "Compose a save");
        assert!(t!("gui.compose.intro").contains("new backup"));
        set_language(Language::German);
        assert_eq!(t!("gui.compose.tab"), "Save zusammenstellen");
        assert!(t!("gui.compose.intro").contains("neues Backup"));
        set_language(Language::English);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p lina-sm2 tab_introduces`
Expected: FAIL — the keys are missing, so `t!` yields the key itself.

- [ ] **Step 3: Implement**

Add the keys. `en.toml`, under `[gui.compose]`:

```toml
tab = "Compose a save"
tab_backups = "Backups"
intro = "The base save supplies every part. Individual parts can be taken from other backups. The result is put away as a new backup — the real save data stays untouched until it is restored."
base = "Base save"
base_meta = "{size} · {parts} parts"
reset_all = "Reset everything to the base"
filter_placeholder = "Filter parts"
only_replaced = "Only replaced"
count = "{shown} of {total} parts"
col_part = "PART"
col_source = "SOURCE"
col_value = "VALUE"
from_base = "Base save"
replaced_badge = "replaced"
older_badge = "older game version"
group_mixed = "mixed"
group_replaced = "{replaced} replaced"
picker_placeholder = "Filter backups"
picker_empty = "No backup matches the filter."
parts_empty = "No part matches the filter."
loading = "Reading backup …"
label_placeholder = "Label of the new backup"
create_button = "Create backup"
no_backups = "There is no backup to compose from yet."
```

`de.toml`, the same keys:

```toml
tab = "Save zusammenstellen"
tab_backups = "Backups"
intro = "Ein Basis-Save liefert alle Bestandteile. Einzelne Bestandteile lassen sich aus anderen Backups holen. Das Ergebnis wird als neues Backup abgelegt — die echten Spielstände bleiben unberührt, bis es wiederhergestellt wird."
base = "Basis-Save"
base_meta = "{size} · {parts} Bestandteile"
reset_all = "Alles auf Basis zurücksetzen"
filter_placeholder = "Bestandteile filtern"
only_replaced = "Nur ersetzte"
count = "{shown} von {total} Bestandteilen"
col_part = "BESTANDTEIL"
col_source = "QUELLE"
col_value = "STAND"
from_base = "Basis-Save"
replaced_badge = "ersetzt"
older_badge = "ältere Spielversion"
group_mixed = "gemischt"
group_replaced = "{replaced} ersetzt"
picker_placeholder = "Backups filtern"
picker_empty = "Kein Backup passt zum Filter."
parts_empty = "Kein Bestandteil passt zum Filter."
loading = "Backup wird gelesen …"
label_placeholder = "Etikett des neuen Backups"
create_button = "Backup erzeugen"
no_backups = "Es gibt noch kein Backup, aus dem sich etwas zusammenstellen ließe."
```

Then write `compose_page::show`. Follow `saves_page.rs` line for line —
it is the same card, the same column head, the same scroll area — with
these differences:

- **Tab strip** in `saves_page::show`, above the heading body: two
  `widgets::button`s with `ButtonStyle::ghost()`, the active one drawn
  with the accent border. Each pushes `Action::ShowSavesTab(...)`. Then
  `if app.saves_tab == SavesTab::Compose { return compose_page::show(app,
  ui, actions); }` after the tab strip and the blocked banner, so both
  tabs keep the banner.
- **Enablement.** `let usable = app.task.is_none();` — and *not*
  `app.saves_blocked.is_none()`, unlike the Backups tab. Composing reads
  backups and writes a backup; it never touches the game's save
  directory. Put that sentence in the code as a comment, because it is
  the one place this page deliberately differs from its neighbour.
- **Columns:** `[Column::Flexible, Column::Fixed(210.0),
  Column::Fixed(120.0), Column::Fixed(34.0)]` — part, source, value,
  reset.
- **Empty state:** `app.backups.is_empty()` →
  `super::empty_hint(ui, &t!("gui.compose.no_backups"))` and return.
- **Base row:** the current base's `human_time`, its `archive_size` and
  `app.compose.parts.len()` through `gui.compose.base_meta`, a button
  opening `Action::OpenComposePicker(Picker::Base)`, and the
  "reset everything" button pushing `Action::ResetComposition`.
- **Error banner:** when `app.compose.error` is `Some`, draw the same
  box `saves_page::blocked_banner` draws, with the message and no
  buttons.
- **Rows:** `for row in app.compose.rows(&app.compose.parts)`, a group
  header drawing the triangle, `summary::group_name(group)`, the count,
  `gui.compose.group_replaced` when `replaced > 0`, and its own source
  button; a part row drawing the id, the file below it in `mono(11.0)`,
  the badges (`replaced_badge` when `source.is_some()`, `older_badge`
  when `app.compose.is_older(&part)`), the source button, the value from
  `summary::summarize` against the part's source documents (empty when
  the source is not read yet or the group has no figure), and the reset
  `✕` pushing `Action::ResetPart(id)`.
- **The picker popover:** exactly the pattern `mods_page.rs` uses for
  the launch menu — an `egui::Area` at the button's rect with a filter
  field pushing `Action::SetPickerFilter` and one row per backup, each
  pushing `PickComposeBase` / `PickPartSource` / `PickGroupSource`
  depending on `app.compose.picker`. The base itself is the first row of
  a part's and a group's picker, labelled `gui.compose.from_base`: that
  is how a part goes back to the base from inside the dropdown.
  `gui.compose.picker_empty` when the filter leaves nothing.
- **While `app.compose.loading` is `Some`:** the affected source button
  shows `gui.compose.loading` instead of the timestamp and is disabled.
- **Footer:** `app.compose.summary()`, the label field
  (`Action::SetComposeLabel`) and the create button
  (`Action::Compose`), enabled when `usable && app.compose.base.is_some()`.
  Above it, `app.compose.version_warning(&app.compose.parts)` when it
  yields something, in the warning colour with `icons::warning`.

- [ ] **Step 4: Run the tests and look at it**

Run: `cargo test && cargo clippy --all-targets`
Expected: PASS, no warnings.

Then run it and use it: `cargo run`, Savegames → "Save zusammenstellen".
Check by eye: all groups collapsed on opening, a group opens and closes,
the filter finds a class by name, "only replaced" empties the table
until something is replaced, picking a source marks the row, the value
column shows levels for classes and nothing for loadouts, and the button
writes a backup that then appears in the Backups tab. Switch the
language in the settings and look at the tab again.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/gui/compose_page.rs crates/app/src/gui/saves_page.rs \
        crates/app/src/gui/mod.rs crates/core/i18n/en.toml crates/core/i18n/de.toml
git commit -m "$(cat <<'EOF'
feat(gui): the tab that composes a backup from several backups

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

## What this plan does not do

- No display names for the 163 part ids. A row shows the id and the
  file, as the catalogue has them.
- No Steam user profile picker — out of scope in the savegame merge
  spec and unchanged here.
- No verification that the game accepts a composed save. That is open
  question 1 of the savegame merge spec and needs a deliberate run
  against the installed game.
- No automated test of the drawing itself. There is no egui harness in
  this repository; the logic is tested in `compose.rs` and the page is
  checked by eye, which is how every other page here is.

# Composing a backup in the interface

Date: 2026-09-21
Status: approved, ready for an implementation plan

## What this is

`save compose` exists on the command line and the core API below it is
complete: a catalogue of the parts a backup offers, a merge that moves
one part from one savegame into another, and a composition that is
written as a new backup. What does not exist is a way to reach any of
it from the window.

This adds the "Save zusammenstellen" tab of the mockup handed over from
Claude Design (`docs/design/design-with-backup-feature.zip`, the file
`SM2 Mod Loader GUI v2 modern.dc.html` inside it — the copy unpacked in
`docs/design/` is the older one without this feature). Nothing below the
interface changes: this sits on `savedata::catalogue`,
`savedata::compose` and `saves::read_files` exactly as the command line
does.

## What the interface has to work with

Numbers from the fifteen real backups on the author's machine, measured
before this was designed, because two of them decided the architecture:

- A backup holds 21 SSF1 files, together **3.6 MB of JSON**. Held as
  `serde_json::Value` that is roughly 15-20 MB per backup, so reading
  every backup up front would cost a quarter of a gigabyte and grow
  with every backup made.
- Decoding one backup costs about **20 ms**, including the process
  start. Reading one on demand is imperceptible.
- A backup offers **163 parts in 14 groups**: 45 weapons, 29 loadouts,
  29 class levels, 29 loyalist armour sets, 13 heraldries, 10 chaos
  armour sets, and eight groups that are a single part each.
- A part is named by its raw game id (`class_level:PVE_TANK`). The
  table derives a readable name from it — see "The names in the table"
  below — but never invents one.
- `summary::summarize` yields a figure for three groups only — class
  level, weapon mastery, heraldry victories. For the other eleven the
  STAND column stays empty, deliberately: a field an older build did
  not record must not be shown as a zero. Class levels are stored from
  zero and shown from one, so the figure is the stored number plus one
  — the level the game itself displays. The other two are counts and
  are reported as they stand.

## The shape of the tab

`Section::Saves` keeps one page. `saves_page::show` gains the tab strip
from the mockup — "Backups" and "Save zusammenstellen" — and dispatches
to itself or to `crates/app/src/gui/compose_page.rs`.

Two new modules, split the way the rest of the interface is split:
`gui/compose.rs` holds the state and every decision that can be made
without a screen — which rows are visible, which parts are replaced,
what the footer says — and is where the tests live. `gui/compose_page.rs`
draws it and holds no logic worth testing.

The tab is usable while `saves_blocked` is set. Composing never touches
the game's save directory: it reads backups and writes a backup. The
banner above the page stays, because restoring is still blocked, but a
user who imported backups from another launcher and has no game
installed can still merge them. This departs from the mockup, which
gates the compose controls on `savesBlocked`.

From top to bottom, as the mockup has it:

1. An explanatory line: the base supplies everything, single parts can
   be taken from other backups, the result is a new backup and the live
   savegames are untouched until it is restored.
2. The base picker — a dropdown listing every backup by timestamp and
   label — with the base's size and the number of parts it offers
   beside it, and a "reset everything to the base" button.

   The mockup puts a game version there. A backup does not have one:
   every `.cfg` carries its own `systemVersion`, and a save written
   across a game update can hold several. The version comparison
   therefore happens per part, against the same file in the base, which
   is also exactly what `merge` raises.
3. The part table with its filter field, a "only replaced" toggle and a
   count.
4. The mixed-version warning, when a chosen source has an *older* game
   version than the base — `source_version < base_version`, not
   "different". See "The version comparison is one-sided" below for
   what that leaves out.
5. The footer: a summary of what has been chosen, the label field for
   the new backup, and "Backup erzeugen".

### The part table

163 rows in one flat list is what the mockup shows and it does not
survive contact with a real backup. The table keeps its columns
(BESTANDTEIL, QUELLE, STAND) and gains collapsible group headers. All
groups start collapsed, so the table opens as 14 rows, not 163.

A group header shows the group's name, how many parts it holds, how
many of them are replaced, and its own source dropdown: picking a
backup there assigns that source to every part of the group at once,
which is what someone wanting "all my weapons from the September
backup" actually means. When the parts of a group have different
sources, the header's dropdown reads "gemischt".

A part row shows the id, the file below it, its source dropdown, its
figure if it has one, and a reset button. A replaced part carries an
"ersetzt" badge; one whose source has an older game version than the
base carries "ältere Spielversion".

The eight single-part groups (tutorial, story, economy, ...) are drawn
as plain rows without a header — a collapsible section holding one row
is noise.

While the filter field holds text or "only replaced" is on, a group
with matching parts is drawn open regardless of its collapsed state,
and the collapsed state is remembered rather than overwritten: clearing
the filter puts the table back the way the user left it. Without this a
filter that matches only parts inside collapsed groups looks like a
filter that does nothing.

### The names in the table

`class_level:STORY_GADRIEL` is not a label. `summary::display_name`
derives one from the id, by taking it apart rather than replacing it:

- the group prefix goes, because the heading above the row says it
- a leading `PVE_`/`PVP_`/`STORY_` moves to the end as ` (PvE)`,
  ` (PvP)`, ` (Story)`
- a known weapon category (`arifle`, `brifle`, `equipment`, `hgun`,
  `hwpn`, `melee`, `pc`, `pwpn`, `shotgun`, `smg`) goes, repeatedly —
  two ids carry two of them
- what is left loses its underscores and gets a capital per word

So `Gadriel (Story)`, `Thunder Hammer`, `Character Mod 2 (PvE)`. The
eight whole-file groups keep their translated group name, which is
already the right wording.

Nothing here is translated and nothing is a catalogue entry, and both
are deliberate. A table of 163 invented names would have to claim which
class `CHARACTER_MOD_2` is; a wrong claim beside a level sends someone
to the wrong backup, which is worse than an awkward name. The
comparison the launcher can honestly offer is the game's own word. For
the same reason an unknown prefix is kept rather than dropped: a
category added by a later build must not be mistaken for part of a
name and swallowed.

The raw id stays under the name in the table. It is what
`save compose --part <id>=<timestamp>` wants typed, and it is what lets
a reader check the derivation. The filter searches both.

### The version comparison is one-sided

Both the "ältere Spielversion" badge and the warning line under the
table fire on `source_version < base_version` only. A part taken from a
*newer* build than the base gets no badge, no warning and no mention in
the footer.

That asymmetry has a sharp edge, because `merge::apply` writes the
composed file at `max(base, source)`: a part from a newer build silently
lifts the composed file's `systemVersion` above the base's. The warning
promises the opposite for the case it does cover — "Er wird auf der
Version des Basis-Saves geschrieben" — and for a newer source that
sentence would simply be false.

This is known and unhandled. Narrowing the comparison to "older" was
deliberate: the badge exists to warn that a part is about to be carried
forward into a newer save, which is the direction that loses data. What
to say about the other direction — whether to warn, and with which
wording — is a product decision, not a defect to be fixed in passing.

## State

One struct, so `App` does not grow a dozen fields:

```rust
struct ComposeUi {
    /// `created_at` of the base backup.
    base: Option<String>,
    /// Part id -> `created_at` of the backup it comes from.
    sources: BTreeMap<String, String>,
    /// `created_at` -> the decoded files of that backup.
    decoded: BTreeMap<String, Arc<Documents>>,
    /// Which backup is being read right now.
    loading: Option<String>,
    open_groups: BTreeSet<&'static str>,
    part_filter: String,
    only_replaced: bool,
    picker: Option<Picker>,
    picker_filter: String,
    label: String,
    error: Option<String>,
}
```

`sources` is exactly the argument `compose::compose` takes, keyed and
valued the same way, so there is no translation step between what the
window shows and what the core is asked to do.

Changing the base clears `sources`. That is not a convenience: a part
id the new base does not offer would be refused by `compose`, and it
would be refused at the click on "Backup erzeugen" rather than at the
click that caused it.

## Reading

Two paths, kept apart on purpose.

**Reading** — decoding the base and any chosen source — does not use
`App.task`. That slot disables the whole interface and occupies the
status bar, which is right for "make a backup" and wrong for "read a
source". `ComposeUi` gets its own channel: a thread runs
`saves::verify`, `saves::read_files` and `catalogue::documents`, sends
the result back, and `loading` marks the affected row meanwhile. The
rest of the page stays usable.

The verify is not optional and the order is the one `cli::part_lines`
already established: `read_files` checks the bytes against nothing, the
manifest is what says they are still the ones that were backed up.
Without it a damaged backup offers a part list that looks perfectly
ordinary, and `compose`, which does verify, refuses the very ids the
window just offered.

A decoded backup stays in `decoded` for as long as the tab lives. With
one base and the one to three sources a composition typically uses,
that is 30-60 MB.

**Writing** — `compose::compose` — goes through `tasks::spawn` like
`CreateBackup`, with a new `Outcome::Composed(Result<BackupEntry,
String>)`. On success: reload the backup list, raise a toast naming the
new backup, and switch to the Backups tab, where the result now sits
and where it is later restored.

## Actions

`ShowSavesTab(SavesTab)`, `PickComposeBase(String)`,
`PickPartSource { part: String, backup: String }`,
`PickGroupSource { group: &'static str, backup: String }`,
`ResetPart(String)`, `ResetComposition`, `SetPartFilter(String)`,
`ToggleOnlyReplaced`, `ToggleGroup(&'static str)`,
`OpenComposePicker(Picker)`, `CloseComposePicker`,
`SetComposeLabel(String)`, `Compose`.

## Errors

A failed read puts the affected row back on the base and shows the
message as a banner above the table, not as a toast: it belongs to a
decision the user is making right now and must not fade after four
seconds. A failed read of the base leaves the tab empty with the same
banner.

A failed `compose` shows the core's own message unchanged —
`PartGivenTwice`, `AmbiguousPartInSource`, `SaveDataDefect` and the
others carry their explanation already. The composed state stays as it
is, so the one disputed source can be changed and the button pressed
again.

## Language

New keys under `gui.compose.*` in `en.toml` and `de.toml`. Group names
and figures come from `savedata::group_name` and `summary::summarize`
and are translated already.

No new label ends up on a disk. The label of the composed backup is
typed by the user and is empty when they type nothing, exactly as
`save compose` behaves.

## Tests

The logic lives in pure functions on `ComposeUi` and the drawing stays
thin — the same split `gui::commands` uses, which is tested without an
egui harness. Written test-first.

- `replacements()` yields exactly the pairs `compose::compose` expects,
  and is empty when everything comes from the base
- changing the base clears the chosen sources
- `rows()` under a filter, under "only replaced", and with a group
  collapsed
- a group header counts its replaced parts and reads "gemischt" when
  two sources are in play within one group
- the mixed-version warning appears exactly when a chosen source has an
  older game version than the base, and stays silent for a newer one
- the summary line ("2 Bestandteile aus 1 anderen Backup") in both
  languages, holding `language_test_lock()`

## What this does not do

- No Steam user profile picker. Out of scope in the savegame merge
  spec, and unchanged here.
- No invented names for the 163 part ids, and no claim about which
  class an internal name stands for. The table derives its labels from
  the ids themselves; see "The names in the table".
- No preview of what a composition would do beyond the per-part figure
  the catalogue already yields.
- No verification that the game accepts a composed save. That is open
  question 1 of the savegame merge spec and needs a run against the
  installed game.

# Composing a savegame backup from several backups

Date: 2026-09-21
Status: approved, ready for an implementation plan

## What this is

Someone has a dozen savegame backups and wants one save that takes some
of its parts from one and some from another: the Bulwark's levels out of
the backup from May, the Tactical's armour sets out of the one from
September. Today a backup can only be restored whole.

This adds a *composition*. One backup is the base and supplies
everything; part by part, another backup can be named as the source of
that one part. The result is written as a new backup. The live savegame
is not touched — a composition is put into the game by restoring it,
through the path that already exists and already verifies itself.

The interface follows a mockup handed over from Claude Design; only
what lies below the interface is specified here.

## The container format

This work rests on the savegame format, which had to be worked out
first. Every `.cfg` below the save directory's `config` folder:

    offset  size  meaning
    0       4     magic "SSF1"
    4       8     length of the payload, u64 little endian
    12      8     length of the decoded JSON, u64 little endian
    20      32    md5 of the payload, lowercase hex ASCII
    52      1     encoding
    53      ...   payload

The encoding byte and its names come from the game binary:
`CONFIG_ENCRYPTION_UNENCRYPTED` (0), `deflate` (1), `deflateXorSkip`
(2). Every file the retail game writes uses 2.

`deflateXorSkip` is a self-inverse run-length mask over an ordinary
zlib stream. The four-byte key is `9d 64 aa ec`, written by a static
initialiser in the retail executable (`movl $0xecaa649d`):

    ki = 0; n = 0; limit = 0
    for each byte b:
        if n == limit:            # control byte, passes through
            limit = b; n = 0
            emit b
        else:
            emit b ^ key[ki]
            ki = (ki + 1) % 4
            n += 1

The first byte of a payload is therefore always a control byte, which
is why every file begins `78 01`: the `78` is the zlib CMF byte passing
through untouched, and `01` is `9c ^ 9d`. Below the mask sits a plain
zlib stream with the default header `78 9c`.

Because the mask is its own inverse, encoding and decoding are the same
function. Checked against 378 real files out of 14 backups: every one
decodes to JSON, and encoding that JSON again reproduces all 378 files
byte for byte.

The key is a constant in one game build. A game update may change it,
and then decoding fails — which has to stay a clean refusal that turns
the feature off, never a half-decoded file.

## Goals

- Compose a new backup from a base and per-part sources.
- Never write outside the backup directory.
- Be fully usable without the interface, from the command line.
- Refuse rather than guess wherever data does not fit together.

## Non-goals

- Editing values. This moves existing data around; it does not raise a
  level or unlock an item.
- Restoring a composition. That is `restore`, unchanged.
- Encodings 0 and 1. They are rejected with a named error.
- Choosing between the two Steam user profiles in the prefix. The
  mockup raises it; it is a separate feature and not part of this.

## Decisions

**A new module `savedata`, beside `saves.rs` rather than inside it.**
`saves.rs` is already the largest file in the crate and its subject is
the safety of filesystem ordering. The subjects separate cleanly:

    savedata/ssf1.rs       container: bytes <-> JSON. Nothing else.
    savedata/catalogue.rs  what a part is: file, selector, summary
    savedata/compose.rs    base + replacements -> a set of files
    saves.rs               that set of files -> a new backup

`ssf1` knows nothing of backups, `catalogue` nothing of the
filesystem, `compose` nothing of the archive format.

**The catalogue is flat and free of overlaps.** If both "class levels"
(a whole file) and "level: Bulwark" (a piece of it) could be chosen,
two selections could contradict each other. Every part is therefore as
fine as the data allows, and no part contains another. The eight
categories of the mockup survive as a group label on each row, which
the interface may fold by later.

**Granularity follows the data, not the file.** The game stores
progression per class, per weapon and per character, so that is what a
part is. This is the point of the whole feature.

**Part ids are discovered, not hard-coded.** Which groups exist — which
file, which container path, which key field — is a static table. The
ids below a group (45 weapons, 29 classes) are read out of the backup
being looked at. A game update that adds a weapon then needs no code
change, and a backup from an older build simply offers fewer parts.

**Only files that receive a replacement are re-encoded.** Everything
else is copied byte for byte out of the base. This makes one property
unconditional: a composition without a single replacement is the base,
byte for byte. It also keeps the six settings files out of the
re-encoding path, where serialising floats would otherwise shorten
`0.69999998807907104` to `0.699999988079071` — the same number, but a
needless difference.

**`systemVersion`: the maximum wins.** Each file carries one, and the
engine knows the complaint that provided data is "older than the data
in config". The counter is read as "this state is at least this new",
so a merged file takes the highest value among the base and every
contributing source. This is a reasoned assumption, not a certainty,
and it is the first thing to verify against the running game.

**`json_version`: a mismatch is refused.** The schema version sits on
the individual node, not on the file. If the base and the source
disagree for a part, that part is rejected by name instead of being
merged. This is the implementable form of the mockup's "older game
version" badge: it tests the data itself rather than a game version
that no backup records.

**The result is a new backup, written through the existing machinery.**
The merged files are assembled in a temporary directory and handed to
the ordinary backup path, so archive, manifest, hashes and
fsync-before-rename are unchanged and untouched by this feature.

**The composition is recorded in the manifest**, as an optional field
next to `label`, which already shows how (`#[serde(default)]`):

    composed_from: Option<Composition>   // base backup + source per part

A composed backup is then self-describing: the list can mark it, and
later it is still possible to see what it was made of.

## The part catalogue

| Group | File | Selector | Ids |
|---|---|---|---|
| Class level | `user_progression.cfg` | `/UserProgression/UserMastery/masteryStates/<id>` | 29, `PVE_*`/`PVP_*` |
| Armour sets | `character_customization_progression.cfg` | `/CharacterCustomizationProgression/CharacterCustomization/teamStates/<team>/outfitStates/<id>` | LOYALIST 29, CHAOS 10 |
| Weapon mastery | `weapon_progression.cfg` | `/WeaponProgression/WeaponMastery/weaponStates/<id>` | 45 |
| Heraldry | `heraldry_progression.cfg` | `/HeraldryProgression/Armor/characterArmorInfos/<id>` | 13, unprefixed |
| Loadouts | `loadouts.cfg` | element of `/Loadouts/Sets/loadoutSets` whose `masteryUid` is `<id>` | 29 |
| Whole file | `challenge_progression`, `story_progression`, `economy`, `pve_state`, `hordemode_state`, `tutorial`, `mutator_challenges`, `achievements` | the system object | one each |

Not offered, and always copied from the base: `user_settings`,
`user_defined_settings`, `shared_user_settings`, `agreements`,
`user_reports`, `platform_syncable`, `achievements_common`. They are
settings and bookkeeping, not progression.

The id namespaces differ between files — `masteryStates` says
`PVE_TANK` where `characterArmorInfos` says `TANK`. The catalogue
therefore names ids per group and never assumes one namespace across
files. Grouping the four parts that belong to one class into a single
row is a matter for the interface, not for the catalogue.

A part addresses `config/<name>.cfg`, relative to the save directory —
the anchoring `saves::config_anchored_layout` already establishes.
Backups written before that rule hold a second, identical copy of all 21
files beside `config/`; those are dead weight the game ignores. They are
copied along unchanged and are never read as a source, so a composition
neither loses them nor lets them contradict the real ones.

Three kinds of selector cover all of it:

    Whole                         the system object of a file
    Member(pointer)               an object member, by JSON pointer
    ListItem(pointer, key, id)    the array element whose `key` is `id`

`ListItem` exists because loadouts are an array. Addressing them by
position would silently take the wrong one when two saves hold
different numbers of sets; `masteryUid` is the identity that actually
holds.

## Composing, step by step

1. Verify base and every named source backup (the existing `verify`).
2. Read the base's file set. Files with no replacement are already
   final.
3. For every file that has at least one replacement: decode the base
   file and each contributing source file to JSON.
4. Per part, compare `json_version` at the selected node. On a
   mismatch, abort and name the part.
5. Replace the selected nodes in the base's JSON with the source's.
6. Set the file's `systemVersion` to the maximum over base and sources.
7. Serialise, encode, and decode the result again, comparing it against
   the JSON that was intended. Only then does the file count as final.
8. Hand the complete set of files to the backup path, with the label.

Nothing is written before step 8, so a failure anywhere leaves no trace
at all — not even a half-written backup to clean up.

## Errors

New variants follow the existing `*Defect` pattern of `error.rs`:
unreadable container (not `SSF1`, unsupported encoding, md5 or length
mismatch, truncated), unusable content (invalid JSON, a pointer that
does not resolve, a list element that is missing), and unusable
composition (`json_version` mismatch, naming part and both backups; a
source backup that does not contain the file at all). Every message
belongs in the catalogue, like every other user-facing sentence.

## Summaries

The mockup shows a "Stand" column: one short figure per part, per
backup — a level, a count, a percentage. The rule per group is
declarative in the catalogue (read this pointer, count those entries);
the wording comes from `t!`. These are ordinary interface texts, not
the special case of the `[label]` entries that end up on a disk, so
they follow the normal rules.

Computing them means decoding the file, so the result is cached per
backup for as long as the list is shown. The interface asks for what it
draws; nothing decodes 18 backups up front.

## Command line

    lina-sm2 save parts [--backup <id>]
    lina-sm2 save compose --base <id> [--part <part>=<backup>]... [--label <text>]

Backups are addressed the same way the other `save` subcommands address
them. `parts` lists the catalogue with the summaries of one backup;
`compose` does the work.

`compose` writes into the backup directory and therefore takes the
instance lock. `cli::requires_exclusive_access` has no `_` arm, so both
subcommands force that decision at compile time. Each new argument
needs its `cli.save.compose.arg.*` key, or
`every_command_and_argument_has_a_key` fails.

## Dependencies

- `flate2` with the pure-Rust backend for the zlib layer. No C
  toolchain, which matters because the Windows half of CI already has
  enough of that.
- `md-5` (RustCrypto) for the container checksum, matching `sha2`,
  which is already in the tree.
- `serde_json` gains `preserve_order`, so a re-encoded file keeps its
  member order instead of being alphabetised. This is a workspace-wide
  feature and reaches every other user of `serde_json`; if that turns
  out to bite, sorted keys are an acceptable fallback, since the game
  parses JSON and does not care.

## Tests

Written first, as the rest of the suite was. Fixtures are generated by
the project's own encoder — no real savegames go into the repository.

- Container: round trip; a corrupted md5, a wrong length, a foreign
  magic and encodings 0 and 1 are each rejected.
- Catalogue: the parts are free of overlaps (a test, not a promise);
  ids are discovered from the data; a file missing from a backup
  offers no parts instead of failing.
- Composition: a replacement lands where it belongs and nowhere else;
  `json_version` mismatch is refused; `systemVersion` becomes the
  maximum; files not covered by any part survive unchanged; a file the
  catalogue does not know at all survives unchanged.
- The property that matters: composing with no replacement reproduces
  the base byte for byte.
- CLI: `compose` holds the lock, `parts` is readable, both speak the
  catalogue.

## Open questions

1. **`systemVersion` as a maximum** is reasoned, not proven. Before
   release: compose a save, load it in the game, confirm the merged
   data survives.
2. **Game version per backup.** The mockup shows one; no manifest
   records it. Recording it from now on is cheap (`game_version:
   Option<String>`, empty for everything that exists today), and the
   real safety check stays `json_version`.
3. **Server-side validation.** Whether online play checks progression
   against a server is unknown from here. The feature only writes
   backups; restoring one stays the user's decision, as it is today.

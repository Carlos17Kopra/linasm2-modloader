# SM2 Mod Loader

A mod loader for Space Marine 2 on Linux. Rust workspace, two crates:

- `crates/core` (`sm2-core`) — all domain logic, no UI. MSRV 1.85.
- `crates/app` (`sm2-modloader`) — one binary that is both GUI and CLI. Started
  with no arguments it opens the egui interface, with arguments it runs the
  command line. MSRV 1.95 (egui 0.36 requires it).

## Language

**Code and comments are English. User-facing text lives in the message
catalogue, never in the code.**

- English: `//`, `///`, `//!`, test names, `assert!` messages — anything
  only a developer reads.
- The catalogue: every sentence a user sees. `crates/core/i18n/en.toml`
  is the source of truth, `de.toml` the translation; both carry exactly
  the same keys. Reach for a text with `t!("area.key")`, or
  `t!("area.key", name = value)` when it has placeholders.

Adding a language: copy `en.toml`, translate it, add a `Language`
variant and name it in `Language::ALL`. `cargo test` then says whether
the translation is complete.

Three tests keep this honest and are worth knowing about before you add
a string: every language has exactly the English key set, every key used
in the sources exists, and no German sentence is left in the code
(`crates/app/tests/no_german_literals.rs`).

Three identifiers stay German on purpose, the rule's only carve-out:
`vanilla::VANILLA_SNAPSHOT_PREFIX` (`"vor Vanilla-Start"`), the sibling
`"vor Modded-Start"` backup label next to it in `gui/commands.rs`, and the
safety-backup label `"vor Wiederherstellung"` in `saves.rs`. All three are
matched by prefix and already written into existing profile and backup
names on disk, so translating them would rename data that is already
there — they are shown to the user even in an English interface. Whether
to eventually split the stored form from the displayed form is an open
question left to the project's owner, not decided here. If you meet one
of these strings elsewhere, it is this exception, not a leftover.

Comment prose is wrapped at 78 columns including the `///` prefix.

## Comment style

Comments here explain *why*, not *what*. A comment that restates the code
below it is noise; a comment that names the failure mode a guard prevents, or
the alternative that was rejected and what would have broken, earns its place.
Match that when adding code — especially around `saves.rs`, where the ordering
of filesystem operations is the entire safety argument.

## Safety rules that the code depends on

- Savegames live in a Proton prefix that Steam Cloud can overwrite at any
  moment. `saves::restore` always takes and verifies its own backup first;
  that is not optional and not configurable.
- Archive and manifest are written fsync-before-rename, and every filesystem
  sequence is ordered so that a crash between two steps leaves a readable
  state — never a half-written one.
- Symlinks inside a save or extraction directory are never followed.

## Working on it

    cargo test                  # 287 tests across both crates
    cargo clippy --all-targets  # kept clean
    cargo run                   # GUI
    cargo run -- <subcommand>   # CLI

Write tests first. The existing suite was built that way and the failure modes
it covers (collisions in the same second, corrupt archives, zip-slip, symlink
cycles) are the reason this tool can be trusted with real save data.

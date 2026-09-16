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

Five tests keep this honest and are worth knowing about before you add
a string: every language has exactly the English key set
(`every_language_has_exactly_the_english_keys`), every translation only
uses placeholders the English original also has
(`every_translation_uses_only_the_placeholders_of_the_original`), every
key used in the sources exists, every clap command and argument has a
key (`every_command_and_argument_has_a_key` — a new clap argument needs
a `cli.<path>.arg.<name>` key, or this one fails), and no German
sentence is left in the code (`crates/app/tests/no_german_literals.rs`).

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

    cargo test                  # 292 tests across both crates
    cargo clippy --all-targets  # kept clean
    cargo run                   # GUI
    cargo run -- <subcommand>   # CLI

Write tests first. The existing suite was built that way and the failure modes
it covers (collisions in the same second, corrupt archives, zip-slip, symlink
cycles) are the reason this tool can be trusted with real save data.

The active language is one process-wide static (`CURRENT` in
`crates/core/src/i18n.rs`), and `cargo test` runs a crate's tests
concurrently by default. Any test anywhere in the workspace whose
assertion depends on which language is active — not just on
`set_language`/`lookup` in isolation, but on the wording a call under
test actually produces — must hold the matching lock for as long as that
dependency lasts: `sm2_core::i18n::language_test_lock()` inside
`crates/core`, `crate::app_state::language_test_lock()` inside
`crates/app` (a second lock there because the first one is `pub(crate)`
to `sm2-core` and so unreachable from the other crate's test binary).
Skip it and two such tests running side by side can flip the language
out from under each other mid-assertion.

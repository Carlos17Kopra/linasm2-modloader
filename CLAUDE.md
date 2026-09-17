# LiNa SM2 - Mod Launcher

A mod launcher for Space Marine 2 on Linux ("LiNa" = Linux native). Rust
workspace, two crates:

- `crates/core` (`sm2-core`) — all domain logic, no UI. MSRV 1.85.
- `crates/app` (`lina-sm2`) — one binary that is both GUI and CLI. Started
  with no arguments it opens the egui interface, with arguments it runs the
  command line. MSRV 1.95 (egui 0.36 requires it).

The name lives in exactly one place, `crates/core/src/branding.rs`:
`APP_NAME` ("LiNa SM2 - Mod Launcher") with the two halves it is composed
of, and `APP_SLUG` ("lina-sm2") for the binary, the XDG directories and the
Wayland app id. Nothing spells either of them out a second time.

Up to and including 0.1.0 the program was called "SM2 Mod Loader" and used
the slug `sm2-modloader`. `paths::app_dirs` moves an installation left
under that name over on the first start (`migrate_legacy_dir`); the old
slug stays in `branding.rs` as `LEGACY_APP_SLUG` for exactly that.

## Language

**Code and comments are English. User-facing text lives in the message
catalogue, never in the code.**

- English: `//`, `///`, `//!`, test names, `assert!` messages — anything
  only a developer reads.
- Not the catalogue: the product's name. A proper name has to read the
  same in every language, and a catalogue entry invites a translator to
  adapt it — so it is a constant in `branding.rs` instead, with a test
  that keeps its forms from drifting apart.
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
  state — never a half-written one. One half of that is weaker on Windows:
  `saves::sync_dir` flushes the containing directory on Unix, and is a
  no-op there, because a directory cannot be opened with `File::open` at
  all (ERROR_ACCESS_DENIED) and Windows offers no equivalent flush. The
  write-flush-rename of the archive file itself is unchanged on both.
- Symlinks inside a save or extraction directory are never followed.

## Platforms

`core::platform::Platform` is the entire platform-dependent surface —
`unix.rs` and `windows.rs` implement it, `Current` picks one, and nothing
above it carries a `cfg`. Adding a platform means adding one file there.

The Windows side has never run on a machine with the game installed. Two
things are therefore uncovered rather than merely unverified, and both are
marked at the tests that had to be gated:

- Everything reached through `save_dir` — thirteen tests across
  `core::paths`, `core::tests::workflow`, `app::app_state` and `app::cli`
  carry `#[cfg(unix)]` for this one reason. The fixtures build the save
  directory inside a Proton prefix under a temporary directory, which is
  where `user_profile_root` looks on Linux; on Windows it returns the real
  `%USERPROFILE%`, which a test must not write into. Giving that one
  function a way to be redirected in tests would bring all thirteen back
  on Windows, and is an open decision, not an oversight.
- The symlink guards in `saves` and `import`. The guards themselves rest on
  `symlink_metadata` and are platform-neutral; only creating a symlink as a
  fixture needs privileges on Windows.
- `backup`'s refusal of a save file with a backslash in its name. The
  hostile file cannot be created on Windows at all, where a backslash
  separates path components. The same guard for archives written elsewhere
  (`verify_rejects_backslash_components_in_entry_names`) runs on both.

When gating a test for one of these, say which of the three it is. A bare
`#[cfg(unix)]` reads like the behaviour is Unix-specific, and here it
almost never is — it is the fixture that cannot be built.

## Working on it

    cargo test                  # 312 tests across both crates
    cargo clippy --all-targets  # kept clean
    cargo run                   # GUI
    cargo run -- <subcommand>   # CLI

Write tests first. The existing suite was built that way and the failure modes
it covers (collisions in the same second, corrupt archives, zip-slip, symlink
cycles) are the reason this tool can be trusted with real save data.

## Packaging

Users install with a one-liner that pipes `install.sh` into `sh`; the
README carries it. The three pieces:

- `install.sh` (repository root) — downloads the release asset, verifies it
  against that release's `SHA256SUMS`, installs binary, desktop entry and
  icon under `$HOME`. POSIX sh, no bash. Also `--uninstall`, `--force`,
  `--version`.
- `packaging/` — the desktop entry and the icon that go into the archive.
  `StartupWMClass` there has to stay equal to `APP_SLUG`, or the running
  window is not connected to the menu entry.
- `.github/workflows/release.yml` — a pushed tag `vX.Y.Z` builds on
  ubuntu-22.04 (glibc 2.35: the oldest base the binary should still start
  on) and uploads `lina-sm2-X.Y.Z-x86_64-linux.tar.gz` plus `SHA256SUMS`.
  The tag has to match the workspace version; the workflow refuses
  otherwise, because `install.sh` compares exactly those two to decide
  whether an update is due.

The installer has its own end-to-end suite, `packaging/test-install.sh`: it
publishes a release into a temporary directory and drives the real script
against it over `file://` URLs. `cargo test` runs it through
`crates/app/tests/install_script.rs`, so it stays in the one command that
says whether this repository is sound. Change the asset naming in one of
the three places and that suite is what tells you about the other two.

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

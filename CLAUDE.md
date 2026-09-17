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

There is no carve-out: `no_german_literals.rs`'s `EXEMPT_LITERALS` is
empty. The three names the launcher gives its own profiles and backups —
the vanilla snapshot, the backup before a launch, the safety copy before
a restore — used to be German literals and are catalogue entries now,
under `[label]`.

They are the only catalogue entries that also end up on a disk, which
makes them work differently from every other string: each is resolved
once, at the moment the profile or backup is created, and then written.
Nothing rewrites it afterwards, so an entry keeps the wording of the run
that made it, and a language switch leaves what is already there alone —
it is that backup's name now, not interface text. Recognising one again
is therefore `vanilla::is_snapshot_name`'s job, which asks every language
instead of the active one; `i18n::lookup_in` exists for exactly that.
The German wordings are held to their historical form by
`the_german_labels_still_read_as_they_do_on_disks_today` — reword them
and every snapshot made before the change silently loses its "automatic"
badge.

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
- Only one launcher runs at a time. `instance::InstanceLock` takes an
  advisory lock on `instance.lock` in the state directory, and the CLI
  takes it for every command that changes something. Which ones those are
  is decided by `cli::requires_exclusive_access`, exhaustively and without
  a `_` arm, so a new subcommand does not compile until someone has chosen
  a side for it. The guard has to stay bound for the whole run — a
  `let _ = ...` releases it on the spot. A lock that cannot be taken at
  all is a warning, not a refusal; only a lock someone else holds stops
  the program.

## Platforms

`core::platform::Platform` is the entire platform-dependent surface —
`unix.rs` and `windows.rs` implement it, `Current` picks one, and nothing
above it carries a `cfg`. Adding a platform means adding one file there.

The Windows side has been run through by hand on a machine with the game
installed — detection, the mod list, savegame backup and restore, and
starting the game all work there. What that does not do is close the gaps
in the automated suite: three things stay uncovered by tests, for reasons
that are about the fixtures rather than the behaviour, and all three are
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

    cargo test                  # 330 tests across both crates
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
- `packaging/` — the desktop entry and the icons that go into the archive.
  `StartupWMClass` there has to stay equal to `APP_SLUG`, or the running
  window is not connected to the menu entry. Every image in the project
  is derived from `docs/logo.jpg` by `packaging/make-icons.py`: the four
  `icons/lina-sm2-<size>.png` the installer puts into hicolor, the
  `lina-sm2-window.png` the interface builds into the binary for X11 and
  Windows, the `lina-sm2.ico` `build.rs` compiles into the executable,
  and `docs/social-preview.png`. The app icon is a crop of the logo, not
  the logo: at 48 px the wordmark is a smudge and only the helmet still
  reads. Change the logo and that script is what regenerates the rest —
  nothing does it automatically, and `crates/app/tests/windows_icon.rs`
  is what notices if the `.ico` stops being one.
- `.github/workflows/release.yml` — a pushed tag `vX.Y.Z` builds on both
  platforms and uploads `lina-sm2-X.Y.Z-x86_64-linux.tar.gz`,
  `lina-sm2-X.Y.Z-x86_64-windows.zip` and one `SHA256SUMS` covering both.
  Linux builds on ubuntu-22.04 (glibc 2.35: the oldest base the binary
  should still start on). Windows gets a plain ZIP and no installer — an
  unsigned installer only adds a second SmartScreen warning to the one the
  executable already triggers. The tag has to match the workspace version;
  the workflow refuses otherwise, because `install.sh` compares exactly
  those two to decide whether an update is due.
- `.github/workflows/ci.yml` — builds and tests both platforms on every
  push. It is not optional: the Windows half cannot be compiled on a Linux
  machine without a C toolchain for the target (`zstd-sys` and `blake3`
  build C), so this is the only thing that says whether it still builds.

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

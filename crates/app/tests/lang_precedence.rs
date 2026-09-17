//! Regression test for the language ordering in `cli::run`: `--lang`
//! has to win over the language stored in `settings.toml` twice over —
//! once for the command tree built before parsing (so `--help` answers
//! in the right language) and again for the command's own output,
//! printed after `AppState::open()` re-applies the stored setting from
//! disk. Without that second `i18n::set_language` call in `run()`,
//! `--lang en` would be silently undone by `AppState::open()` for
//! everything printed after it.
//!
//! Unix only, and not because the behaviour under test is: the harness
//! steers `directories::ProjectDirs` through the XDG environment
//! variables, and on Windows those are ignored in favour of `%APPDATA%` —
//! the tests would then read and write the real user's configuration
//! directory instead of a sandbox. Porting the harness is a piece of work
//! of its own, not a side effect of the Windows build.
//!
//! This runs the real binary as a subprocess (steered through the XDG
//! environment variables `directories::ProjectDirs` reads), not
//! `cli::run()` in-process: that function reads `std::env::args()`
//! directly and calls `std::process::exit` on error, neither of which a
//! single test binary can do more than once.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// A minimal, valid Space Marine 2 directory — the same shape
/// `sm2_core::paths::GamePaths::from_game_dir` checks for. Nothing below
/// it (Proton prefix, Steam library layout) needs to exist: the `paths`
/// command used below reports their absence as a line of its own instead
/// of failing.
fn write_fake_installation(root: &Path) -> PathBuf {
    let game = root.join("game");
    std::fs::create_dir_all(game.join("client_pc/root/mods")).unwrap();
    game
}

/// Writes `settings.toml` at the path `load_dirs_and_settings` reads
/// (`$XDG_CONFIG_HOME/lina-sm2/settings.toml`), with a stored
/// language distinct from the `--lang` this test's assertions pass.
fn write_settings(xdg_config_home: &Path, game_dir: &Path, language: &str) {
    write_settings_in(&xdg_config_home.join(sm2_core::APP_SLUG), game_dir, language);
}

/// Writes a `settings.toml` into `dir`, whichever directory that is — the
/// migration test below needs the same file under the *previous* name.
fn write_settings_in(dir: &Path, game_dir: &Path, language: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let settings = format!("game_dir = {:?}\nlanguage = \"{language}\"\n", game_dir.display());
    std::fs::write(dir.join("settings.toml"), settings).unwrap();
}

/// Runs the built `lina-sm2` binary with its XDG directories
/// pointed at `tmp`, and returns its stdout.
fn run_modloader(tmp: &Path, args: &[&str]) -> String {
    String::from_utf8(run_modloader_full(tmp, args).stdout).expect("stdout must be valid UTF-8")
}

/// Same as `run_modloader`, but returns the whole `Output` — for the tests
/// below that also need stderr and the exit status.
fn run_modloader_full(tmp: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_lina-sm2"))
        .args(args)
        .env("XDG_CONFIG_HOME", tmp.join("config"))
        .env("XDG_DATA_HOME", tmp.join("data"))
        .env("XDG_STATE_HOME", tmp.join("state"))
        .output()
        .expect("failed to run the lina-sm2 binary")
}

#[test]
fn lang_flag_overrides_the_stored_setting_for_the_commands_own_output() {
    let tmp = tempfile::tempdir().unwrap();
    let game = write_fake_installation(tmp.path());
    write_settings(&tmp.path().join("config"), &game, "de");

    // Baseline this test's real assertion depends on: without `--lang`,
    // `AppState::open()` applies the stored "de" setting, so the
    // command's own output is German.
    let german = run_modloader(tmp.path(), &["paths"]);
    assert!(german.contains("Spiel:"), "stored language must be German by default:\n{german}");

    // With `--lang en`, `run()` sets English before parsing, and has to
    // set it again after `AppState::open()` — which would otherwise
    // silently re-apply "de" from `settings.toml` for everything printed
    // from here on. This is the assertion that catches that regression.
    let english = run_modloader(tmp.path(), &["--lang", "en", "paths"]);
    assert!(english.contains("Game:"), "--lang en must win over the stored setting:\n{english}");
    assert!(!english.contains("Spiel:"), "no German text may leak through:\n{english}");
}

#[test]
fn lang_flag_also_governs_help_text_answered_during_parsing() {
    let tmp = tempfile::tempdir().unwrap();
    let game = write_fake_installation(tmp.path());
    write_settings(&tmp.path().join("config"), &game, "de");

    let help = run_modloader(tmp.path(), &["--lang", "en", "--help"]);
    assert!(help.contains("Mod launcher for Space Marine 2 on Linux"), "{help}");
}

/// Regression test for finding I5: an unrecognised `--lang` code used to
/// come back from `cli_help::language_from_args` as `None`, indistinguishable
/// from no `--lang` at all — so `--lang klingon` silently fell back to the
/// stored setting and exited 0, while the `lang klingon` subcommand already
/// reported the same typo and exited non-zero. Both now have to behave the
/// same way.
#[test]
fn an_unrecognised_lang_flag_is_reported_like_the_lang_subcommand_is() {
    let tmp = tempfile::tempdir().unwrap();
    let game = write_fake_installation(tmp.path());
    write_settings(&tmp.path().join("config"), &game, "de");

    let via_subcommand = run_modloader_full(tmp.path(), &["lang", "klingon"]);
    assert!(!via_subcommand.status.success(), "`lang klingon` must fail");

    let via_flag = run_modloader_full(tmp.path(), &["--lang", "klingon", "paths"]);
    assert!(!via_flag.status.success(), "`--lang klingon` must fail exactly like `lang klingon` does");
    let stderr = String::from_utf8(via_flag.stderr).expect("stderr must be valid UTF-8");
    assert!(stderr.contains("klingon"), "the unrecognised code must be named in the error: {stderr}");
}

/// The rename from "SM2 Mod Loader" to "LiNa SM2 - Mod Launcher" moved the
/// XDG directories from `sm2-modloader` to `lina-sm2`. An installation
/// that predates it keeps everything — settings, profiles, backups — under
/// the old name, and `paths::app_dirs` has to carry that over on the first
/// start under the new one.
///
/// The check runs through the real binary rather than through
/// `migrate_legacy_dir` directly (which `sm2-core` unit-tests on its own):
/// only the binary proves that the move happens early enough, before
/// `load_dirs_and_settings` creates an empty directory under the new name
/// that would block it forever after.
#[test]
fn an_installation_under_the_previous_name_is_carried_over_on_first_start() {
    let tmp = tempfile::tempdir().unwrap();
    let game = write_fake_installation(tmp.path());
    let config = tmp.path().join("config");
    let legacy = config.join(sm2_core::LEGACY_APP_SLUG);
    write_settings_in(&legacy, &game, "de");

    // German output can only come from the settings file that was written
    // under the old name — a fresh installation speaks English.
    let output = run_modloader(tmp.path(), &["paths"]);
    assert!(output.contains("Spiel:"), "the stored language must have survived the rename:\n{output}");

    let current = config.join(sm2_core::APP_SLUG);
    assert!(current.join("settings.toml").is_file(), "the settings must live under the new name now");
    assert!(!legacy.exists(), "the old directory must not be left behind as a second copy");
}

/// The counterpart: once the program has written under the new name, an
/// old directory that is still lying around is a leftover and must not
/// overwrite it. Getting this wrong would silently reset the settings on
/// every start for anyone who kept a copy of the old directory.
#[test]
fn a_leftover_directory_of_the_previous_name_does_not_overwrite_the_current_one() {
    let tmp = tempfile::tempdir().unwrap();
    let game = write_fake_installation(tmp.path());
    let config = tmp.path().join("config");
    write_settings_in(&config.join(sm2_core::LEGACY_APP_SLUG), &game, "de");
    write_settings_in(&config.join(sm2_core::APP_SLUG), &game, "en");

    let output = run_modloader(tmp.path(), &["paths"]);

    assert!(output.contains("Game:"), "the current settings must win:\n{output}");
    assert!(config.join(sm2_core::LEGACY_APP_SLUG).exists(), "the leftover stays for the user to look at");
}

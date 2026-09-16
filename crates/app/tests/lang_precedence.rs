//! Regression test for the language ordering in `cli::run`: `--lang`
//! has to win over the language stored in `settings.toml` twice over —
//! once for the command tree built before parsing (so `--help` answers
//! in the right language) and again for the command's own output,
//! printed after `AppState::open()` re-applies the stored setting from
//! disk. Without that second `i18n::set_language` call in `run()`,
//! `--lang en` would be silently undone by `AppState::open()` for
//! everything printed after it.
//!
//! This runs the real binary as a subprocess (steered through the XDG
//! environment variables `directories::ProjectDirs` reads), not
//! `cli::run()` in-process: that function reads `std::env::args()`
//! directly and calls `std::process::exit` on error, neither of which a
//! single test binary can do more than once.

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
/// (`$XDG_CONFIG_HOME/sm2-modloader/settings.toml`), with a stored
/// language distinct from the `--lang` this test's assertions pass.
fn write_settings(xdg_config_home: &Path, game_dir: &Path, language: &str) {
    let dir = xdg_config_home.join("sm2-modloader");
    std::fs::create_dir_all(&dir).unwrap();
    let settings = format!("game_dir = {:?}\nlanguage = \"{language}\"\n", game_dir.display());
    std::fs::write(dir.join("settings.toml"), settings).unwrap();
}

/// Runs the built `sm2-modloader` binary with its XDG directories
/// pointed at `tmp`, and returns its stdout.
fn run_modloader(tmp: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_sm2-modloader"))
        .args(args)
        .env("XDG_CONFIG_HOME", tmp.join("config"))
        .env("XDG_DATA_HOME", tmp.join("data"))
        .env("XDG_STATE_HOME", tmp.join("state"))
        .output()
        .expect("failed to run the sm2-modloader binary");
    String::from_utf8(output.stdout).expect("stdout must be valid UTF-8")
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
    assert!(help.contains("Mod loader for Space Marine 2"), "{help}");
}

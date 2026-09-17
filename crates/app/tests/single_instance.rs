//! The single-instance lock as the user meets it: two processes, not two
//! `InstanceLock` values in one. `sm2-core` unit-tests the lock itself;
//! only the real binary can show that `cli::run` takes it early enough,
//! that it lets the reading commands through, and that it is gone again
//! once the process is.
//!
//! Unix only, and not because the behaviour is: the harness steers
//! `directories::ProjectDirs` through the XDG environment variables, which
//! Windows ignores in favour of `%APPDATA%` — the tests would then lock
//! and write inside the real user's configuration directory instead of a
//! sandbox. Same reason `lang_precedence.rs` carries the same gate.
//!
//! The expected sentences come from `lookup_in`, which asks a named
//! language rather than the active one. Nothing here touches the
//! process-wide language, so nothing here needs the language lock either.

#![cfg(unix)]

use sm2_core::i18n::{lookup_in, Language};
use sm2_core::instance::InstanceLock;
use sm2_core::paths::AppDirs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A minimal, valid Space Marine 2 directory — the shape
/// `GamePaths::from_game_dir` checks for. Nothing below it has to exist;
/// the `paths` command reports the missing Proton prefix as a line of its
/// own instead of failing.
fn write_fake_installation(root: &Path) -> PathBuf {
    let game = root.join("game");
    std::fs::create_dir_all(game.join("client_pc/root/mods")).unwrap();
    game
}

/// Writes the `settings.toml` that `load_dirs_and_settings` reads, so the
/// binary finds the fake installation instead of searching for Steam.
fn write_settings(tmp: &Path, game_dir: &Path) {
    let dir = tmp.join("config").join(sm2_core::APP_SLUG);
    std::fs::create_dir_all(&dir).unwrap();
    let settings = format!("game_dir = {:?}\nlanguage = \"en\"\n", game_dir.display());
    std::fs::write(dir.join("settings.toml"), settings).unwrap();
}

/// The base directories the binary below will compute for itself from the
/// same environment variables — so the lock this test holds is the very
/// file the subprocess runs into.
fn dirs_of(tmp: &Path) -> AppDirs {
    AppDirs {
        config: tmp.join("config").join(sm2_core::APP_SLUG),
        data: tmp.join("data").join(sm2_core::APP_SLUG),
        state: tmp.join("state").join(sm2_core::APP_SLUG),
    }
}

fn run_modloader(tmp: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_lina-sm2"))
        .args(args)
        .env("XDG_CONFIG_HOME", tmp.join("config"))
        .env("XDG_DATA_HOME", tmp.join("data"))
        .env("XDG_STATE_HOME", tmp.join("state"))
        .output()
        .expect("failed to run the lina-sm2 binary")
}

fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr must be valid UTF-8")
}

/// The sentence a turned-away instance prints, in English — the language
/// every subprocess here is pinned to with `--lang en`.
fn refusal() -> String {
    lookup_in(Language::English, "error.already_running").replace("{name}", sm2_core::APP_NAME)
}

#[test]
fn a_command_that_writes_is_turned_away_while_another_instance_holds_the_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let game = write_fake_installation(tmp.path());
    write_settings(tmp.path(), &game);
    let _held = InstanceLock::acquire(&dirs_of(tmp.path())).unwrap().expect("lock must be free");

    let output = run_modloader(tmp.path(), &["--lang", "en", "lang", "de"]);

    assert!(!output.status.success(), "a second instance must not be allowed to write");
    assert!(
        stderr_of(&output).contains(&refusal()),
        "the refusal has to say what is wrong:\n{}",
        stderr_of(&output)
    );
}

#[test]
fn a_command_that_only_reads_runs_while_another_instance_holds_the_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let game = write_fake_installation(tmp.path());
    write_settings(tmp.path(), &game);
    let _held = InstanceLock::acquire(&dirs_of(tmp.path())).unwrap().expect("lock must be free");

    let output = run_modloader(tmp.path(), &["--lang", "en", "paths"]);

    assert!(
        output.status.success(),
        "looking things up has to stay possible while the interface is open:\n{}",
        stderr_of(&output)
    );
}

/// The lock file is deliberately never deleted. This is the test that says
/// the leftover file is harmless: if the lock were mistaken for the file's
/// existence, every start after the first would fail.
#[test]
fn the_next_start_is_free_again_once_the_first_process_has_ended() {
    let tmp = tempfile::tempdir().unwrap();
    let game = write_fake_installation(tmp.path());
    write_settings(tmp.path(), &game);

    let first = run_modloader(tmp.path(), &["--lang", "en", "lang", "en"]);
    assert!(first.status.success(), "{}", stderr_of(&first));

    let second = run_modloader(tmp.path(), &["--lang", "en", "lang", "en"]);

    assert!(
        second.status.success(),
        "the lock outlived the process that held it:\n{}",
        stderr_of(&second)
    );
    assert!(
        tmp.path().join("state").join(sm2_core::APP_SLUG).join("instance.lock").exists(),
        "the lock file is expected to stay behind — only the lock on it is released"
    );
}

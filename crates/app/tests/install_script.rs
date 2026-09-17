//! Runs the installer's own test suite as part of `cargo test`.
//!
//! `packaging/test-install.sh` publishes a release into a temporary
//! directory and drives the real `install.sh` against it over `file://`
//! URLs — checksum check, atomic replace, version comparison and uninstall
//! included. It is hooked in here so that a change to the installer cannot
//! quietly break it: this repository has exactly one command that says
//! whether it is sound, and that command is `cargo test`.
//!
//! Nothing outside a temporary directory is touched: the harness points
//! `HOME`, the XDG variables and the installer's two endpoint variables at
//! a sandbox of its own.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repository_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is `crates/app`.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repository root")
}

#[test]
fn the_installer_passes_its_own_test_suite() {
    let root = repository_root();
    let harness = root.join("packaging/test-install.sh");
    assert!(harness.is_file(), "{} is missing", harness.display());

    let output = Command::new("sh")
        .arg(&harness)
        .current_dir(&root)
        .output()
        .expect("failed to run the installer test suite");

    if !output.status.success() {
        // Both streams: the harness names the failing case on stderr and
        // prints the run's summary on stdout, and a failure is unreadable
        // without the two together.
        panic!(
            "packaging/test-install.sh failed\n--- stdout ---\n{}\n--- stderr ---\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

/// The installer derives the asset name from the release tag and the
/// launcher's own name. Both sides of that have to keep agreeing with
/// `sm2_core::APP_SLUG`, which is where the binary's name actually comes
/// from — a rename there that stops here would produce a release whose
/// files the installer cannot find.
#[test]
fn the_installer_and_the_packaging_use_the_binary_name_from_the_branding() {
    let root = repository_root();
    let slug = sm2_core::APP_SLUG;

    let installer = std::fs::read_to_string(root.join("install.sh")).expect("install.sh");
    assert!(
        installer.contains(&format!("BIN_NAME=\"{slug}\"")),
        "install.sh must install the binary called {slug}"
    );

    let desktop =
        std::fs::read_to_string(root.join("packaging/lina-sm2.desktop")).expect("desktop entry");
    assert!(
        desktop.contains(&format!("StartupWMClass={slug}")),
        "the desktop entry's StartupWMClass must match the window's app id ({slug})"
    );
}

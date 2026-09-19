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
//!
//! Beside that one, the file holds the checks that hold the three places
//! naming a release asset to each other — `install.sh`, the packaging and
//! `.github/workflows/release.yml`. Those only read files and run
//! everywhere; it is the harness above that is Unix only, and it is
//! gated on its own.

use std::path::{Path, PathBuf};

fn repository_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is `crates/app`.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repository root")
}

// Unix only: `install.sh` is a POSIX shell script for Linux, the harness
// that drives it is another, and Windows is served by the ZIP from the
// same release instead. None of the three fixture-related reasons
// CLAUDE.md lists applies — there is genuinely nothing here to run there.
#[cfg(unix)]
#[test]
fn the_installer_passes_its_own_test_suite() {
    use std::process::Command;

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

/// `update::install` downloads the release's own `install.sh` and refuses
/// to run it unless it matches the checksum that release published for
/// it. Both halves of that live in `.github/workflows/release.yml`: the
/// assembly step puts the script into `dist/`, and the publish job's
/// `sha256sum` line names it so that `SHA256SUMS` covers it. Drop either
/// one and the update button fails with `ChecksumMismatch` — in
/// production, on a release that has already shipped, for the one
/// mechanism whose job is to deliver the fix. The three other places that
/// have to agree on an asset name are held together by tests; this is the
/// fourth, and it had none.
///
/// Matched by what the lines do rather than by their exact spelling, so
/// reformatting the YAML or renaming the step does not fail this — but
/// tightly enough that a copy to the wrong destination does.
#[test]
fn the_release_workflow_ships_the_installer_and_checksums_it() {
    let root = repository_root();
    let path = root.join(".github/workflows/release.yml");
    let workflow = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} is not readable: {e}", path.display()));

    let lines: Vec<&str> = workflow.lines().map(str::trim).collect();

    // `cp install.sh dist/` — into the directory the archives are
    // collected from, not into the unpacked archive. The destination has
    // to be `dist` itself: `dist/$name/` would put the script inside the
    // tarball, where the launcher cannot fetch it as an asset, and would
    // pass a test that only asked for "dist" somewhere on the line.
    assert!(
        lines.iter().any(|line| line.starts_with("cp ")
            && line.contains("install.sh")
            && line.trim_end_matches('/').ends_with("dist")),
        "{} must copy install.sh into dist/ itself, or update::install downloads a \
         script that release never published a checksum for",
        path.display()
    );

    assert!(
        lines.iter().any(|line| line.contains("sha256sum") && line.contains("install.sh")),
        "{}'s sha256sum line must name install.sh, or SHA256SUMS does not cover the \
         script update::install verifies against it",
        path.display()
    );
}

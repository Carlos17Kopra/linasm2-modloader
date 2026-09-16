//! Finds what slipped through: a German sentence still sitting in the
//! code instead of in a language file. Heuristic on purpose — it looks
//! for umlauts, the German quotation mark and a handful of words that do
//! not occur in English.
//!
//! Skipped are comment lines (comments are English by house rule, but a
//! quoted German example in one is legitimate) and everything from a
//! file's `#[cfg(test)]` onwards, because test data may well be German.

use std::path::{Path, PathBuf};

const MARKERS: [&str; 10] =
    ["ä", "ö", "ü", "ß", "„", " nicht ", " wird ", " kein ", " eine ", " für "];

/// Persisted identifiers that stay German on purpose (see CLAUDE.md →
/// "Language"): `vanilla::VANILLA_SNAPSHOT_PREFIX`, the "vor Modded-Start"
/// backup label next to it in `gui/commands.rs`, and the safety-backup
/// label in `saves.rs`. All three are matched by prefix and already
/// written into existing profile and backup names on users' disks, so
/// translating them would rename data that exists there today. They are
/// shown to the user even in an English interface — a known limitation,
/// not an oversight this sweep failed to catch — and whether to eventually
/// split the stored form from the displayed form is an open question left
/// for later, not decided by this test.
///
/// None of the three currently trips any `MARKERS` entry (no umlaut, no
/// listed word), so stripping them here changes nothing about which lines
/// fail today. The list exists so the decision stays visible in the one
/// place a future contributor would look — this test — instead of quietly
/// depending on the heuristic never being sharp enough to notice them.
const EXEMPT_LITERALS: [&str; 3] =
    ["vor Vanilla-Start", "vor Modded-Start", "vor Wiederherstellung"];

fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("source directory is readable") {
        let path = entry.expect("directory entry is readable").path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn no_german_text_is_left_in_the_sources() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate lives two levels below the workspace root");

    let mut files = Vec::new();
    rust_files(&workspace.join("crates/core/src"), &mut files);
    rust_files(&workspace.join("crates/app/src"), &mut files);

    let mut hits = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).expect("source file is readable");
        for (number, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("#[cfg(test)]") {
                break;
            }
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            if !code.contains('"') {
                continue;
            }
            // Exempted literals are removed before the marker check, not
            // used to skip the whole line: anything else German on the
            // same line must still be caught.
            let mut checked = code.to_string();
            for literal in EXEMPT_LITERALS {
                checked = checked.replace(literal, "");
            }
            if MARKERS.iter().any(|marker| checked.contains(marker)) {
                hits.push(format!("{}:{}: {}", file.display(), number + 1, code.trim()));
            }
        }
    }

    assert!(hits.is_empty(), "German text still in the sources:\n{}", hits.join("\n"));
}

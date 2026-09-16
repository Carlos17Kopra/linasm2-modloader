//! Finds what slipped through: a German sentence still sitting in the
//! code instead of in a language file.
//!
//! What this guarantees: every quoted string in production code (that is,
//! not inside a `#[cfg(test)]` item and not inside a `//`-comment) is
//! checked against `MARKERS` — a handful of umlauts, `ß`, the German
//! opening quote, and five whole words that do not occur in English. A
//! `#[cfg(test)]` item is skipped precisely for its own extent (tracked by
//! brace depth, so a `#[cfg(test)] fn helper() { … }` early in a file does
//! not blind the sweep to the production code that follows it, the way a
//! plain "stop at the first `#[cfg(test)]` line" would) and scanning
//! resumes right after it — test data may well be German, but the
//! production code around it must not be.
//!
//! What this cannot see: `MARKERS` is a fixed, short list, not a language
//! model — a German sentence built entirely from words outside that list
//! (no umlaut, no `ß`, none of the five listed words as whole words) slips
//! through undetected. And the comment check only recognises a line whose
//! *entire* trimmed content starts with `//`; a trailing `// Kommentar`
//! after real code on the same line is treated as part of the code, which
//! risks the opposite mistake — flagging a legitimate German example in a
//! trailing comment as a hit. That risk was accepted deliberately: telling
//! a trailing `//` comment from a `//` inside a string (a URL, for
//! instance) would need a real tokenizer, and a heuristic that occasionally
//! over-reports on a comment is a far smaller problem than one that
//! under-reports on code.

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

/// Tracks whether the current line falls inside a `#[cfg(test)]` item, so
/// the caller can skip exactly that item and nothing else.
///
/// A file-wide "stop at the first `#[cfg(test)]` line" would blind the
/// sweep to every production line after the first test-only helper — and
/// this codebase has exactly that shape more than once (a `#[cfg(test)]`
/// helper function ahead of the real, later-defined item it supports).
/// Brace depth is tracked instead: once inside a skipped item, lines are
/// skipped until the item's own braces close (or, for a brace-less item
/// such as a single `use` or `const`, until its terminating `;`), and
/// scanning resumes normally right after — including for another
/// `#[cfg(test)]` item met later in the same file.
struct TestItemSkipper {
    depth: i32,
    skip: Option<SkipState>,
}

struct SkipState {
    base_depth: i32,
    opened: bool,
}

impl TestItemSkipper {
    fn new() -> Self {
        Self { depth: 0, skip: None }
    }

    /// Feeds one line and reports whether it belongs to a skipped
    /// `#[cfg(test)]` item (and must therefore not be checked for
    /// German).
    fn is_test_only(&mut self, trimmed: &str) -> bool {
        if self.skip.is_none() && trimmed.starts_with("#[cfg(test)]") {
            self.skip = Some(SkipState { base_depth: self.depth, opened: false });
        }

        let Some(state) = &mut self.skip else {
            self.depth += brace_delta(trimmed);
            return false;
        };

        let delta = brace_delta(trimmed);
        if delta > 0 {
            state.opened = true;
        }
        self.depth += delta;

        let finished = if state.opened {
            self.depth <= state.base_depth
        } else {
            // No brace seen yet: either still reading a multi-line
            // signature ahead of the item's body, or the whole item was a
            // single statement (`use …;`, `const …;`) that never opens
            // one at all.
            trimmed.ends_with(';')
        };
        if finished {
            self.skip = None;
        }
        true
    }
}

fn brace_delta(line: &str) -> i32 {
    line.matches('{').count() as i32 - line.matches('}').count() as i32
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
        let mut skipper = TestItemSkipper::new();
        for (number, line) in text.lines().enumerate() {
            let code = line.trim_start();

            if skipper.is_test_only(code) {
                continue;
            }
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

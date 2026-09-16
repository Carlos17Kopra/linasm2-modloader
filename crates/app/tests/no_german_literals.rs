//! Finds what slipped through: a German sentence still sitting in the
//! code instead of in a language file.
//!
//! What this guarantees: every quoted string in production code (that is,
//! not inside a `#[cfg(test)]` item and not inside a `//`-comment) is
//! checked against `MARKERS` — a handful of umlauts, `ß`, the German
//! opening quote, and five whole words that do not occur in English. A
//! `#[cfg(test)]` item is skipped precisely for its own extent (see
//! `TestItemSkipper`, below) so a `#[cfg(test)] fn helper() { … }` early in
//! a file does not blind the sweep to the production code that follows
//! it, and scanning resumes right after it — test data may well be
//! German, but the production code around it must not be.
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
//!
//! Finding the extent of a skipped `#[cfg(test)]` item is its own small
//! hazard, covered separately below `TestItemSkipper`.

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

/// Finds the extent of a `#[cfg(test)]` item by indentation, not by
/// counting braces.
///
/// A brace counter was tried first and rejected: `rustfmt` never puts two
/// statements on one line, but it does not touch the *contents* of string
/// literals, and this codebase's own test style is full of strings that
/// can carry a lone unmatched `{` or `}` (a YAML/JSON snippet, a raw
/// string, a format example). One such brace inside a skipped item is
/// enough to offset a running depth counter by one forever — the item's
/// real closing brace then never brings depth back to baseline, and
/// everything from there to the end of the file (production code and the
/// real trailing `mod tests` alike) is misread as still part of the
/// skipped item. That failure is worse than the one it replaced: it is
/// unbounded (nothing after the corrupted item is ever scanned again) and
/// silent (the sweep reports green).
///
/// Indentation is used instead, leaning on the one property `rustfmt`
/// *does* guarantee: an item's own top-level lines (its attribute, its
/// signature, its closing brace) all sit at the same column, and
/// everything belonging to its body is indented strictly deeper. So:
/// record the attribute's column, then skip lines without inspecting
/// their contents at all until a later line lands back at that exact
/// column — that line is either the item's own single-line ending (a
/// signature ending in `{` opens the body; one ending in `;` or `}`
/// finishes a brace-less or single-line item on the spot), or, once the
/// body has been opened, a line whose first non-blank character is `}`.
/// Nothing inside the body is ever looked at except its indentation, so a
/// brace inside a string several lines deep — the exact case that broke
/// the brace counter — never reaches this logic at all.
///
/// The tradeoff: this is blind to hand-mangled formatting (an item whose
/// own lines are not all at one consistent column). That risk was judged
/// far smaller than reading arbitrary string content as if it were code.
struct TestItemSkipper {
    state: State,
}

enum State {
    Normal,
    /// Looking for the item's own top-level line, back at `col` (its
    /// attribute's indentation) — could be another stacked attribute, a
    /// doc comment, a continuation of a multi-line signature (all at a
    /// deeper indentation, hence skipped without a state change), or
    /// finally the line that decides how the item ends.
    AwaitingItemLine { col: usize },
    /// The item's body has been opened (its top-level line ended in
    /// `{`); waiting for a line back at `col` whose first non-blank
    /// character closes it.
    InBody { col: usize },
}

/// The number of leading space/tab characters on `line`.
fn indentation(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

impl TestItemSkipper {
    fn new() -> Self {
        Self { state: State::Normal }
    }

    /// Feeds one raw (untrimmed) line and reports whether it belongs to a
    /// skipped `#[cfg(test)]` item and must therefore not be checked for
    /// German.
    fn is_test_only(&mut self, line: &str) -> bool {
        let trimmed = line.trim();
        match &self.state {
            State::Normal => {
                if trimmed.starts_with("#[cfg(test)]") {
                    self.state = State::AwaitingItemLine { col: indentation(line) };
                    true
                } else {
                    false
                }
            }
            State::AwaitingItemLine { col } => {
                let col = *col;
                if indentation(line) != col || trimmed.is_empty() {
                    // Deeper (a multi-line signature still being read) or
                    // a blank separator line — either way, not yet the
                    // item's decisive line.
                    return true;
                }
                if trimmed.starts_with("#[") || trimmed.starts_with("//") {
                    // Another stacked attribute or a doc comment above the
                    // item itself; still waiting.
                    return true;
                }
                if trimmed.ends_with('{') {
                    self.state = State::InBody { col };
                } else if trimmed.ends_with(';') || trimmed.ends_with('}') {
                    self.state = State::Normal;
                }
                // Anything else at this column (an unusually broken
                // multi-line signature line, for instance) is treated as
                // "still not decisive" and left in `AwaitingItemLine` —
                // deliberately conservative, since ending the skip early
                // would expose body content to the marker check, and
                // staying in this state never inspects that content for
                // braces either way.
                true
            }
            State::InBody { col } => {
                if indentation(line) == *col && trimmed.starts_with('}') {
                    self.state = State::Normal;
                }
                true
            }
        }
    }
}

/// Scans one file's already-read `source` for German left in production
/// code. Returns one `(1-based line number, trimmed code)` pair per hit.
///
/// Extracted from the repository-walking test so the exact same logic can
/// run over a synthetic string in a unit test — the only way to exercise
/// inputs (an unmatched brace inside a string, for instance) that the real
/// tree does not happen to contain today.
fn scan_for_german(source: &str) -> Vec<(usize, String)> {
    let mut skipper = TestItemSkipper::new();
    let mut hits = Vec::new();
    for (number, line) in source.lines().enumerate() {
        if skipper.is_test_only(line) {
            continue;
        }
        let code = line.trim_start();
        if code.starts_with("//") {
            continue;
        }
        if !code.contains('"') {
            continue;
        }
        // Exempted literals are removed before the marker check, not used
        // to skip the whole line: anything else German on the same line
        // must still be caught.
        let mut checked = code.to_string();
        for literal in EXEMPT_LITERALS {
            checked = checked.replace(literal, "");
        }
        if MARKERS.iter().any(|marker| checked.contains(marker)) {
            hits.push((number + 1, code.trim().to_string()));
        }
    }
    hits
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
        for (number, code) in scan_for_german(&text) {
            hits.push(format!("{}:{number}: {code}", file.display()));
        }
    }

    assert!(hits.is_empty(), "German text still in the sources:\n{}", hits.join("\n"));
}

/// Unit tests for `scan_for_german` and `TestItemSkipper` over synthetic
/// sources, targeting exactly the inputs the real tree does not (and, for
/// the adversarial cases, must never) contain — see `TestItemSkipper`'s
/// own doc for the failure mode these guard against.
#[cfg(test)]
mod skipper_tests {
    use super::scan_for_german;

    /// A canary sentence built to trip several `MARKERS` at once (an
    /// umlaut, `ß`, and more than one of the five listed words), so a test
    /// asserting it was *not* found cannot pass by the sentence being too
    /// weak to trip anything in the first place.
    const CANARY: &str =
        "Diese Zeichenkette darf nicht für später stehen, weil sie öde wäre und über bleibt";

    #[test]
    fn german_in_a_normal_function_is_found() {
        let source = format!("fn production() {{\n    let s = \"{CANARY}\";\n}}\n");
        let hits = scan_for_german(&source);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].0, 2);
    }

    /// The bug from review round 1: stopping at the very first
    /// `#[cfg(test)]` line instead of only for that item's extent.
    #[test]
    fn production_code_after_an_early_cfg_test_item_is_still_scanned() {
        let source = format!(
            "#[cfg(test)]\nfn helper() {{\n    let ok = 1;\n}}\n\nfn production() -> &'static str {{\n    \"{CANARY}\"\n}}\n"
        );
        let hits = scan_for_german(&source);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].0, 7);
    }

    /// The bug from review round 2: an unmatched `{` inside a string
    /// literal, deep in a skipped item's body, corrupting a brace
    /// counter so that everything after the item — including real
    /// production code — is (silently, and forever) treated as still
    /// skipped.
    #[test]
    fn an_unmatched_open_brace_in_a_skipped_item_does_not_swallow_the_rest_of_the_file() {
        let source = format!(
            "#[cfg(test)]\nfn helper() {{\n    let weird = \"unbalanced {{ brace\";\n}}\n\nfn production() -> &'static str {{\n    \"{CANARY}\"\n}}\n"
        );
        let hits = scan_for_german(&source);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].0, 7);
    }

    /// Same failure mode, mirrored: an unmatched `}` inside a string.
    #[test]
    fn an_unmatched_close_brace_in_a_skipped_item_does_not_swallow_the_rest_of_the_file() {
        let source = format!(
            "#[cfg(test)]\nfn helper() {{\n    let weird = \"unbalanced }} brace\";\n}}\n\nfn production() -> &'static str {{\n    \"{CANARY}\"\n}}\n"
        );
        let hits = scan_for_german(&source);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].0, 7);
    }

    /// A raw string is the natural place for both braces to show up
    /// unmatched at once (a JSON or YAML snippet, for instance).
    #[test]
    fn a_raw_string_with_both_braces_in_a_skipped_item_does_not_swallow_the_rest_of_the_file() {
        let source = format!(
            "#[cfg(test)]\nfn helper() {{\n    let weird = r#\"{{ \"weird\": \"}}\" }}\"#;\n}}\n\nfn production() -> &'static str {{\n    \"{CANARY}\"\n}}\n"
        );
        let hits = scan_for_german(&source);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].0, 7);
    }

    /// A brace-shaped comment inside a skipped item must not be mistaken
    /// for the item's closing brace, or for its production-code
    /// counterpart escaping the skip early. Comments always start with
    /// `//`, so a line's first non-blank character being `}` can only
    /// ever be real code — but this is worth pinning down explicitly.
    #[test]
    fn a_brace_in_a_comment_inside_a_skipped_item_is_not_mistaken_for_its_close() {
        let source = format!(
            "#[cfg(test)]\nfn helper() {{\n    // looks like a close: }}\n    let ok = 1;\n}}\n\nfn production() -> &'static str {{\n    \"{CANARY}\"\n}}\n"
        );
        let hits = scan_for_german(&source);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].0, 8);
    }

    /// A `#[cfg(test)]` item nested inside a module: the skip must end at
    /// the *item's* closing brace (indented one level in), not at the
    /// enclosing module's, which would either end too early (exposing the
    /// rest of the module's test-only content) or — if the module's own
    /// brace were mistaken for an earlier, unrelated closing brace — too
    /// late.
    #[test]
    fn a_cfg_test_item_nested_inside_a_module_is_skipped_for_only_its_own_extent() {
        let source = format!(
            "mod outer {{\n    #[cfg(test)]\n    fn helper() {{\n        let s = \"{CANARY}\";\n    }}\n\n    pub fn production() -> &'static str {{\n        \"{CANARY}\"\n    }}\n}}\n"
        );
        let hits = scan_for_german(&source);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].0, 8);
    }

    /// The common case: a normal trailing `mod tests` full of legitimate
    /// German test data must still be skipped in its entirety, including
    /// after this fix's rework of how the extent is found.
    #[test]
    fn a_trailing_mod_tests_with_german_test_data_is_fully_skipped() {
        let source = format!(
            "fn production() -> &'static str {{\n    \"english only\"\n}}\n\n#[cfg(test)]\nmod tests {{\n    #[test]\n    fn uses_german_fixture_data() {{\n        let input = \"{CANARY}\";\n        assert!(!input.is_empty());\n    }}\n}}\n"
        );
        let hits = scan_for_german(&source);
        assert!(hits.is_empty(), "{hits:?}");
    }

    /// A brace-less item (`use …;`) directly under `#[cfg(test)]` must end
    /// its skip on that same line, not swallow whatever follows.
    #[test]
    fn a_brace_less_cfg_test_item_ends_its_skip_on_its_own_line() {
        let source = format!(
            "#[cfg(test)]\nuse std::collections::HashMap;\n\nfn production() -> &'static str {{\n    \"{CANARY}\"\n}}\n"
        );
        let hits = scan_for_german(&source);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].0, 5);
    }
}

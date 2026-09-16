//! Finds what slipped through: a German sentence still sitting in the
//! code instead of in a language file.
//!
//! This is a lint, not a proof. It guarantees: every quoted string in
//! production code — everything in a file up to (not including) its
//! trailing `#[cfg(test)] mod tests { … }`, and outside a `//`-comment —
//! is checked against `MARKERS`, a fixed list of umlauts, `ß`, the German
//! opening quote, and five whole words that do not occur in English.
//! Everything from the trailing test module onward is not looked at:
//! test data may well be German. Anything exempted from the check despite
//! matching a marker is listed explicitly, with a reason, in
//! `EXEMPT_LITERALS` — never hidden behind a mechanism.
//!
//! It does *not* guarantee: `MARKERS` is a short, fixed list, not a
//! language model — a German sentence built entirely from words outside
//! it (no umlaut, no `ß`, none of the five listed words as whole words)
//! is invisible to this sweep. A `#[cfg(test)]` item that is *not* the
//! trailing module (a test-only helper function defined ahead of the
//! production code it supports, for instance) is scanned like any other
//! code — deliberately: three earlier attempts at structurally
//! recognising "this is test-only" each introduced their own hole (see
//! `find_trailing_test_module`'s doc comment for the history), so this
//! version tracks nothing and skips nothing except the one trailing block
//! that is common practice throughout this codebase. If a legitimate
//! test-only helper ever needs German ahead of the trailing module, the
//! fix is to add it to `EXEMPT_LITERALS`, not to make the skip logic
//! smarter again. And the comment check only recognises a line whose
//! *entire* trimmed content starts with `//`; a trailing `// Kommentar`
//! after real code on the same line is read as code, which risks flagging
//! a legitimate German example in a trailing comment — accepted
//! deliberately, since telling that comment from a `//` inside a string
//! (a URL, for instance) needs a real tokenizer, and over-reporting on a
//! comment is a far smaller problem than under-reporting on code.

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

/// The 0-based index of the `#[cfg(test)]` line that opens a file's
/// trailing test module, or `None` if the file has none in that exact
/// shape.
///
/// History: this used to be a state machine that tried to recognise the
/// extent of *every* `#[cfg(test)]` item, first by counting braces, then
/// (after a brace inside a string literal corrupted the count and
/// silently swallowed the rest of the file) by comparing indentation
/// columns instead. That second version was undone by the same class of
/// input from the other direction: a multi-line string literal whose
/// *continuation* line is unindented and happens to start with `}` —
/// legitimate Rust, and this codebase's own tests already write strings
/// like that — which the column check misread as the item's closing
/// brace, ending the skip too early. Three implementations, three holes,
/// each found only once the previous one was in place.
///
/// So this version tracks nothing structural at all. It looks for one
/// specific, literal shape: a line that is exactly `#[cfg(test)]` at
/// column zero, immediately (modulo blank lines) followed by a line that
/// is exactly `mod tests {` at column zero — the shape every trailing
/// test module in this codebase already has (verified by hand across the
/// whole tree when this was written) — and takes the *last* such pair in
/// the file. Nothing about a line's content decides anything here except
/// two exact string comparisons; a string literal could only mislead this
/// by containing a line that reads *exactly* `mod tests {` with *exactly*
/// `#[cfg(test)]` above it, both flush against column zero, which is
/// absurd on its face, and even then would only cause this file's own
/// tail to be over-skipped — not swallow anything forward the way the
/// previous two versions could.
///
/// A `#[cfg(test)]` item that is not shaped like this trailing module
/// (an early test-only helper function, for instance) is not recognised
/// and is not skipped — see the module doc's "does not guarantee"
/// paragraph for why that is the deliberate point, not a gap to close.
fn find_trailing_test_module(lines: &[&str]) -> Option<usize> {
    for i in (0..lines.len()).rev() {
        if lines[i] != "mod tests {" {
            continue;
        }
        if let Some(attribute) = previous_non_blank(lines, i) {
            if lines[attribute] == "#[cfg(test)]" {
                return Some(attribute);
            }
        }
    }
    None
}

/// The index of the nearest non-blank line strictly before `index`, or
/// `None` if there is none.
fn previous_non_blank(lines: &[&str], index: usize) -> Option<usize> {
    (0..index).rev().find(|&i| !lines[i].trim().is_empty())
}

/// Scans one file's already-read `source` for German left in production
/// code. Returns one `(1-based line number, trimmed code)` pair per hit.
///
/// Extracted from the repository-walking test so the exact same logic can
/// run over a synthetic string in a unit test — the only way to exercise
/// the inputs that broke the two earlier versions of this sweep, none of
/// which the real tree happens to contain today.
fn scan_for_german(source: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = source.lines().collect();
    let skip_from = find_trailing_test_module(&lines).unwrap_or(lines.len());

    let mut hits = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if index >= skip_from {
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
            hits.push((index + 1, code.trim().to_string()));
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

/// Unit tests for `scan_for_german` and `find_trailing_test_module` over
/// synthetic sources — the only way to exercise inputs the repository
/// does not happen to contain, including the ones that broke the two
/// earlier versions of this sweep.
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

    /// An early, non-trailing `#[cfg(test)]` item (a test-only helper
    /// function ahead of the code it supports — this codebase has more
    /// than one) is not structurally recognised, by design: it is scanned
    /// like any other code, exactly as this test names it.
    #[test]
    fn an_early_non_trailing_cfg_test_item_is_scanned_like_any_other_code() {
        let source = format!(
            "#[cfg(test)]\nfn helper() -> &'static str {{\n    \"{CANARY}\"\n}}\n\nfn production() {{}}\n"
        );
        let hits = scan_for_german(&source);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].0, 3);
    }

    /// The failure mode of the very first version (brace counting) and,
    /// mirrored, of the version before this one (indentation columns):
    /// production code that comes *after* something that looks test-only
    /// must still be scanned. A trailing test module is the one shape
    /// that is genuinely skipped, and only when it is genuinely trailing.
    #[test]
    fn production_code_after_an_early_cfg_test_item_is_still_scanned() {
        let source = format!(
            "#[cfg(test)]\nfn helper() {{\n    let ok = 1;\n}}\n\nfn production() -> &'static str {{\n    \"{CANARY}\"\n}}\n"
        );
        let hits = scan_for_german(&source);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].0, 7);
    }

    /// The failure mode that defeated the indentation-column version: a
    /// multi-line string whose continuation line is unindented and starts
    /// with `}` — legitimate Rust — sitting *outside* the trailing test
    /// module, in production code. A column check misreads that line as
    /// some enclosing item's close; this version does not look at body
    /// content or indentation at all, only for the one literal
    /// `#[cfg(test)]` / `mod tests {` pair, so it cannot be confused by it.
    #[test]
    fn an_unindented_continuation_line_starting_with_brace_outside_the_test_module_is_not_confused() {
        let source = format!(
            "fn production() -> &'static str {{\n    let s = \"line one\n}} not actually a brace, still inside the string\";\n    \"{CANARY}\"\n}}\n"
        );
        let hits = scan_for_german(&source);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].0, 4);
    }

    /// The same shape, this time genuinely inside the trailing test
    /// module: it must not falsely end the skip early and expose the
    /// German test data that follows it as if it were production code.
    #[test]
    fn an_unindented_continuation_line_starting_with_brace_inside_the_test_module_does_not_end_the_skip_early(
    ) {
        let source = format!(
            "fn production() -> &'static str {{\n    \"english only\"\n}}\n\n#[cfg(test)]\nmod tests {{\n    #[test]\n    fn uses_a_multiline_fixture() {{\n        let s = \"line one\n}} not actually a brace, still inside the string\";\n        let input = \"{CANARY}\";\n        assert!(!input.is_empty() && !s.is_empty());\n    }}\n}}\n"
        );
        let hits = scan_for_german(&source);
        assert!(hits.is_empty(), "{hits:?}");
    }

    /// The ordinary case, and the one a sweep must never get wrong:
    /// legitimate German test data inside a normal trailing `mod tests`
    /// is not flagged. A sweep that cries wolf on ordinary test fixtures
    /// gets disabled by the first contributor it annoys.
    #[test]
    fn legitimate_german_test_data_in_a_trailing_mod_tests_is_not_flagged() {
        let source = format!(
            "fn production() -> &'static str {{\n    \"english only\"\n}}\n\n#[cfg(test)]\nmod tests {{\n    #[test]\n    fn uses_german_fixture_data() {{\n        let input = \"{CANARY}\";\n        assert!(!input.is_empty());\n    }}\n}}\n"
        );
        let hits = scan_for_german(&source);
        assert!(hits.is_empty(), "{hits:?}");
    }

    /// A file with no trailing test module at all (or none in the exact
    /// recognised shape) has nothing skipped — the whole file is scanned,
    /// which is the safe default: under-skipping only risks a false
    /// positive on test data, never a missed German sentence in
    /// production code.
    #[test]
    fn a_file_without_a_trailing_test_module_is_scanned_in_full() {
        let source = format!("fn production() -> &'static str {{\n    \"{CANARY}\"\n}}\n");
        let hits = scan_for_german(&source);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].0, 2);
    }

    /// Only the *last* `#[cfg(test)]` / `mod tests {` pair counts as the
    /// trailing module. An earlier occurrence of that same shape (however
    /// unlikely) must not cut the file short before its real end.
    #[test]
    fn only_the_last_cfg_test_mod_tests_pair_is_treated_as_the_trailing_module() {
        let source = format!(
            "#[cfg(test)]\nmod tests {{\n    // not the real one\n}}\n\nfn production() -> &'static str {{\n    \"{CANARY}\"\n}}\n\n#[cfg(test)]\nmod tests {{\n    let fixture = \"{CANARY}\";\n}}\n"
        );
        let hits = scan_for_german(&source);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].0, 7);
    }
}

//! Guards the catalogue against the one mistake the unit tests cannot
//! see: a key used in the sources that nobody ever put into `en.toml`.
//! It would only show up as its own name in the running program.

use std::path::{Path, PathBuf};

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

/// Every `t!("…")` in a source text, ignoring the macro definition itself.
///
/// Three things a plain substring search for `t!("` would misread as a
/// catalogue key: it also lights up inside `format!("`, `print!("` and
/// `write!("`, all common in this codebase, so the character before the
/// match must not be part of an identifier; a `t!("…")` written as a
/// doc-comment example (see the macro's own docs) is documentation, not an
/// invocation, so whole-line comments are stripped before the scan; and
/// `rustfmt` wraps a long `t!(` call onto several lines once its argument
/// list no longer fits one, which puts the key on its own line — so
/// whitespace, including newlines, between `t!(` and the opening quote has
/// to be allowed, not just the single space rustfmt happens to use today.
fn keys_in(text: &str) -> Vec<String> {
    let code: String = text
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut keys = Vec::new();
    let mut offset = 0;
    while let Some(found) = code[offset..].find("t!(") {
        let position = offset + found;
        let starts_word = code[..position]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_');
        let after_paren = &code[position + 3..];
        let quote_start = after_paren.find(|c: char| !c.is_whitespace());
        let Some(quote_start) = quote_start else { break };
        if !after_paren[quote_start..].starts_with('"') {
            // Not a `t!("…")` call at all (e.g. the macro's own
            // `macro_rules!` arms) — move past `t!(` and keep scanning.
            offset = position + 3;
            continue;
        }
        let after_quote = &after_paren[quote_start + 1..];
        match after_quote.find('"') {
            Some(end) => {
                if starts_word {
                    keys.push(after_quote[..end].to_string());
                }
                offset = position + 3 + quote_start + 1 + end + 1;
            }
            None => break,
        }
    }
    keys
}

#[test]
fn every_key_used_in_the_sources_exists_in_the_english_catalogue() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate lives two levels below the workspace root");

    let mut files = Vec::new();
    rust_files(&workspace.join("crates/core/src"), &mut files);
    rust_files(&workspace.join("crates/app/src"), &mut files);

    let mut missing = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).expect("source file is readable");
        for key in keys_in(&text) {
            if !sm2_core::i18n::has_key(&key) {
                missing.push(format!("{}: {key}", file.display()));
            }
        }
    }

    assert!(missing.is_empty(), "keys used in code but missing from en.toml:\n{}", missing.join("\n"));
}

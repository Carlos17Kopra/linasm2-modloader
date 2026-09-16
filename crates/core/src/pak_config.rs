use crate::atomic::write_atomic;
use crate::error::{Error, PakConfigDefect, Result, YamlScalarShape};
use std::collections::HashMap;
use std::path::Path;
use yaml_rust2::{Yaml, YamlLoader};

/// An entry in pak_config.yaml.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PakEntry {
    pub pak: String,
    pub disabled: bool,
}

/// The contents of pak_config.yaml. The order of the entries is the engine's
/// load order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PakConfig {
    pub entries: Vec<PakEntry>,
}

impl PakConfig {
    pub fn parse(text: &str) -> Result<Self> {
        // Windows editors (Notepad, for example) like to write a UTF-8 BOM in
        // front. Without stripping it, yaml-rust2 reads the document as a
        // hash instead of an array, and the root check below fails with a
        // misleading message.
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);

        if text.trim().is_empty() {
            return Ok(Self::default());
        }

        let docs = YamlLoader::load_from_str(text).map_err(|e| {
            let marker = e.marker();
            Error::PakConfig(PakConfigDefect::InvalidYaml {
                line: marker.line(),
                column: marker.col() + 1,
            })
        })?;

        let Some(doc) = docs.first() else {
            return Ok(Self::default());
        };

        let items = match doc {
            Yaml::Array(items) => items,
            Yaml::Null => return Ok(Self::default()),
            _ => return Err(Error::PakConfig(PakConfigDefect::RootNotAList)),
        };

        let mut entries = Vec::with_capacity(items.len());
        for (i, item) in items.iter().enumerate() {
            let Yaml::Hash(map) = item else {
                return Err(Error::PakConfig(PakConfigDefect::EntryNotAnObject { index: i + 1 }));
            };

            let pak = map
                .get(&Yaml::String("pak".into()))
                .and_then(Yaml::as_str)
                .ok_or(Error::PakConfig(PakConfigDefect::EntryMissingPakKey { index: i + 1 }))?;

            let disabled = match map.get(&Yaml::String("disabled".into())) {
                None => false,
                Some(value) => value.as_bool().ok_or_else(|| {
                    Error::PakConfig(PakConfigDefect::EntryInvalidDisabledValue {
                        index: i + 1,
                        found: describe_yaml_scalar(value),
                    })
                })?,
            };

            entries.push(PakEntry { pak: pak.to_string(), disabled });
        }

        Ok(Self { entries })
    }

    /// The active entries in load order.
    pub fn enabled(&self) -> impl Iterator<Item = &PakEntry> {
        self.entries.iter().filter(|e| !e.disabled)
    }

    /// Produces the file contents for the engine.
    ///
    /// Written by hand instead of serialized: the format has two fields, and
    /// byte-exact control matters more here than convenience.
    pub fn to_yaml(&self) -> String {
        if self.entries.is_empty() {
            // An empty file parses as YAML null, not as an empty list.
            return "[]\n".to_string();
        }

        let mut out = String::new();
        for entry in &self.entries {
            out.push_str("- pak: ");
            out.push_str(&quote_if_needed(&entry.pak));
            out.push('\n');
            if entry.disabled {
                out.push_str("  disabled: true\n");
            }
        }
        out
    }

    /// Loads the configuration from `path`. A missing file yields an empty
    /// configuration (a fresh installation without mods).
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    /// Writes the configuration atomically to `path`.
    pub fn save(&self, path: &Path) -> Result<()> {
        write_atomic(path, &self.to_yaml())
    }

    /// Brings the configuration in line with what the directory actually
    /// contains. Order and activation state of existing entries stay
    /// untouched.
    ///
    /// For paks that are currently not in the configuration but once had a
    /// known state (see `KnownState`), `last_known` supplies activation and
    /// position from the last time they were part of the configuration. That
    /// is the only source from which a pak that has reappeared (after a Steam
    /// update, for example, see spec §9 R3) gets its previous place and
    /// activation state back, instead of being appended enabled at the end
    /// like a pak never seen before. `pak_config.rs` deliberately does not
    /// know the library itself — that would invert the module layering, see
    /// `library.rs`s dependency in the other direction; the caller (for
    /// example `AppState::open`) builds this map from `Library` and passes it
    /// in explicitly.
    pub fn reconcile(
        &mut self,
        present: &[String],
        last_known: &HashMap<String, KnownState>,
    ) -> Reconciliation {
        let present_set: std::collections::HashSet<&str> =
            present.iter().map(String::as_str).collect();

        let mut removed = Vec::new();
        self.entries.retain(|e| {
            if present_set.contains(e.pak.as_str()) {
                true
            } else {
                removed.push(e.pak.clone());
                false
            }
        });

        // `present` is a `&[String]`, not a set: a directory cannot contain
        // the same name twice, but a caller could report it twice. `known` is
        // therefore extended as `added`/`restored` are built up (rather than
        // computed once up front), so that a repeated name is taken in only
        // once.
        let mut known: std::collections::HashSet<&str> =
            self.entries.iter().map(|e| e.pak.as_str()).collect();

        let mut added: Vec<String> = Vec::new();
        let mut restored: Vec<String> = Vec::new();
        let mut fresh: Vec<String> = Vec::new();
        let mut with_history: Vec<(String, &KnownState)> = Vec::new();

        for pak in present {
            if known.insert(pak.as_str()) {
                added.push(pak.clone());
                match last_known.get(pak.as_str()) {
                    Some(state) => with_history.push((pak.clone(), state)),
                    None => fresh.push(pak.clone()),
                }
            }
        }
        added.sort();

        // Known paks first, at their old position (sorted ascending so that
        // several paks reappearing at once keep their relative order to one
        // another). Only after that are never-before-seen paks appended
        // alphabetically at the end — their position was written down
        // nowhere, so "at the end" is the only sensible choice.
        with_history.sort_by_key(|(_, state)| state.position);
        for (pak, state) in with_history {
            let index = state.position.min(self.entries.len());
            self.entries.insert(index, PakEntry { pak: pak.clone(), disabled: state.disabled });
            restored.push(pak);
        }
        restored.sort();

        fresh.sort();
        for pak in &fresh {
            self.entries.push(PakEntry { pak: pak.clone(), disabled: false });
        }

        Reconciliation { added, restored, removed }
    }
}

/// Last known state of a pak that is currently no longer in the configuration
/// — activation and position as they were written on the last `persist()`.
/// Lets `reconcile` put a pak that has reappeared back in its old place,
/// instead of appending it enabled at the end like an unknown pak (see spec
/// §9 R3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownState {
    pub disabled: bool,
    pub position: usize,
}

/// What a reconciliation between directory and configuration changed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Reconciliation {
    /// All paks that were in the directory but not in the configuration —
    /// the union of `restored` (the known ones) and the newly arrived paks
    /// that had never been seen before.
    pub added: Vec<String>,
    /// Subset of `added`: paks with a known previous state that were put back
    /// at their old position with their old activation.
    pub restored: Vec<String>,
    /// Entries whose file is missing.
    pub removed: Vec<String>,
}

impl Reconciliation {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
}

/// Puts a file name in quotes when it would otherwise be read as a different
/// YAML construct.
///
/// The obvious special characters (colon, hash, quotes, leading structure
/// characters, surrounding whitespace) are caught by a character check. That
/// is not enough, though: a name like "true", "null" or "123" contains none
/// of those characters, yet unquoted it would be read as a bool, null or
/// number instead of a string — `parse` would then no longer get a valid
/// `pak` key and would fail. So the real YAML parser is used as well, to
/// check whether the unquoted name, taken as a scalar, would resolve to
/// anything other than exactly itself as a string (this also catches embedded
/// line breaks, which as a standalone plain scalar would be folded into a
/// single space).
fn quote_if_needed(name: &str) -> String {
    let needs_quotes = name.is_empty()
        || name.contains(':')
        || name.contains('#')
        || name.contains('"')
        || name.contains('\'')
        || name.starts_with(['-', '?', '&', '*', '!', '|', '>', '%', '@', '`', '[', '{'])
        || name.trim() != name
        || resolves_to_non_string_scalar(name);

    if needs_quotes {
        format!(
            "\"{}\"",
            name.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\r', "\\r")
                .replace('\n', "\\n")
        )
    } else {
        name.to_string()
    }
}

/// Checks whether `name`, as a standalone unquoted YAML plain scalar, would
/// resolve to something other than the string `name` itself (to a bool, a
/// number, `null` or a folded line, for example). Uses the same parser as
/// `parse`, so the decision is guaranteed to match the actual read behavior.
fn resolves_to_non_string_scalar(name: &str) -> bool {
    match YamlLoader::load_from_str(name) {
        Ok(docs) => !matches!(docs.first(), Some(Yaml::String(s)) if s == name),
        Err(_) => true,
    }
}

/// Classifies a YAML scalar for an error message. `yaml-rust2` has no
/// `Display` for `Yaml`, hence this small, user-readable rendering of the
/// common cases with a `Debug` fallback for the rest.
///
/// Returns a `YamlScalarShape` rather than an already-rendered `String`:
/// `List`/`Object` carry no wording of their own, only a shape, so that
/// `PakConfigDefect::text` can pick the word in whichever language is
/// active when the error is actually displayed — not the one active here,
/// while `parse` runs. The other cases (`Literal`) are data (a quoted
/// value, a number, a boolean, `null`), not text, so there is nothing to
/// translate and they carry their rendered form directly.
fn describe_yaml_scalar(value: &Yaml) -> YamlScalarShape {
    match value {
        Yaml::String(s) => YamlScalarShape::Literal(format!("\"{s}\"")),
        Yaml::Integer(n) => YamlScalarShape::Literal(n.to_string()),
        Yaml::Real(s) => YamlScalarShape::Literal(s.clone()),
        Yaml::Boolean(b) => YamlScalarShape::Literal(b.to_string()),
        Yaml::Null => YamlScalarShape::Literal("null".to_string()),
        Yaml::Array(_) => YamlScalarShape::List,
        Yaml::Hash(_) => YamlScalarShape::Object,
        other => YamlScalarShape::Literal(format!("{other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(pak: &str, disabled: bool) -> PakEntry {
        PakEntry { pak: pak.to_string(), disabled }
    }

    fn names(cfg: &PakConfig) -> Vec<&str> {
        cfg.entries.iter().map(|e| e.pak.as_str()).collect()
    }

    fn no_history() -> HashMap<String, KnownState> {
        HashMap::new()
    }

    #[test]
    fn adds_unknown_paks_as_enabled_at_end() {
        // The engine loads unlisted paks anyway — so take them in enabled,
        // which is what makes them controllable.
        let mut cfg = PakConfig { entries: vec![entry("a.pak", false)] };
        let result = cfg.reconcile(&["a.pak".into(), "neu.pak".into()], &no_history());

        assert_eq!(names(&cfg), vec!["a.pak", "neu.pak"]);
        assert!(!cfg.entries[1].disabled);
        assert_eq!(result.added, vec!["neu.pak"]);
        assert!(result.removed.is_empty());
    }

    #[test]
    fn removes_entries_without_file() {
        let mut cfg = PakConfig {
            entries: vec![entry("a.pak", false), entry("weg.pak", true)],
        };
        let result = cfg.reconcile(&["a.pak".into()], &no_history());

        assert_eq!(names(&cfg), vec!["a.pak"]);
        assert_eq!(result.removed, vec!["weg.pak"]);
    }

    #[test]
    fn keeps_order_and_state_of_existing_entries() {
        let mut cfg = PakConfig {
            entries: vec![entry("z.pak", true), entry("a.pak", false)],
        };
        cfg.reconcile(&["a.pak".into(), "z.pak".into()], &no_history());

        assert_eq!(names(&cfg), vec!["z.pak", "a.pak"], "the order must not change");
        assert!(cfg.entries[0].disabled, "the disabled state must not get lost");
    }

    #[test]
    fn adds_multiple_new_paks_alphabetically() {
        let mut cfg = PakConfig::default();
        let result = cfg.reconcile(&["b.pak".into(), "a.pak".into()], &no_history());

        assert_eq!(names(&cfg), vec!["a.pak", "b.pak"]);
        assert_eq!(result.added, vec!["a.pak", "b.pak"]);
    }

    #[test]
    fn reconcile_without_change_reports_nothing() {
        let mut cfg = PakConfig { entries: vec![entry("a.pak", false)] };
        let result = cfg.reconcile(&["a.pak".into()], &no_history());

        assert!(result.is_empty());
    }

    /// A directory cannot contain the same file name twice, but `present` is
    /// a `&[String]`, not a set — a caller could pass the same name twice (by
    /// accident, for example by reading the directory in twice). Without
    /// deduplication, `reconcile` would turn that into two identical entries
    /// in the configuration — silent data corruption.
    #[test]
    fn deduplicates_repeatedly_reported_filenames() {
        let mut cfg = PakConfig::default();
        let result = cfg.reconcile(&["a.pak".into(), "a.pak".into()], &no_history());

        assert_eq!(names(&cfg), vec!["a.pak"], "must not produce a duplicate entry");
        assert_eq!(result.added, vec!["a.pak"]);
    }

    /// A hand-edited configuration can already contain a pak name twice.
    /// `reconcile` must not merge or reorder existing entries (see
    /// `keeps_order_and_state_of_existing_entries`) — an already present
    /// duplicate therefore stays untouched, instead of reconcile "repairing"
    /// it or adding yet another duplicate.
    #[test]
    fn leaves_existing_duplicates_in_config_untouched() {
        let mut cfg = PakConfig {
            entries: vec![entry("a.pak", false), entry("a.pak", true)],
        };
        let result = cfg.reconcile(&["a.pak".into()], &no_history());

        assert_eq!(
            cfg.entries,
            vec![entry("a.pak", false), entry("a.pak", true)],
            "existing duplicates are neither removed nor changed"
        );
        assert!(result.is_empty(), "an already known name is not a new find");
    }

    // --- Restoration from a known state (KnownState) -------------------

    /// The central case from spec §9 R3: a pak disappears (through a Steam
    /// update, for example), gets reconciled away (and thereby removed from
    /// the configuration), later reappears — and must then return to its old
    /// position with its old activation state, instead of being appended
    /// enabled at the end like a pak never seen before.
    #[test]
    fn a_pak_that_reappears_is_restored_to_its_previous_state_and_position() {
        let mut cfg = PakConfig {
            entries: vec![entry("a.pak", false), entry("c.pak", false)],
        };
        let mut history = HashMap::new();
        history.insert("b.pak".to_string(), KnownState { disabled: true, position: 1 });

        let result = cfg.reconcile(&["a.pak".into(), "b.pak".into(), "c.pak".into()], &history);

        assert_eq!(
            names(&cfg),
            vec!["a.pak", "b.pak", "c.pak"],
            "b.pak has to return to its old position (1)"
        );
        assert!(cfg.entries[1].disabled, "b.pak was disabled and has to be disabled again");
        assert_eq!(result.added, vec!["b.pak"]);
        assert_eq!(result.restored, vec!["b.pak"]);
        assert!(result.removed.is_empty());
    }

    /// A pak without a known state (never seen in the library before) keeps
    /// today's behavior: enabled at the end, not `restored`.
    #[test]
    fn a_pak_without_history_is_still_added_enabled_at_the_end() {
        let mut cfg = PakConfig { entries: vec![entry("a.pak", false)] };

        let result = cfg.reconcile(&["a.pak".into(), "neu.pak".into()], &no_history());

        assert_eq!(names(&cfg), vec!["a.pak", "neu.pak"]);
        assert!(!cfg.entries[1].disabled);
        assert_eq!(result.added, vec!["neu.pak"]);
        assert!(result.restored.is_empty(), "without a known state there is nothing to restore");
    }

    /// Several known paks reappearing at once must each land at their own old
    /// position, inserted in ascending order of position relative to one
    /// another.
    #[test]
    fn multiple_reappearing_known_paks_are_inserted_at_their_own_positions() {
        let mut cfg = PakConfig { entries: vec![entry("b.pak", false)] };
        let mut history = HashMap::new();
        history.insert("a.pak".to_string(), KnownState { disabled: false, position: 0 });
        history.insert("c.pak".to_string(), KnownState { disabled: true, position: 2 });

        cfg.reconcile(&["a.pak".into(), "b.pak".into(), "c.pak".into()], &history);

        assert_eq!(names(&cfg), vec!["a.pak", "b.pak", "c.pak"]);
        assert!(cfg.entries[2].disabled);
    }

    /// A stored position that reaches beyond the current number of entries
    /// (because other entries were removed in the meantime, for example) must
    /// not insert outside the vector — it is clamped to the end.
    #[test]
    fn a_stale_out_of_range_position_is_clamped_to_the_end() {
        let mut cfg = PakConfig::default();
        let mut history = HashMap::new();
        history.insert("a.pak".to_string(), KnownState { disabled: true, position: 99 });

        cfg.reconcile(&["a.pak".into()], &history);

        assert_eq!(names(&cfg), vec!["a.pak"]);
        assert!(cfg.entries[0].disabled);
    }

    #[test]
    fn reads_the_example_from_the_engine_readme() {
        let text = "- pak: mod_a.pak\n- pak: mod_b.pak\n  disabled: true\n";
        let cfg = PakConfig::parse(text).unwrap();
        assert_eq!(cfg.entries, vec![entry("mod_a.pak", false), entry("mod_b.pak", true)]);
    }

    #[test]
    fn reads_the_real_config_from_the_reference_system() {
        let cfg = PakConfig::parse("- pak: wa_astartes_14_1.pak\n").unwrap();
        assert_eq!(cfg.entries, vec![entry("wa_astartes_14_1.pak", false)]);
    }

    #[test]
    fn keeps_the_order() {
        let text = "- pak: z.pak\n- pak: a.pak\n- pak: m.pak\n";
        let cfg = PakConfig::parse(text).unwrap();
        let names: Vec<&str> = cfg.entries.iter().map(|e| e.pak.as_str()).collect();
        assert_eq!(names, vec!["z.pak", "a.pak", "m.pak"], "the order is the load order");
    }

    #[test]
    fn empty_file_yields_empty_config() {
        assert_eq!(PakConfig::parse("").unwrap(), PakConfig::default());
        assert_eq!(PakConfig::parse("\n\n").unwrap(), PakConfig::default());
        assert_eq!(PakConfig::parse("[]\n").unwrap(), PakConfig::default());
    }

    #[test]
    fn tolerates_comments_and_quotes() {
        let text = "# von Hand bearbeitet\n- pak: \"mit leerzeichen.pak\"\n  disabled: false\n";
        let cfg = PakConfig::parse(text).unwrap();
        assert_eq!(cfg.entries, vec![entry("mit leerzeichen.pak", false)]);
    }

    #[test]
    fn rejects_non_list_root() {
        let error = PakConfig::parse("pak: a.pak\n").unwrap_err();
        assert!(matches!(error, Error::PakConfig(PakConfigDefect::RootNotAList)));
    }

    #[test]
    fn rejects_entry_without_pak_key() {
        let error = PakConfig::parse("- disabled: true\n").unwrap_err();
        assert!(matches!(
            error,
            Error::PakConfig(PakConfigDefect::EntryMissingPakKey { index: 1 })
        ));
    }

    #[test]
    fn enabled_filters_out_disabled() {
        let cfg = PakConfig::parse("- pak: a.pak\n- pak: b.pak\n  disabled: true\n- pak: c.pak\n").unwrap();
        let names: Vec<&str> = cfg.enabled().map(|e| e.pak.as_str()).collect();
        assert_eq!(names, vec!["a.pak", "c.pak"]);
    }

    /// The syntax error carries a structured position instead of a
    /// ready-made sentence (see `PakConfigDefect`), so that `Display` can
    /// render it in either language and never leaks yaml-rust2's own
    /// (English) scanner wording, in whichever language is active.
    #[test]
    fn reports_yaml_syntax_error_with_position() {
        // A duplicate key in the same mapping is, per the YAML specification,
        // a scanner error in yaml-rust2, not merely an overwrite.
        let text = "- pak: a.pak\n  pak: b.pak\n";
        let error = PakConfig::parse(text).unwrap_err();
        let Error::PakConfig(PakConfigDefect::InvalidYaml { line, column }) = &error else {
            panic!("expected Error::PakConfig(InvalidYaml), got {error:?}");
        };
        assert!(*line >= 1 && *column >= 1, "line/column should be 1-based: {line}:{column}");

        let _guard = crate::i18n::language_test_lock();
        crate::i18n::set_language(crate::i18n::Language::English);
        let english = error.to_string();
        crate::i18n::set_language(crate::i18n::Language::German);
        let german = error.to_string();
        crate::i18n::set_language(crate::i18n::Language::English);

        for english_fragment in ["duplicated key", "mapping", "byte", "at byte"] {
            assert!(
                !english.contains(english_fragment),
                "the message must not carry raw scanner text from yaml-rust2 \
                 (found: {english_fragment:?}): {english:?}"
            );
        }
        assert!(german.contains("Zeile") && german.contains("Spalte"), "{german:?}");
    }

    #[test]
    fn rejects_non_boolean_disabled_value() {
        for text in [
            "- pak: a.pak\n  disabled: yes\n",
            "- pak: a.pak\n  disabled: on\n",
            "- pak: a.pak\n  disabled: 1\n",
            "- pak: a.pak\n  disabled: \"true\"\n",
        ] {
            let error = PakConfig::parse(text).unwrap_err();
            let Error::PakConfig(PakConfigDefect::EntryInvalidDisabledValue { index, .. }) = error
            else {
                panic!("expected Error::PakConfig(EntryInvalidDisabledValue) for {text:?}, got {error:?}");
            };
            assert_eq!(index, 1, "the message should name the entry (input: {text:?})");
        }
    }

    /// A `disabled:` value that is itself a YAML list or mapping carries no
    /// data to render, only a shape — and unlike the literal cases above
    /// (a quoted string, a number, `true`/`false`, `null`), "a list" / "an
    /// object" is text that needs translating. This guards against baking
    /// that word in at parse time (see `YamlScalarShape`): `PakConfig::parse`
    /// runs once, but the resulting error can be displayed later, after a
    /// live language switch (Task 9 made the GUI's language switch live), so
    /// parse-time and display-time language are not guaranteed to match.
    #[test]
    fn disabled_value_that_is_a_list_or_mapping_translates_at_display_time() {
        let _guard = crate::i18n::language_test_lock();

        for (text, is_list) in [
            ("- pak: a.pak\n  disabled: [true]\n", true),
            ("- pak: a.pak\n  disabled: {a: 1}\n", false),
        ] {
            // Parse while German is the active language …
            crate::i18n::set_language(crate::i18n::Language::German);
            let error = PakConfig::parse(text).unwrap_err();
            let Error::PakConfig(PakConfigDefect::EntryInvalidDisabledValue { found, .. }) = &error
            else {
                panic!("expected Error::PakConfig(EntryInvalidDisabledValue) for {text:?}, got {error:?}");
            };
            match (found, is_list) {
                (YamlScalarShape::List, true) | (YamlScalarShape::Object, false) => {}
                _ => panic!("wrong shape stored for {text:?}: {found:?}"),
            }

            // … but display it in English: the wording must follow the
            // language active at `Display` time, not the one active while
            // `parse` ran.
            crate::i18n::set_language(crate::i18n::Language::English);
            let english = error.to_string();
            crate::i18n::set_language(crate::i18n::Language::English);

            let expected = if is_list { "a list" } else { "an object" };
            assert!(english.contains(expected), "{english:?}");
            assert!(
                !english.contains("eine Liste") && !english.contains("ein Objekt"),
                "the wording must not be frozen into German from parse time: {english:?}"
            );
        }
    }

    #[test]
    fn ignores_utf8_bom_at_start_of_file() {
        let text = "\u{feff}- pak: mod_a.pak\n- pak: mod_b.pak\n  disabled: true\n";
        let cfg = PakConfig::parse(text).unwrap();
        assert_eq!(cfg.entries, vec![entry("mod_a.pak", false), entry("mod_b.pak", true)]);
    }

    #[test]
    fn bom_alone_yields_empty_config() {
        assert_eq!(PakConfig::parse("\u{feff}").unwrap(), PakConfig::default());
    }

    #[test]
    fn writes_the_format_from_the_engine_readme() {
        let cfg = PakConfig {
            entries: vec![entry("mod_a.pak", false), entry("mod_b.pak", true)],
        };
        assert_eq!(cfg.to_yaml(), "- pak: mod_a.pak\n- pak: mod_b.pak\n  disabled: true\n");
    }

    #[test]
    fn omits_disabled_for_active_entries() {
        let cfg = PakConfig { entries: vec![entry("a.pak", false)] };
        assert_eq!(cfg.to_yaml(), "- pak: a.pak\n");
    }

    #[test]
    fn writes_empty_list_as_valid_yaml() {
        // An empty file would be YAML null, not a list. "[]" is unambiguous.
        assert_eq!(PakConfig::default().to_yaml(), "[]\n");
    }

    #[test]
    fn quotes_names_with_special_characters() {
        let cfg = PakConfig { entries: vec![entry("mit: doppelpunkt.pak", false)] };
        assert_eq!(cfg.to_yaml(), "- pak: \"mit: doppelpunkt.pak\"\n");
    }

    #[test]
    fn round_trip_preserves_the_config() {
        let original = PakConfig {
            entries: vec![
                entry("z.pak", false),
                entry("mit: doppelpunkt.pak", true),
                entry("a.pak", false),
            ],
        };
        let roundtripped = PakConfig::parse(&original.to_yaml()).unwrap();
        assert_eq!(roundtripped, original);
    }

    #[test]
    fn load_with_missing_file_yields_empty_config() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = PakConfig::load(&dir.path().join("gibt_es_nicht.yaml")).unwrap();
        assert_eq!(cfg, PakConfig::default());
    }

    #[test]
    fn save_and_load_are_inverses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pak_config.yaml");
        let cfg = PakConfig { entries: vec![entry("a.pak", true), entry("b.pak", false)] };

        cfg.save(&path).unwrap();

        assert_eq!(PakConfig::load(&path).unwrap(), cfg);
    }

    /// Names that contain none of the "obvious" special characters but that
    /// unquoted would be read as a YAML bool, number or null instead of a
    /// string. Without the extra scalar check in `quote_if_needed`, `parse`
    /// would reject these entries as missing the 'pak' key.
    #[test]
    fn round_trips_names_that_would_be_read_as_yaml_scalars() {
        for name in [
            "true", "True", "TRUE", "false", "False", "FALSE", "null", "~", "123", "-123", "0",
            "0x1F", "0o17", "1.5", "-1.5", ".inf", "-.inf", ".nan",
        ] {
            let cfg = PakConfig { entries: vec![entry(name, false)] };
            let roundtripped = PakConfig::parse(&cfg.to_yaml()).unwrap();
            assert_eq!(roundtripped, cfg, "the name {name:?} does not survive the round trip");
        }
    }

    /// Embedded control characters and special characters that the escaping
    /// logic in `quote_if_needed` has to handle separately, so that reading
    /// them back does not break the YAML structure or fold them into a single
    /// space.
    #[test]
    fn round_trips_names_with_embedded_special_characters() {
        for name in ["a\nb.pak", "a\rb.pak", "a\\b.pak", "a'b.pak", "  a.pak  "] {
            let cfg = PakConfig { entries: vec![entry(name, false)] };
            let roundtripped = PakConfig::parse(&cfg.to_yaml()).unwrap();
            assert_eq!(roundtripped, cfg, "the name {name:?} does not survive the round trip");
        }
    }
}

# Mehrsprachigkeit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Jeder nutzersichtbare Text kommt aus einer Sprachdatei; Englisch ist Standard, Deutsch umschaltbar, eine weitere Sprache kostet eine Datei und eine Zeile Code.

**Architecture:** Ein Katalogmodul in `crates/core` bettet je Sprache eine TOML-Datei ein (`include_str!`), ebnet sie zu Punkt-Schlüsseln ein und liefert Texte über ein `t!`-Makro; die aktive Sprache steht in einem globalen `RwLock`. `Error` verliert `thiserror` und bekommt ein handgeschriebenes `Display`, das je Variante einen Schlüssel nachschlägt. Der `clap`-Befehlsbaum wird vor dem Parsen einmal durchlaufen und mit Katalogtexten bestückt.

**Tech Stack:** Rust (core 1.85, app 1.95), `toml` + `serde` (bereits vorhanden), `clap` 4, `egui` 0.36. Neue Abhängigkeiten: keine. Entfernte Abhängigkeit: `thiserror` in `crates/core`.

**Spec:** `docs/superpowers/specs/2026-09-16-mehrsprachigkeit-design.md`

## Global Constraints

- **Kommentare sind Englisch**, Kommentarumbruch bei 78 Spalten inklusive `///`-Präfix (`CLAUDE.md`).
- **Nutzertext steht nie im Code.** Ab Task 1 gilt: jeder Satz, den ein Mensch sieht, kommt über `t!` aus dem Katalog. Neue Literale in Rust-Dateien sind ein Fehler, keine Abkürzung.
- **`en.toml` ist die Quelle der Wahrheit.** `de.toml` hat exakt dieselbe Schlüsselmenge — nicht mehr, nicht weniger.
- **Deutsche Texte werden übernommen, nicht neu erfunden.** Beim Migrieren wandert der heutige deutsche Wortlaut unverändert nach `de.toml`; die englische Fassung ist die Neuschöpfung.
- **Nach jedem Task:** `cargo test` grün und `cargo clippy --all-targets` ohne Ausgabe. Kein Task endet rot.
- **Verhalten ändert sich nicht.** Bestehende Tests prüfen Verhalten; wo einer heute auf deutschen Wortlaut prüft, wird er auf Verhalten oder auf den Schlüssel umgestellt, niemals gelöscht.
- **Schlüsselnamen:** `error.*`, `cli.*`, `gui.*`, `format.*`; Wörter in `snake_case`, Bereiche durch Punkte.

---

### Task 1: Katalog, Sprachen und das `t!`-Makro

**Files:**
- Create: `crates/core/src/i18n.rs`
- Create: `crates/core/i18n/en.toml`
- Create: `crates/core/i18n/de.toml`
- Create: `crates/core/tests/i18n_keys.rs`
- Modify: `crates/core/src/lib.rs` (Modul anmelden)

**Interfaces:**
- Produces: `sm2_core::i18n::Language` (`English`, `German`) mit `ALL: [Language; 2]`, `code() -> &'static str`, `native_name() -> &'static str`, `from_code(&str) -> Option<Language>`; `sm2_core::i18n::language() -> Language`; `sm2_core::i18n::set_language(Language)`; `sm2_core::i18n::lookup(&str) -> &'static str`; `sm2_core::i18n::format(&str, &[(&str, String)]) -> String`; `sm2_core::i18n::has_key(&str) -> bool`; Makro `t!` (per `#[macro_export]` unter `sm2_core::t!`).
- Consumes: nichts.

- [ ] **Step 1: Sprachdateien anlegen**

`crates/core/i18n/en.toml`:

```toml
[demo]
greeting = "Backup created"
with_value = "Backup {created_at} verified"
```

`crates/core/i18n/de.toml`:

```toml
[demo]
greeting = "Backup angelegt"
with_value = "Backup {created_at} geprüft"
```

Die beiden `demo`-Schlüssel sind das Testmaterial dieses Tasks. Sie bleiben bestehen, bis Task 4 echte Schlüssel einträgt — dann werden sie ersatzlos gelöscht (siehe Task 4, Step 6).

- [ ] **Step 2: Die Tests schreiben**

`crates/core/src/i18n.rs` — vorerst **nur** das Testmodul:

```rust
//! The message catalogue: one embedded TOML file per language, looked up
//! through the `t!` macro.

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises the tests that touch the global language, so that they
    /// cannot see each other's switching.
    fn with_language<T>(language: Language, body: impl FnOnce() -> T) -> T {
        static GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _held = GUARD.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = self::language();
        set_language(language);
        let result = body();
        set_language(before);
        result
    }

    #[test]
    fn english_is_the_language_before_anything_is_set() {
        assert_eq!(Language::default(), Language::English);
    }

    #[test]
    fn a_key_resolves_to_the_text_of_the_active_language() {
        with_language(Language::German, || {
            assert_eq!(lookup("demo.greeting"), "Backup angelegt");
        });
        with_language(Language::English, || {
            assert_eq!(lookup("demo.greeting"), "Backup created");
        });
    }

    #[test]
    fn placeholders_are_replaced_by_name() {
        with_language(Language::English, || {
            let text = format("demo.with_value", &[("created_at", String::from("2026-09-16"))]);
            assert_eq!(text, "Backup 2026-09-16 verified");
        });
    }

    /// A missing key must stay visible instead of crashing or leaving an
    /// empty label behind: the key itself is the most useful thing to show.
    #[test]
    fn an_unknown_key_yields_the_key_itself() {
        assert_eq!(lookup("nope.not.here"), "nope.not.here");
        assert!(!has_key("nope.not.here"));
    }

    #[test]
    fn a_code_maps_to_its_language_and_back() {
        assert_eq!(Language::from_code("de"), Some(Language::German));
        assert_eq!(Language::from_code("klingon"), None);
        for language in Language::ALL {
            assert_eq!(Language::from_code(language.code()), Some(language));
        }
    }

    /// The test that signs off a new translation: every language carries
    /// exactly the keys of the English one. A missing key would show up as
    /// its own name in the window, a surplus one is a typo nobody reads.
    #[test]
    fn every_language_has_exactly_the_english_keys() {
        let english: Vec<&String> = Language::English.catalog().keys().collect();
        for language in Language::ALL {
            let keys: Vec<&String> = language.catalog().keys().collect();
            let missing: Vec<_> = english.iter().filter(|k| !keys.contains(k)).collect();
            let surplus: Vec<_> = keys.iter().filter(|k| !english.contains(k)).collect();
            assert!(missing.is_empty(), "{} lacks: {missing:?}", language.code());
            assert!(surplus.is_empty(), "{} has extra: {surplus:?}", language.code());
        }
    }

    /// A `{naem}` typo in a translation would otherwise sit unreplaced on
    /// screen: every placeholder of a translation must exist in the
    /// English original.
    #[test]
    fn every_translation_uses_only_the_placeholders_of_the_original() {
        for language in Language::ALL {
            for (key, text) in language.catalog() {
                let original = Language::English.catalog().get(key).expect("checked by the test above");
                for name in placeholders(text) {
                    assert!(
                        placeholders(original).contains(&name),
                        "{}: {key} uses {{{name}}}, the English text does not",
                        language.code()
                    );
                }
            }
        }
    }

    #[test]
    fn the_macro_resolves_with_and_without_arguments() {
        with_language(Language::English, || {
            assert_eq!(crate::t!("demo.greeting"), "Backup created");
            assert_eq!(
                crate::t!("demo.with_value", created_at = "2026-09-16"),
                "Backup 2026-09-16 verified"
            );
        });
    }
}
```

- [ ] **Step 3: Tests laufen lassen und Fehlschlag prüfen**

Run: `cargo test -p sm2-core --lib i18n`
Expected: FAIL, Kompilierfehler „cannot find type `Language` in this scope" und Konsorten — die Mechanik fehlt noch.

- [ ] **Step 4: Die Mechanik schreiben**

Vor das Testmodul in `crates/core/src/i18n.rs`:

```rust
use std::collections::BTreeMap;
use std::sync::{OnceLock, RwLock};

/// The languages the program speaks. Adding one means: put the file next
/// to the others, add a variant here, name it in `ALL` — nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    English,
    German,
}

impl Language {
    /// Every language, in the order they appear in the picker. The index
    /// into this array doubles as the index into the parsed catalogues
    /// (see `catalog`), so the order must match the variant order.
    pub const ALL: [Language; 2] = [Language::English, Language::German];

    /// The code stored in the settings file.
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::German => "de",
        }
    }

    /// The name of the language in that language — a picker that offers
    /// "German" to someone who cannot read English helps nobody.
    pub fn native_name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::German => "Deutsch",
        }
    }

    pub fn from_code(code: &str) -> Option<Language> {
        Language::ALL.into_iter().find(|language| language.code() == code)
    }

    fn source(self) -> &'static str {
        match self {
            Language::English => include_str!("../i18n/en.toml"),
            Language::German => include_str!("../i18n/de.toml"),
        }
    }

    /// The parsed catalogue, built once per program run. Indexed by the
    /// position in `ALL`, which is why `ALL` and the variants stay in the
    /// same order.
    fn catalog(self) -> &'static BTreeMap<String, String> {
        static CACHE: OnceLock<Vec<BTreeMap<String, String>>> = OnceLock::new();
        let all = CACHE.get_or_init(|| Language::ALL.iter().map(|l| parse(l.source())).collect());
        &all[Language::ALL.iter().position(|l| *l == self).expect("ALL holds every variant")]
    }
}

/// The active language. Global on purpose: the GUI's background threads
/// call into the core, and threading a catalogue reference through every
/// call site would be noise in exchange for a setting that changes a
/// handful of times per run at most.
static CURRENT: RwLock<Language> = RwLock::new(Language::English);

pub fn language() -> Language {
    *CURRENT.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn set_language(language: Language) {
    *CURRENT.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = language;
}

/// The text for a key: from the active language, otherwise from English
/// (a half-finished translation should show the original, not a gap), and
/// otherwise the key itself — see `an_unknown_key_yields_the_key_itself`.
///
/// Returns `String` rather than `&'static str` precisely because of that
/// last case: the key belongs to the caller and does not live long enough
/// to be handed back by reference.
pub fn lookup(key: &str) -> String {
    language()
        .catalog()
        .get(key)
        .or_else(|| Language::English.catalog().get(key))
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

/// Like `lookup`, but replaces `{name}` placeholders.
pub fn format(key: &str, arguments: &[(&str, String)]) -> String {
    let mut text = lookup(key);
    for (name, value) in arguments {
        text = text.replace(&std::format!("{{{name}}}"), value);
    }
    text
}

pub fn has_key(key: &str) -> bool {
    Language::English.catalog().contains_key(key)
}

/// Flattens the nested tables of a language file into dotted keys:
/// `[gui.saves] import_button = "…"` becomes `gui.saves.import_button`.
///
/// A malformed file is a build-time mistake, not a runtime condition: the
/// files are embedded, so a panic here can only ever fire in a test run of
/// a broken commit, never in a shipped binary.
fn parse(source: &str) -> BTreeMap<String, String> {
    let value: toml::Value = toml::from_str(source).expect("language file is not valid TOML");
    let mut flat = BTreeMap::new();
    flatten("", &value, &mut flat);
    flat
}

fn flatten(prefix: &str, value: &toml::Value, out: &mut BTreeMap<String, String>) {
    match value {
        toml::Value::Table(table) => {
            for (name, child) in table {
                let key =
                    if prefix.is_empty() { name.clone() } else { std::format!("{prefix}.{name}") };
                flatten(&key, child, out);
            }
        }
        toml::Value::String(text) => {
            out.insert(prefix.to_string(), text.clone());
        }
        other => panic!("{prefix} is {other:?}, only strings and tables belong in a language file"),
    }
}

/// The placeholder names in a text, without the braces.
fn placeholders(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                names.push(after[..end].to_string());
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    names
}
```

Das Makro ans Ende der Datei (außerhalb des Testmoduls):

```rust
/// Looks a text up in the active language.
///
/// `t!("gui.saves.import_button")` yields the text, `t!("saves.verified",
/// created_at = entry.created_at)` replaces `{created_at}` as well. Both
/// forms return `String`, so call sites do not have to care which one they
/// are using.
#[macro_export]
macro_rules! t {
    ($key:literal) => {
        $crate::i18n::lookup($key)
    };
    ($key:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::format($key, &[$((stringify!($name), $value.to_string())),+])
    };
}
```

In `crates/core/src/lib.rs` das Modul anmelden (alphabetisch zwischen den bestehenden `pub mod`-Zeilen):

```rust
pub mod i18n;
```

- [ ] **Step 5: Tests laufen lassen**

Run: `cargo test -p sm2-core --lib i18n`
Expected: PASS, acht Tests.

- [ ] **Step 6: Den Schlüssel-Sweep als Integrationstest schreiben**

`crates/core/tests/i18n_keys.rs`:

```rust
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
fn keys_in(text: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut rest = text;
    while let Some(position) = rest.find("t!(\"") {
        let after = &rest[position + 4..];
        match after.find('"') {
            Some(end) => {
                keys.push(after[..end].to_string());
                rest = &after[end..];
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
```

- [ ] **Step 7: Integrationstest laufen lassen**

Run: `cargo test -p sm2-core --test i18n_keys`
Expected: PASS (im Code steht noch kein `t!`, der Test läuft also über eine leere Menge — er wird ab Task 3 scharf).

- [ ] **Step 8: Clippy und Commit**

```bash
cargo clippy --all-targets
git add crates/core/src/i18n.rs crates/core/i18n crates/core/src/lib.rs crates/core/tests/i18n_keys.rs
git commit -m "feat(core): Katalog für mehrsprachige Texte

Eine TOML-Datei je Sprache, beim Bauen eingebettet und zu Punkt-
Schlüsseln eingeebnet, dazu das t!-Makro und ein globaler Schalter für
die aktive Sprache. Die Tests sind der eigentliche Gehalt: jede Sprache
hat exakt die Schlüssel der englischen, jede Übersetzung benutzt nur
deren Platzhalter, und ein Integrationstest prüft jeden im Code
benutzten Schlüssel gegen en.toml.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 2: Sprache in den Einstellungen

**Files:**
- Modify: `crates/core/src/settings.rs` (Feld, Default, Tests)
- Modify: `crates/app/src/app_state.rs` (Sprache beim Laden setzen)

**Interfaces:**
- Consumes: `sm2_core::i18n::{Language, set_language}` aus Task 1.
- Produces: `Settings.language: Option<String>`; `Settings::language() -> Language` (übersetzt den gespeicherten Code, fällt auf Englisch zurück); `AppState` setzt die Sprache beim Laden.

- [ ] **Step 1: Die Tests schreiben**

In das Testmodul von `crates/core/src/settings.rs`:

```rust
#[test]
fn a_saved_language_survives_a_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.toml");
    let mut settings = Settings::default();
    settings.language = Some(String::from("de"));

    settings.save(&path).unwrap();
    let loaded = Settings::load(&path).unwrap();

    assert_eq!(loaded.language.as_deref(), Some("de"));
    assert_eq!(loaded.language(), Language::German);
}

#[test]
fn without_a_setting_the_program_speaks_english() {
    assert_eq!(Settings::default().language(), Language::English);
}

/// A settings file edited by hand must not keep the program from
/// starting: an unknown code falls back instead of failing.
#[test]
fn an_unknown_language_code_falls_back_to_english() {
    let settings = Settings { language: Some(String::from("klingon")), ..Settings::default() };

    assert_eq!(settings.language(), Language::English);
}
```

Der Import oben in `settings.rs` wird um `use crate::i18n::Language;` ergänzt.

- [ ] **Step 2: Tests laufen lassen und Fehlschlag prüfen**

Run: `cargo test -p sm2-core --lib settings`
Expected: FAIL, „no field `language` on type `Settings`".

- [ ] **Step 3: Feld und Methode ergänzen**

In `crates/core/src/settings.rs`:

```rust
    /// The language code from `Language::code`, empty until the user
    /// chooses one. See `Settings::language`.
    pub language: Option<String>,
```

im `Default`-Block `language: None,` ergänzen und die Methode anfügen:

```rust
    /// The chosen language. An unknown code — a settings file edited by
    /// hand, or one from a newer version — falls back to English rather
    /// than keeping the program from starting.
    pub fn language(&self) -> Language {
        self.language.as_deref().and_then(Language::from_code).unwrap_or_default()
    }
```

- [ ] **Step 4: Tests laufen lassen**

Run: `cargo test -p sm2-core --lib settings`
Expected: PASS.

- [ ] **Step 5: Die Sprache beim Start setzen**

In `crates/app/src/app_state.rs`, unmittelbar nachdem die Einstellungen geladen sind (dieselbe Stelle, an der heute `settings` in den `AppState` wandert):

```rust
    // Before anything can report something: every message from here on
    // goes through the catalogue.
    sm2_core::i18n::set_language(settings.language());
```

- [ ] **Step 6: Einen unbekannten Code melden**

Ein stiller Rückfall auf Englisch wäre verwirrend: wer `language = "dutch"`
in die Datei schreibt, soll erfahren, warum nichts passiert. Direkt nach
dem Setzen der Sprache in `app_state.rs`:

```rust
    if let Some(code) = settings.language.as_deref() {
        if Language::from_code(code).is_none() {
            notices.push(Notice::warning(t!("app.notice.unknown_language", code = code)));
        }
    }
```

Schlüssel `app.notice.unknown_language` in beiden Sprachdateien:
`"Unknown language '{code}' in the settings – falling back to English"`
bzw. `"Unbekannte Sprache '{code}' in den Einstellungen – es bleibt bei
Englisch"`. `notices` ist der Vektor, den `AppState` an dieser Stelle
ohnehin befüllt.

- [ ] **Step 7: Tests, Clippy, Commit**

```bash
cargo test
cargo clippy --all-targets
git add crates/core/src/settings.rs crates/app/src/app_state.rs crates/core/i18n
git commit -m "feat(core,app): Sprache als Einstellung

settings.toml merkt sich einen Sprachcode, ein unbekannter fällt auf
Englisch zurück statt den Start zu verhindern. AppState setzt die
Sprache, sobald die Einstellungen gelesen sind.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 3: Fehlertexte aus dem Katalog

**Files:**
- Modify: `crates/core/src/error.rs` (vollständiger Umbau)
- Modify: `crates/core/src/saves.rs` (Aufrufstellen der beiden Defekt-Enums)
- Modify: `crates/core/src/import.rs`, `crates/core/src/library.rs`, `crates/core/src/pak_config.rs`, `crates/core/src/paths.rs`, `crates/core/src/profile.rs` (Aufrufstellen, soweit sie `CorruptBackup`/`UnusableArchive` bauen)
- Modify: `crates/core/i18n/en.toml`, `crates/core/i18n/de.toml`
- Modify: `crates/core/Cargo.toml` (`thiserror` entfernen)

**Interfaces:**
- Consumes: `t!`, `i18n::format` aus Task 1.
- Produces: `Error` ohne `thiserror`, mit handgeschriebenem `Display` und `source()`; `BackupDefect` und `ArchiveDefect` als öffentliche Enums; `Error::CorruptBackup(BackupDefect)`, `Error::UnusableArchive(ArchiveDefect)`.

- [ ] **Step 1: Schlüssel eintragen**

In `crates/core/i18n/en.toml` (und spiegelbildlich in `de.toml` mit dem **heutigen deutschen Wortlaut** aus den `#[error(…)]`-Attributen):

```toml
[error]
steam_not_found = "Steam installation not found"
game_not_found = "Space Marine 2 (AppID {app_id}) is not installed in any Steam library"
not_a_game_dir = "This does not look like a Space Marine 2 directory: {path} (expected: client_pc/root/mods)"
prefix_missing = "The Proton prefix for AppID {app_id} is missing – the game has to be started at least once"
no_save_user = "No Steam user profile found below {path}"
ambiguous_save_user = "Several Steam user profiles found ({users}) – please pick one in the settings"
unknown_save_user = "The Steam user profile '{requested}' from the settings was not found (available: {available})"
pak_config = "pak_config.yaml is malformed: {detail}"
corrupt_backup = "Backup damaged: {detail}"
unsafe_save_dir = "The save directory holds a symlink in a safety-relevant place, restore aborted: {path}"
restore_failed_after_backup = "Restore failed after a copy of the previous state had already been made (it is at {safety_backup}): {source}"
no_rar_tool = "No tool for unpacking .rar found – please install 'unar' or '7zip' (Debian/Ubuntu: apt install unar, Arch: pacman -S unarchiver, Fedora: dnf install unar)"
no_pak_in_archive = "The archive holds no .pak file: {path}"
no_save_in_archive = "The archive holds no savegame files (.cfg or .sav): {path}"
unusable_archive = "The archive cannot be imported: {detail}"
not_writable = "No write permission for {path}"
io = "I/O error at {path}: {source}"
invalid_toml = "Invalid TOML (line {line}, column {column})"

[error.backup_defect]
not_a_zip = "the archive cannot be opened as a ZIP: {path}"
unknown_entry = "{name} is not listed in the manifest"
duplicate_entry = "{name} appears more than once in the archive"
size_mismatch = "{name} has a different size"
hash_mismatch = "{name} has a different hash"
count_mismatch = "the archive holds {found} of {expected} expected files"
invalid_path = "invalid path in the archive: {name}"
empty_path = "invalid (empty) path in the archive"
outside_save_dir = "path outside the save directory"

[error.archive_defect]
not_a_zip = "the file cannot be opened as a ZIP: {path}"
symlink = "the archive holds a symlink: {name}"
invalid_path = "invalid path in the archive: {name}"
duplicate_name = "{name} appears more than once in the archive"
too_large = "the archive unpacks to more than {limit}"

[error.size]
mib = "{value} MiB"
bytes = "{value} bytes"
```

Die `demo`-Schlüssel aus Task 1 bleiben vorerst stehen — sie tragen noch die Tests aus Task 1.

- [ ] **Step 2: Den Test für den Sprachwechsel schreiben**

In `crates/core/src/error.rs`, Testmodul:

```rust
    use crate::i18n::{set_language, Language};

    /// The point of the whole rebuild: the same error speaks whichever
    /// language is set, without a single caller changing.
    #[test]
    fn an_error_speaks_the_active_language() {
        let error = Error::GameNotFound(2183900);

        set_language(Language::English);
        let english = error.to_string();
        set_language(Language::German);
        let german = error.to_string();
        set_language(Language::English);

        assert!(english.contains("is not installed"), "{english}");
        assert!(german.contains("ist in keiner"), "{german}");
        assert!(english.contains("2183900") && german.contains("2183900"));
    }

    #[test]
    fn a_defect_reason_is_part_of_the_message() {
        set_language(Language::English);
        let error = Error::CorruptBackup(BackupDefect::DuplicateEntry { name: "slot1.sav".into() });

        let text = error.to_string();

        assert!(text.contains("slot1.sav"), "{text}");
        assert!(text.contains("more than once"), "{text}");
    }

    /// The chain has to survive the loss of `thiserror`: an I/O error
    /// still names its cause.
    #[test]
    fn an_io_error_keeps_its_source() {
        let error = Error::io("/tmp/x", std::io::Error::new(std::io::ErrorKind::NotFound, "weg"));

        assert!(std::error::Error::source(&error).is_some());
    }
```

- [ ] **Step 3: Tests laufen lassen und Fehlschlag prüfen**

Run: `cargo test -p sm2-core --lib error`
Expected: FAIL, „cannot find type `BackupDefect`" und ein Wortlaut-Fehlschlag beim Sprachwechsel.

- [ ] **Step 4: `error.rs` umbauen**

`thiserror` verschwindet vollständig. Das Grundgerüst — jede Variante behält ihre Felder, nur die Attribute fallen weg:

```rust
use crate::i18n;
use std::path::PathBuf;

#[derive(Debug)]
pub enum Error {
    SteamNotFound,
    GameNotFound(u32),
    NotAGameDir(PathBuf),
    PrefixMissing(u32),
    NoSaveUser(PathBuf),
    AmbiguousSaveUser(Vec<String>),
    UnknownSaveUser { requested: String, available: Vec<String> },
    PakConfig(String),
    CorruptBackup(BackupDefect),
    UnsafeSaveDir(PathBuf),
    RestoreFailedAfterBackup { safety_backup: PathBuf, source: Box<Error> },
    NoRarTool,
    NoPakInArchive(PathBuf),
    NoSaveInArchive(PathBuf),
    UnusableArchive(ArchiveDefect),
    NotWritable(PathBuf),
    Io { path: PathBuf, source: std::io::Error },
    PlainIo(std::io::Error),
}

/// Why a backup failed verification. Its own type rather than a ready-made
/// sentence, so that the detail can be translated as well.
#[derive(Debug)]
pub enum BackupDefect {
    NotAZip { path: PathBuf },
    UnknownEntry { name: String },
    DuplicateEntry { name: String },
    SizeMismatch { name: String },
    HashMismatch { name: String },
    CountMismatch { found: usize, expected: usize },
    InvalidPath { name: String },
    EmptyPath,
    OutsideSaveDir,
}

/// Why an archive cannot be imported — same idea as `BackupDefect`.
#[derive(Debug)]
pub enum ArchiveDefect {
    NotAZip { path: PathBuf },
    Symlink { name: String },
    InvalidPath { name: String },
    DuplicateName { name: String },
    TooLarge { limit: u64 },
}

impl BackupDefect {
    fn text(&self) -> String {
        match self {
            BackupDefect::NotAZip { path } => {
                i18n::format("error.backup_defect.not_a_zip", &[("path", path.display().to_string())])
            }
            BackupDefect::UnknownEntry { name } => {
                i18n::format("error.backup_defect.unknown_entry", &[("name", name.clone())])
            }
            BackupDefect::DuplicateEntry { name } => {
                i18n::format("error.backup_defect.duplicate_entry", &[("name", name.clone())])
            }
            BackupDefect::SizeMismatch { name } => {
                i18n::format("error.backup_defect.size_mismatch", &[("name", name.clone())])
            }
            BackupDefect::HashMismatch { name } => {
                i18n::format("error.backup_defect.hash_mismatch", &[("name", name.clone())])
            }
            BackupDefect::CountMismatch { found, expected } => i18n::format(
                "error.backup_defect.count_mismatch",
                &[("found", found.to_string()), ("expected", expected.to_string())],
            ),
            BackupDefect::InvalidPath { name } => {
                i18n::format("error.backup_defect.invalid_path", &[("name", name.clone())])
            }
            BackupDefect::EmptyPath => i18n::lookup("error.backup_defect.empty_path"),
            BackupDefect::OutsideSaveDir => i18n::lookup("error.backup_defect.outside_save_dir"),
        }
    }
}
```

`ArchiveDefect::text` folgt demselben Muster; `TooLarge { limit }` benutzt `describe_size` (das aus `saves.rs` hierher wandert, weil es jetzt Fehlertext ist) mit den Schlüsseln `error.size.mib` und `error.size.bytes`.

`Display` für `Error` — eine Verzweigung je Variante, alle nach demselben Schema:

```rust
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Error::SteamNotFound => i18n::lookup("error.steam_not_found"),
            Error::GameNotFound(app_id) => {
                i18n::format("error.game_not_found", &[("app_id", app_id.to_string())])
            }
            Error::NotAGameDir(path) => {
                i18n::format("error.not_a_game_dir", &[("path", path.display().to_string())])
            }
            Error::AmbiguousSaveUser(users) => {
                i18n::format("error.ambiguous_save_user", &[("users", users.join(", "))])
            }
            Error::UnknownSaveUser { requested, available } => i18n::format(
                "error.unknown_save_user",
                &[("requested", requested.clone()), ("available", available.join(", "))],
            ),
            Error::CorruptBackup(defect) => {
                i18n::format("error.corrupt_backup", &[("detail", defect.text())])
            }
            Error::UnusableArchive(defect) => {
                i18n::format("error.unusable_archive", &[("detail", defect.text())])
            }
            Error::RestoreFailedAfterBackup { safety_backup, source } => i18n::format(
                "error.restore_failed_after_backup",
                &[
                    ("safety_backup", safety_backup.display().to_string()),
                    ("source", source.to_string()),
                ],
            ),
            Error::Io { path, source } => i18n::format(
                "error.io",
                &[("path", path.display().to_string()), ("source", source.to_string())],
            ),
            Error::PlainIo(source) => source.to_string(),
            // … the remaining variants follow the same two shapes:
            // `lookup` without fields, `format` with them.
        };
        f.write_str(&text)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            Error::PlainIo(source) => Some(source),
            Error::RestoreFailedAfterBackup { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(source: std::io::Error) -> Self {
        Error::PlainIo(source)
    }
}
```

`describe_toml_error` bleibt, benutzt aber den Schlüssel `error.invalid_toml` mit `{line}`/`{column}`.

- [ ] **Step 5: Aufrufstellen umstellen**

Jedes `Error::CorruptBackup(format!(…))` wird zum passenden `BackupDefect`, jedes `Error::UnusableArchive(format!(…))` zum passenden `ArchiveDefect`. Der Compiler zeigt jede Stelle; die Zuordnung ergibt sich aus dem heutigen deutschen Satz:

| heutiger Satz | neue Variante |
|---|---|
| „Archiv lässt sich nicht als ZIP öffnen: …" | `BackupDefect::NotAZip` |
| „… steht nicht im Manifest" | `BackupDefect::UnknownEntry` |
| „… kommt mehrfach im Archiv vor" | `BackupDefect::DuplicateEntry` / `ArchiveDefect::DuplicateName` |
| „… hat eine abweichende Größe" | `BackupDefect::SizeMismatch` |
| „… hat einen abweichenden Hash" | `BackupDefect::HashMismatch` |
| „Archiv enthält … von … erwarteten Dateien" | `BackupDefect::CountMismatch` |
| „unzulässiger Pfad im Archiv: …" | `BackupDefect::InvalidPath` / `ArchiveDefect::InvalidPath` |
| „unzulässiger (leerer) Pfad im Archiv" | `BackupDefect::EmptyPath` |
| „Pfad außerhalb des Save-Verzeichnisses" | `BackupDefect::OutsideSaveDir` |
| „Datei lässt sich nicht als ZIP öffnen: …" | `ArchiveDefect::NotAZip` |
| „Archiv enthält einen Symlink: …" | `ArchiveDefect::Symlink` |
| „Archiv ist entpackt größer als …" | `ArchiveDefect::TooLarge` |

Außerdem stecken deutsche Sätze an zwei Stellen, die kein `Error`-Attribut
sind und beim Compiler-Durchlauf deshalb nicht auffallen: in
`Error::PlainIo(std::io::Error::new(…, "…"))` (zum Beispiel
„Basisverzeichnisse des Systems nicht ermittelbar" in `paths.rs`) und in
den `#[error]`-freien Meldungen, die `saves.rs` und `library.rs` selbst
bauen. `grep -rn '"[^"]*[äöüß]' crates/core/src` findet sie; jede davon
bekommt einen Schlüssel unter `error.*`.

`describe_size` wandert aus `saves.rs` nach `error.rs` und wird dort privat; der Test `import_archive_rejects_an_oversized_archive_before_unpacking` prüft weiterhin auf „1024", nicht mehr auf „1024 Byte" (der Wortlaut hängt jetzt an der Sprache).

- [ ] **Step 6: `thiserror` entfernen**

In `crates/core/Cargo.toml` die Zeile `thiserror.workspace = true` löschen. In der Workspace-`Cargo.toml` bleibt der Eintrag, solange `crates/app` ihn noch benutzt; benutzt ihn niemand mehr, fliegt er auch dort raus (`cargo tree -p sm2-modloader | grep thiserror` gibt Auskunft).

- [ ] **Step 7: Tests, Clippy, Commit**

```bash
cargo test
cargo clippy --all-targets
git add crates/core crates/core/i18n
git commit -m "feat(core): Fehlertexte kommen aus dem Katalog

Error verliert thiserror und bekommt handgeschriebenes Display und
source(): eine Stelle, an der eine Variante auf einen Schlüssel
abgebildet wird, und alle Aufrufer, die {e} ausgeben, sprechen ohne
Änderung die eingestellte Sprache. CorruptBackup und UnusableArchive
tragen ihren Grund jetzt als Enum statt als fertigen deutschen Satz,
damit auch das Detail übersetzbar ist.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 4: Ausgaben der Kommandozeile

**Files:**
- Modify: `crates/app/src/cli.rs` (alle `println!`, `eprintln!`, `bail!`, `anyhow!`)
- Modify: `crates/app/src/main.rs` (die beiden Fehlerausgaben)
- Modify: `crates/app/src/vanilla.rs` (Meldungstext)
- Modify: `crates/core/i18n/en.toml`, `crates/core/i18n/de.toml`

**Interfaces:**
- Consumes: `t!` aus Task 1.
- Produces: keine neuen Signaturen; ab hier ist `cli.*` der Namensraum für Kommandozeilentexte.

- [ ] **Step 1: Schlüssel anlegen**

Für jede Ausgabe ein Schlüssel unter `cli.<befehl>.<zweck>`, Beispiele:

```toml
[cli.save]
backup_done = "✓ Backup: {path}"
none_present = "No backups present."
selected = "Selected backup: {created_at}  {label}"
restored = "✓ Restored: {created_at}"
restored_safety = "  Previous state saved: {path}"
import_done = "✓ Imported: {created_at}  {label}"
import_location = "  Located at: {path}"
steam_running = "Steam is running. Cloud synchronisation can overwrite the restored state. Please quit Steam and try again, or continue at your own risk with --force."
steam_forced = "Warning: Steam is running – --force enforces the restore despite possible cloud synchronisation."

[cli.error]
gui_start = "Error: the interface could not be started – {detail}"
prefix = "Error: {detail}"
```

`de.toml` bekommt die heutigen deutschen Sätze wortgleich.

- [ ] **Step 2: Ausgaben umstellen**

Jede Ausgabe wird zu `t!`. Drei Beispiele, die die drei Formen zeigen:

```rust
// vorher: println!("✓ Backup: {}", entry.archive.display());
println!("{}", t!("cli.save.backup_done", path = entry.archive.display()));

// vorher: bail!("keine Backups vorhanden");
bail!(t!("cli.save.none_present"));

// vorher: eprintln!("Fehler: {e:#}");
eprintln!("{}", t!("cli.error.prefix", detail = format!("{e:#}")));
```

`use sm2_core::t;` gehört an den Kopf jeder Datei, die das Makro benutzt.

- [ ] **Step 3: Bestehende Tests nachziehen**

`save_restore_refuses_when_steam_is_running_without_force` prüft heute `err.to_string().contains("Steam")`. Das Wort steht in beiden Sprachen im Text, der Test bleibt also gültig. Jeder Test, der auf einen anderen deutschen Wortlaut prüft, wird auf die englische Fassung umgestellt und setzt vorher explizit `i18n::set_language(Language::English)`.

- [ ] **Step 4: Tests laufen lassen**

Run: `cargo test -p sm2-modloader --bins`
Expected: PASS.

- [ ] **Step 5: `demo`-Schlüssel entfernen**

Die beiden `demo`-Schlüssel aus Task 1 werden aus beiden Sprachdateien gelöscht; die Tests in `i18n.rs`, die sie benutzen, ziehen auf `error.steam_not_found` (ohne Platzhalter) und `error.game_not_found` (mit `{app_id}`) um.

- [ ] **Step 6: Tests, Clippy, Commit**

```bash
cargo test
cargo clippy --all-targets
git add crates/app/src/cli.rs crates/app/src/main.rs crates/app/src/vanilla.rs crates/core/i18n crates/core/src/i18n.rs
git commit -m "feat(app): Kommandozeilenausgaben aus dem Katalog

Jede println!-, bail!- und eprintln!-Ausgabe läuft über t!. Die
demo-Schlüssel aus dem Katalog-Grundstein sind damit überflüssig und
weichen echten Schlüsseln in den Tests.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 5: `--help`, `--lang` und der Unterbefehl `lang`

**Files:**
- Modify: `crates/app/src/cli.rs` (Lokalisierung des Befehlsbaums, neue Option, neuer Unterbefehl)
- Create: `crates/app/src/cli_help.rs` — der Durchlauf über den Befehlsbaum plus sein Test
- Modify: `crates/core/i18n/en.toml`, `crates/core/i18n/de.toml`

**Interfaces:**
- Consumes: `i18n::lookup`, `i18n::has_key`, `Language` aus Task 1; `Settings::language` aus Task 2.
- Produces: `cli_help::localize(clap::Command, prefix: &str) -> clap::Command`; `cli_help::language_from_args(args: &[String]) -> Option<Language>`.

- [ ] **Step 1: Die Tests schreiben**

`crates/app/src/cli_help.rs`, Testmodul:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// The guard against drift: an argument added later without a key in
    /// the catalogue would silently keep clap's own English text.
    #[test]
    fn every_command_and_argument_has_a_key() {
        let mut missing = Vec::new();
        collect_missing_keys(&crate::cli::Cli::command(), "cli", &mut missing);

        assert!(missing.is_empty(), "keys missing from en.toml:\n{}", missing.join("\n"));
    }

    #[test]
    fn localizing_replaces_the_about_text() {
        sm2_core::i18n::set_language(sm2_core::i18n::Language::English);
        let command = localize(crate::cli::Cli::command(), "cli");

        let about = command.get_about().expect("the root command has an about text").to_string();

        assert_eq!(about, sm2_core::i18n::lookup("cli.about"));
    }

    #[test]
    fn a_language_option_in_front_of_everything_is_recognised() {
        let args = ["sm2-modloader", "--lang", "de", "save", "list"].map(String::from);
        assert_eq!(language_from_args(&args), Some(sm2_core::i18n::Language::German));

        let joined = ["sm2-modloader", "--lang=de"].map(String::from);
        assert_eq!(language_from_args(&joined), Some(sm2_core::i18n::Language::German));

        let without = ["sm2-modloader", "save", "list"].map(String::from);
        assert_eq!(language_from_args(&without), None);
    }
}
```

- [ ] **Step 2: Tests laufen lassen und Fehlschlag prüfen**

Run: `cargo test -p sm2-modloader --bins cli_help`
Expected: FAIL, „cannot find function `localize`".

- [ ] **Step 3: Den Durchlauf schreiben**

`crates/app/src/cli_help.rs`:

```rust
//! Puts the catalogue's texts into the `clap` command tree.
//!
//! `clap`'s derive takes its help from doc comments, which are `&'static
//! str` and therefore cannot change with the language. So the tree is
//! walked once before parsing and every `about` and `help` is replaced
//! with catalogue text. The doc comments stay in the source as English
//! developer notes and as the fallback should a key ever be missing.

use sm2_core::i18n::{self, Language};

/// The keys clap's own built-in arguments use, at every level of the
/// tree: `help` and `version` are added by clap to each subcommand, and
/// giving them a key per command would mean one pointless entry per
/// subcommand.
const BUILT_IN: [(&str, &str); 2] =
    [("help", "cli.built_in.help"), ("version", "cli.built_in.version")];

pub fn localize(command: clap::Command, prefix: &str) -> clap::Command {
    let mut command = command.about(i18n::lookup(&format!("{prefix}.about")));

    let ids: Vec<clap::Id> = command.get_arguments().map(|arg| arg.get_id().clone()).collect();
    for id in ids {
        let key = match BUILT_IN.iter().find(|(name, _)| *name == id.as_str()) {
            Some((_, key)) => (*key).to_string(),
            None => format!("{prefix}.arg.{id}"),
        };
        command = command.mut_arg(id, |arg| arg.help(i18n::lookup(&key)));
    }

    let names: Vec<String> =
        command.get_subcommands().map(|sub| sub.get_name().to_string()).collect();
    for name in names {
        let child = format!("{prefix}.{name}");
        command = command.mut_subcommand(&name, |sub| localize(sub, &child));
    }
    command
}

/// Collects every key the tree needs and that `en.toml` does not have.
/// Shared by `localize`'s test and nothing else — hence `cfg(test)`.
#[cfg(test)]
fn collect_missing_keys(command: &clap::Command, prefix: &str, missing: &mut Vec<String>) {
    let about = format!("{prefix}.about");
    if !i18n::has_key(&about) {
        missing.push(about);
    }
    for arg in command.get_arguments() {
        let id = arg.get_id().as_str();
        if BUILT_IN.iter().any(|(name, _)| *name == id) {
            continue;
        }
        let key = format!("{prefix}.arg.{id}");
        if !i18n::has_key(&key) {
            missing.push(key);
        }
    }
    for sub in command.get_subcommands() {
        collect_missing_keys(sub, &format!("{prefix}.{}", sub.get_name()), missing);
    }
}

/// Reads `--lang` out of the raw arguments, before `clap` parses.
///
/// It has to happen this early because `--help` is answered *during*
/// parsing: by then the tree has to carry the right language already.
pub fn language_from_args(args: &[String]) -> Option<Language> {
    let mut iter = args.iter();
    while let Some(argument) = iter.next() {
        if let Some(code) = argument.strip_prefix("--lang=") {
            return Language::from_code(code);
        }
        if argument == "--lang" {
            return iter.next().and_then(|code| Language::from_code(code));
        }
    }
    None
}
```

- [ ] **Step 4: Schlüssel eintragen**

Für jeden Befehl und jedes Argument ein Eintrag; den vollständigen Satz nennt der Test aus Step 1, wenn er fehlschlägt — seine Ausgabe ist die Arbeitsliste. Beispiele:

```toml
[cli]
about = "Mod loader for Space Marine 2"

[cli.built_in]
help = "Print help"
version = "Print version"

[cli.save]
about = "Back up and restore savegames"

[cli.save.import]
about = "Import a backup from another launcher: a ZIP holding the savegame files"

[cli.save.import.arg]
archive = "Path to the ZIP file"
tag = "Label for the imported backup; without one the archive's file name is used"
```

Die `///`-Doc-Kommentare in `cli.rs` werden bei dieser Gelegenheit ins Englische übersetzt: sie sind ab jetzt Entwicklerkommentare, und `CLAUDE.md` verlangt dafür Englisch.

- [ ] **Step 5: In `cli::run` einhängen**

```rust
pub fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // The language has to be right *before* parsing, because `--help` is
    // answered during it. `--lang` wins over the setting. The settings
    // file is read here and again in `AppState::open()` below: reading a
    // small TOML file twice is cheaper than parsing the arguments twice,
    // which is the only other way to learn about `--lang` this early.
    let stored = crate::app_state::load_dirs_and_settings()
        .map(|(_, settings)| settings.language())
        .unwrap_or_default();
    i18n::set_language(cli_help::language_from_args(&args).unwrap_or(stored));

    let cli = Cli::from_arg_matches(&cli_help::localize(Cli::command(), "cli").get_matches_from(args))?;

    let mut state = AppState::open()?;
    print_notices(&mut state);
    let result = run_command(&mut state, cli.command);
    print_notices(&mut state);
    result
}
```

`Cli::parse()` weicht damit `Cli::from_arg_matches`; dafür braucht `cli.rs`
`use clap::{CommandFactory, FromArgMatches};`.

`--lang` als globale Option in `Cli`:

```rust
    /// Language for this run (en, de); overrides the setting
    #[arg(long, global = true, value_name = "CODE")]
    lang: Option<String>,
```

- [ ] **Step 6: Den Unterbefehl `lang` ergänzen**

```rust
    /// Shows or sets the language
    Lang {
        /// Language code (en, de); without one the current setting is shown
        code: Option<String>,
    },
```

Behandlung:

```rust
        Command::Lang { code } => match code {
            None => {
                println!("{}", t!("cli.lang.current", name = i18n::language().native_name()));
                for language in Language::ALL {
                    println!("  {}  {}", language.code(), language.native_name());
                }
            }
            Some(code) => {
                let Some(language) = Language::from_code(&code) else {
                    bail!(t!("cli.lang.unknown", code = code));
                };
                let mut settings = state.settings.clone();
                settings.language = Some(language.code().to_string());
                settings.save(&state.dirs.settings_file())?;
                i18n::set_language(language);
                println!("{}", t!("cli.lang.set", name = language.native_name()));
            }
        },
```

Der genaue Pfad der Einstellungsdatei folgt der Stelle, an der `run_save_command` heute `state.backups_dir()` benutzt — dasselbe `AppState`.

- [ ] **Step 7: Tests, Clippy, Commit**

```bash
cargo test
cargo clippy --all-targets
git add crates/app/src/cli.rs crates/app/src/cli_help.rs crates/app/src/main.rs crates/core/i18n
git commit -m "feat(app): --help, --lang und der Unterbefehl lang

Der clap-Befehlsbaum wird vor dem Parsen einmal durchlaufen und mit
Katalogtexten bestückt; --lang wird vorher roh aus den Argumenten
gelesen, weil --help schon während des Parsens beantwortet wird. Ein
Test läuft denselben Baum ab und besteht auf einem Schlüssel für jeden
Befehl und jedes Argument.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 6: Zahlen, Datum und die gemeinsamen GUI-Bausteine

**Files:**
- Modify: `crates/app/src/gui/format.rs` (Trennzeichen und Datumsmuster aus dem Katalog)
- Modify: `crates/app/src/gui/status_bar.rs`, `crates/app/src/gui/top_bar.rs`, `crates/app/src/gui/side_bar.rs`, `crates/app/src/gui/widgets.rs`, `crates/app/src/gui/toasts.rs`
- Modify: `crates/core/i18n/en.toml`, `crates/core/i18n/de.toml`

**Interfaces:**
- Consumes: `t!`, `i18n::lookup` aus Task 1.
- Produces: `format::human_size` und `format::human_time` verhalten sich sprachabhängig; Signaturen bleiben unverändert.

- [ ] **Step 1: Schlüssel eintragen**

```toml
[format]
decimal_separator = "."
datetime = "{year}-{month}-{day} {clock}"
byte_unit = "bytes"
```

`de.toml`: `","`, `"{day}.{month}.{year} {clock}"`, `"Byte"`.

- [ ] **Step 2: Die Tests schreiben**

In `crates/app/src/gui/format.rs`, Testmodul — die bestehenden Tests werden zu Paaren:

```rust
    use sm2_core::i18n::{set_language, Language};

    #[test]
    fn a_size_uses_the_separator_of_the_language() {
        set_language(Language::German);
        assert_eq!(human_size(3_800_000_000), "3,8 GB");
        set_language(Language::English);
        assert_eq!(human_size(3_800_000_000), "3.8 GB");
    }

    #[test]
    fn a_timestamp_follows_the_pattern_of_the_language() {
        set_language(Language::German);
        assert_eq!(human_time("2026-09-14T21:38:00Z"), "14.09.2026 21:38");
        set_language(Language::English);
        assert_eq!(human_time("2026-09-14T21:38:00Z"), "2026-09-14 21:38");
    }
```

- [ ] **Step 3: Tests laufen lassen und Fehlschlag prüfen**

Run: `cargo test -p sm2-modloader --bins format`
Expected: FAIL — die englische Erwartung stimmt nicht, die Funktionen sind noch fest deutsch.

- [ ] **Step 4: `format.rs` umstellen**

```rust
pub fn human_size(bytes: u64) -> String {
    const UNITS: [(&str, f64); 3] = [("GB", 1e9), ("MB", 1e6), ("kB", 1e3)];
    let separator = i18n::lookup("format.decimal_separator");
    // `lookup` hands back an owned String, hence the borrow below.
    let value = bytes as f64;
    for (unit, factor) in UNITS {
        if value >= factor {
            return format!("{:.1} {unit}", value / factor).replace('.', &separator);
        }
    }
    t!("format.byte_unit_value", value = bytes)
}
```

Dafür kommt ein Schlüssel `format.byte_unit_value` dazu (`"{value} bytes"` / `"{value} Byte"`), der `format.byte_unit` ersetzt — eine Zahl und ihre Einheit gehören in einer Sprache zusammen, ihre Reihenfolge ist nicht überall gleich.

```rust
pub fn human_time(rfc3339: &str) -> String {
    // … Zerlegung unverändert …
    i18n::format(
        "format.datetime",
        &[
            ("year", year.to_string()),
            ("month", month.to_string()),
            ("day", day.to_string()),
            ("clock", clock.to_string()),
        ],
    )
}
```

- [ ] **Step 5: Die gemeinsamen Bausteine umstellen**

Alle Texte in `status_bar.rs`, `top_bar.rs`, `side_bar.rs`, `widgets.rs` und `toasts.rs` über `t!`, Schlüssel unter `gui.status_bar.*`, `gui.top_bar.*`, `gui.side_bar.*`, `gui.widgets.*`. Beispiel:

```rust
// vorher: format!("pak_config.yaml · {} Einträge", state.config.entries.len())
t!("gui.status_bar.entries", count = state.config.entries.len())
```

mit `entries = "pak_config.yaml · {count} entries"` bzw. `"pak_config.yaml · {count} Einträge"`.

- [ ] **Step 6: Tests, Clippy, Commit**

```bash
cargo test
cargo clippy --all-targets
git add crates/app/src/gui crates/core/i18n
git commit -m "feat(app): Zahlen, Datum und gemeinsame GUI-Bausteine übersetzt

Dezimaltrennzeichen und Datumsmuster stehen im Katalog statt im Code –
eine neue Sprache bringt ihre Schreibweise damit selbst mit. Statusbar,
Kopf- und Seitenleiste, Widgets und Toasts holen ihre Texte über t!.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 7: Die Seiten und Dialoge

**Files:**
- Modify: `crates/app/src/gui/mods_page.rs`, `profiles_page.rs`, `saves_page.rs`, `settings_page.rs`, `dialogs.rs`
- Modify: `crates/core/i18n/en.toml`, `crates/core/i18n/de.toml`

**Interfaces:**
- Consumes: `t!` aus Task 1.
- Produces: nichts Neues; Namensraum `gui.mods.*`, `gui.profiles.*`, `gui.saves.*`, `gui.settings.*`, `gui.dialog.*`.

- [ ] **Step 1: Datei für Datei umstellen**

Jede Zeichenkette, die auf dem Bildschirm landet, wird zu `t!`. Spaltenköpfe, Knopfbeschriftungen, Platzhaltertexte, Leerzustände, Dialogtitel und -texte. Beispiel aus `saves_page.rs`:

```rust
// vorher: "Backup importieren"
t!("gui.saves.import_button")
// vorher: format!("{} Backups · neueste zuerst", app.backups.len())
t!("gui.saves.count", count = app.backups.len())
```

Reihenfolge, in der die Arbeit übersichtlich bleibt: erst `saves_page.rs` (die wenigsten Texte), dann `profiles_page.rs`, `settings_page.rs`, `dialogs.rs`, zuletzt `mods_page.rs` (die meisten).

- [ ] **Step 2: Nach jeder Datei prüfen**

Run: `cargo test -p sm2-core --test i18n_keys`
Expected: PASS — der Test nennt jeden Schlüssel, der im Code steht und in `en.toml` fehlt. Er ist beim Migrieren die Arbeitsliste, nicht erst die Abnahme.

- [ ] **Step 3: Die Oberfläche ansehen**

Run: `cargo run`
Die vier Abschnitte durchklicken und auf abgeschnittene Beschriftungen achten: englische Texte sind oft länger als die deutschen, für die die Spaltenbreiten in `theme::metric` gewählt wurden. Wo ein Text nicht passt, wird der Text gekürzt — nicht die Spalte verbreitert, sonst weicht das Layout vom Entwurf in `docs/design/` ab.

- [ ] **Step 4: Tests, Clippy, Commit**

```bash
cargo test
cargo clippy --all-targets
git add crates/app/src/gui crates/core/i18n
git commit -m "feat(app): Seiten und Dialoge übersetzt

Alle Beschriftungen, Spaltenköpfe, Leerzustände und Dialogtexte der vier
Abschnitte laufen über den Katalog.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 8: Meldungen aus Zustand und Hintergrundarbeit

**Files:**
- Modify: `crates/app/src/gui/commands.rs`, `crates/app/src/gui/tasks.rs`, `crates/app/src/gui/mod.rs`, `crates/app/src/app_state.rs`
- Modify: `crates/core/i18n/en.toml`, `crates/core/i18n/de.toml`

**Interfaces:**
- Consumes: `t!` aus Task 1.
- Produces: nichts Neues; Namensraum `gui.message.*` für Status- und Toast-Texte, `app.notice.*` für die Hinweise aus dem Abgleich.

- [ ] **Step 1: Die Meldungen umstellen**

Alle Argumente von `set_status`, `set_warning`, `set_busy` und die Hinweistexte in `app_state.rs`. Beispiel:

```rust
// vorher: self.set_status(format!("Backup angelegt: {}", entry.created_at));
self.set_status(t!("gui.message.backup_done", created_at = entry.created_at));
```

Auch die `anyhow`-Zusätze zählen dazu: `.context("…")` und
`.with_context(|| format!("…"))` landen in der Fehlerausgabe und werden
ebenso zu `t!`. `grep -rn 'context(' crates/app/src` nennt sie alle.

`blocked_reason` und die `Notice`-Texte in `app_state.rs` folgen demselben Muster; ihr Namensraum ist `app.notice.*`, weil sie auch die Kommandozeile erreichen.

- [ ] **Step 2: Nach jeder Datei prüfen**

Run: `cargo test -p sm2-core --test i18n_keys`
Expected: PASS.

- [ ] **Step 3: Tests, Clippy, Commit**

```bash
cargo test
cargo clippy --all-targets
git add crates/app/src crates/core/i18n
git commit -m "feat(app): Status-, Toast- und Hinweistexte übersetzt

Alle Meldungen aus commands.rs, tasks.rs und app_state.rs kommen aus dem
Katalog; die Hinweise des Abgleichs liegen unter app.notice.*, weil sie
auch auf der Kommandozeile ausgegeben werden.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 9: Die Sprachauswahl in der Oberfläche

**Files:**
- Modify: `crates/app/src/gui/mod.rs` (`Dialog::Language`, drei `Action`-Varianten, Anwenden und Speichern)
- Modify: `crates/app/src/gui/dialogs.rs` (der Auswahldialog)
- Modify: `crates/app/src/gui/settings_page.rs` (die Zeile, die ihn öffnet)
- Modify: `crates/core/i18n/en.toml`, `crates/core/i18n/de.toml`

**Interfaces:**
- Consumes: `Settings::language`, `Language::ALL`, `Language::native_name`, `i18n::set_language`.
- Produces: `Dialog::Language { picked: Option<Language> }`; `Action::OpenLanguageDialog`, `Action::PickLanguage(Language)`, `Action::ConfirmLanguage`.

- [ ] **Step 1: Den Test schreiben**

In `crates/app/src/gui/mod.rs`, Testmodul — geprüft wird der Teil, der ohne Fenster läuft: dass Auswahl plus Bestätigung die Einstellung schreibt und die Sprache umstellt.

```rust
    /// The picker's job in one line: remember the choice and switch the
    /// program over. Everything else about it is painting.
    #[test]
    fn confirming_a_language_stores_the_code_and_switches_over() {
        let mut settings = Settings::default();

        apply_language(&mut settings, Language::German);

        assert_eq!(settings.language.as_deref(), Some("de"));
        assert_eq!(sm2_core::i18n::language(), Language::German);
        sm2_core::i18n::set_language(Language::English);
    }
```

- [ ] **Step 2: Test laufen lassen und Fehlschlag prüfen**

Run: `cargo test -p sm2-modloader --bins confirming_a_language`
Expected: FAIL, „cannot find function `apply_language`".

- [ ] **Step 3: Die Funktion schreiben**

In `crates/app/src/gui/mod.rs`:

```rust
/// Writes the chosen language into the settings and switches the running
/// program over. Separate from the dialog so that it can be tested
/// without a window.
fn apply_language(settings: &mut Settings, language: Language) {
    settings.language = Some(language.code().to_string());
    sm2_core::i18n::set_language(language);
}
```

- [ ] **Step 4: Test laufen lassen**

Run: `cargo test -p sm2-modloader --bins confirming_a_language`
Expected: PASS.

- [ ] **Step 5: Dialog und Aktionen ergänzen**

`Dialog::Language { picked: Option<Language> }` neben die bestehenden Varianten; im Dialog eine Radio-Zeile je `Language::ALL` mit `native_name()` — dasselbe Muster wie `Dialog::SteamUser`, inklusive Breite in `dialogs::show`. `Action::OpenLanguageDialog` öffnet ihn, `Action::PickLanguage(language)` setzt `picked`, `Action::ConfirmLanguage` ruft `apply_language`, speichert die Einstellungen über den bestehenden `save_settings`-Weg und schließt den Dialog.

Auf der Einstellungsseite im Abschnitt „Verhalten" eine Zeile mit Beschriftung `gui.settings.language`, rechts ein Knopf mit dem aktuellen `native_name()`, der `Action::OpenLanguageDialog` auslöst.

- [ ] **Step 6: Von Hand prüfen**

Run: `cargo run`
Sprache auf Deutsch stellen, Fenster schließen, `cargo run` erneut: die Oberfläche startet deutsch. In `~/.local/share/sm2-modloader/settings.toml` steht `language = "de"`.

- [ ] **Step 7: Tests, Clippy, Commit**

```bash
cargo test
cargo clippy --all-targets
git add crates/app/src/gui crates/core/i18n
git commit -m "feat(app): Sprachauswahl in den Einstellungen

Ein Auswahldialog nach dem Muster der Steam-Profilwahl, jede Sprache mit
ihrem eigenen Namen. Die Auswahl greift sofort und wird gespeichert.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 10: Abschluss — Sweep, Hausregel, Doku

**Files:**
- Create: `crates/app/tests/no_german_literals.rs`
- Modify: `CLAUDE.md`
- Modify: `docs/GUI-UMSETZUNG.md`

**Interfaces:**
- Consumes: nichts.
- Produces: den Sweep-Test als dauerhafte Abnahme.

- [ ] **Step 1: Den Sweep-Test schreiben**

`crates/app/tests/no_german_literals.rs`:

```rust
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
            if MARKERS.iter().any(|marker| code.contains(marker)) {
                hits.push(format!("{}:{}: {}", file.display(), number + 1, code.trim()));
            }
        }
    }

    assert!(hits.is_empty(), "German text still in the sources:\n{}", hits.join("\n"));
}
```

- [ ] **Step 2: Test laufen lassen**

Run: `cargo test -p sm2-modloader --test no_german_literals`
Expected: PASS. Schlägt er fehl, nennt er Datei und Zeile — jeder Treffer wandert in den Katalog, bis der Test grün ist.

- [ ] **Step 3: Die Hausregel umschreiben**

In `CLAUDE.md` den Abschnitt „Language" ersetzen:

```markdown
## Language

**Code and comments are English. User-facing text lives in the message
catalogue, never in the code.**

- English: `//`, `///`, `//!`, test names, `assert!` messages — anything
  only a developer reads.
- The catalogue: every sentence a user sees. `crates/core/i18n/en.toml`
  is the source of truth, `de.toml` the translation; both carry exactly
  the same keys. Reach for a text with `t!("area.key")`, or
  `t!("area.key", name = value)` when it has placeholders.

Adding a language: copy `en.toml`, translate it, add a `Language`
variant and name it in `Language::ALL`. `cargo test` then says whether
the translation is complete.

Three tests keep this honest and are worth knowing about before you add
a string: every language has exactly the English key set, every key used
in the sources exists, and no German sentence is left in the code
(`crates/app/tests/no_german_literals.rs`).
```

Ebenso die Testzahl in „Working on it" auf den dann aktuellen Stand ziehen.

- [ ] **Step 4: `docs/GUI-UMSETZUNG.md` nachziehen**

Der Abschnitt über Beschriftungen bekommt einen Absatz: Texte stehen im Katalog, die Oberfläche zeigt sie in der eingestellten Sprache, Spaltenbreiten sind auf die längere der beiden Fassungen ausgelegt.

- [ ] **Step 5: Tests, Clippy, Commit**

```bash
cargo test
cargo clippy --all-targets
git add crates/app/tests/no_german_literals.rs CLAUDE.md docs/GUI-UMSETZUNG.md
git commit -m "docs,test: Hausregel auf den Katalog umgestellt

Der Sweep-Test findet deutsche Reste im Code und nennt Datei und Zeile.
CLAUDE.md beschreibt ab jetzt den Katalog statt der alten Regel
'Nutzertext ist Deutsch', inklusive der drei Tests, die eine neue
Sprache abnehmen.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

## Reihenfolge und Abhängigkeiten

```
Task 1 (Katalog)
  ├── Task 2 (Einstellung)
  ├── Task 3 (Fehler) ──── Task 4 (CLI-Ausgaben) ──── Task 5 (--help, lang)
  └── Task 6 (Format, Bausteine) ── Task 7 (Seiten) ── Task 8 (Meldungen) ── Task 9 (Auswahl)
                                                                                   └── Task 10 (Abschluss)
```

Task 2 und Task 3 können parallel laufen, ebenso die Stränge CLI (4–5) und GUI (6–9), sobald Task 1 steht. Task 10 kommt zuletzt, weil sein Sweep-Test erst grün werden kann, wenn alle Texte umgezogen sind.

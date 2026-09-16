//! The message catalogue: one embedded TOML file per language, looked up
//! through the `t!` macro.

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

/// The active-language-then-English resolution chain, pulled out of
/// `lookup` as a pure function of two maps.
///
/// The committed catalogues can never exercise the English-fallback arm:
/// `every_language_has_exactly_the_english_keys` guarantees every shipped
/// language already has every English key. The fallback still earns its
/// place — it is what makes "a further language costs one file and one
/// line of code" true, since a newly added, half-translated language file
/// is exactly the state this arm serves. A synthetic-map unit test is the
/// only way to reach it before such a language exists.
fn resolve<'a>(active: &'a BTreeMap<String, String>, english: &'a BTreeMap<String, String>, key: &str) -> Option<&'a str> {
    active.get(key).or_else(|| english.get(key)).map(String::as_str)
}

/// The text for a key: from the active language, otherwise from English
/// (a half-finished translation should show the original, not a gap), and
/// otherwise the key itself — see `an_unknown_key_yields_the_key_itself`.
///
/// Returns `String` rather than `&'static str` precisely because of that
/// last case: the key belongs to the caller and does not live long enough
/// to be handed back by reference.
pub fn lookup(key: &str) -> String {
    resolve(language().catalog(), Language::English.catalog(), key)
        .map(str::to_string)
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
///
/// Only tests call this: it checks translations against the English
/// original, a build-time property with no runtime counterpart.
#[cfg(test)]
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

    /// The committed catalogues can never be missing a key that English
    /// has (that is `every_language_has_exactly_the_english_keys`'s whole
    /// point), so the fallback arm of `resolve` — the one that makes a
    /// freshly added, half-translated language show English instead of a
    /// gap — can only be reached with synthetic maps, never with the
    /// shipped `de.toml`/`en.toml`.
    #[test]
    fn resolve_prefers_the_active_language_over_english() {
        let active = BTreeMap::from([("greeting".to_string(), "Moin".to_string())]);
        let english = BTreeMap::from([("greeting".to_string(), "Hi".to_string())]);
        assert_eq!(resolve(&active, &english, "greeting"), Some("Moin"));
    }

    #[test]
    fn resolve_falls_back_to_english_when_the_active_language_lacks_the_key() {
        let active = BTreeMap::new();
        let english = BTreeMap::from([("greeting".to_string(), "Hi".to_string())]);
        assert_eq!(resolve(&active, &english, "greeting"), Some("Hi"));
    }

    #[test]
    fn resolve_yields_nothing_when_neither_map_has_the_key() {
        let active: BTreeMap<String, String> = BTreeMap::new();
        let english: BTreeMap<String, String> = BTreeMap::new();
        assert_eq!(resolve(&active, &english, "greeting"), None);
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

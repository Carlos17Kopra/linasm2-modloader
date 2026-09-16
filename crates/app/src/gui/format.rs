//! Numbers and timestamps written the way the active language writes them.
//!
//! The decimal separator and the date pattern come from the catalogue
//! (`format.decimal_separator`, `format.datetime`) instead of being fixed
//! in the code, so that a new language brings its own notation with it
//! rather than requiring a code change here.

use sm2_core::{i18n, t};

/// File size as "3.8 GB" (English) or "3,8 GB" (German) — decimal prefixes
/// with the active language's decimal separator.
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

/// Timestamp as "2026-09-14 21:38" (English) or "14.09.2026 21:38"
/// (German), following the active language's `format.datetime` pattern.
///
/// The input is RFC 3339 in UTC, the way `sm2_core::import::now_rfc3339`
/// writes it. If the string cannot be read as such, it is passed through
/// unchanged: a value edited by hand or left over from an older version
/// should stay visible rather than disappear behind a "—".
pub fn human_time(rfc3339: &str) -> String {
    let Some((date, time)) = rfc3339.split_once('T') else { return rfc3339.to_string() };
    let parts: Vec<&str> = date.split('-').collect();
    let [year, month, day] = parts.as_slice() else { return rfc3339.to_string() };
    if year.len() != 4 || month.len() != 2 || day.len() != 2 {
        return rfc3339.to_string();
    }
    let Some(clock) = time.get(0..5) else { return rfc3339.to_string() };
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

/// Hash shortened as in the design: eight characters at the front, four at
/// the end.
pub fn short_hash(hash: &str) -> String {
    if hash.len() <= 13 {
        return hash.to_string();
    }
    format!("{}…{}", &hash[..8], &hash[hash.len() - 4..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::language_test_lock;
    use sm2_core::i18n::{set_language, Language};

    #[test]
    fn a_size_uses_the_separator_of_the_language() {
        let _held = language_test_lock();
        set_language(Language::German);
        assert_eq!(human_size(3_800_000_000), "3,8 GB");
        set_language(Language::English);
        assert_eq!(human_size(3_800_000_000), "3.8 GB");
    }

    #[test]
    fn a_timestamp_follows_the_pattern_of_the_language() {
        let _held = language_test_lock();
        set_language(Language::German);
        assert_eq!(human_time("2026-09-14T21:38:00Z"), "14.09.2026 21:38");
        set_language(Language::English);
        assert_eq!(human_time("2026-09-14T21:38:00Z"), "2026-09-14 21:38");
    }

    #[test]
    fn an_unreadable_value_stays_put_instead_of_vanishing() {
        assert_eq!(human_time("von Hand eingetragen"), "von Hand eingetragen");
        assert_eq!(human_time("2026-09T21:38:07Z"), "2026-09T21:38:07Z");
    }

    #[test]
    fn the_hash_is_shown_at_both_ends() {
        let hash = "b1f4c9a0deadbeefcafe00112233445566778899aabbccddeeff0011223344";
        assert_eq!(short_hash(hash), "b1f4c9a0…3344");
        assert_eq!(short_hash("abc"), "abc", "nothing to shorten, nothing to invent");
    }
}

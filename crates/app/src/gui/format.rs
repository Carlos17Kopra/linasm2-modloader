//! Numbers and timestamps written the way the design writes them.

/// File size as "3,8 GB" — decimal prefixes and a comma, as in the design.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [(&str, f64); 3] = [("GB", 1e9), ("MB", 1e6), ("kB", 1e3)];
    let value = bytes as f64;
    for (unit, factor) in UNITS {
        if value >= factor {
            return format!("{:.1} {unit}", value / factor).replace('.', ",");
        }
    }
    format!("{bytes} Byte")
}

/// Timestamp as "14.09.2026 21:38".
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
    format!("{day}.{month}.{year} {clock}")
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

    #[test]
    fn sizes_are_written_with_a_decimal_comma() {
        assert_eq!(human_size(3_800_000_000), "3,8 GB");
        assert_eq!(human_size(640_000_000), "640,0 MB");
        assert_eq!(human_size(4_200_000), "4,2 MB");
        assert_eq!(human_size(512), "512 Byte");
    }

    #[test]
    fn timestamps_are_formatted_in_german_notation() {
        assert_eq!(human_time("2026-09-14T21:38:07Z"), "14.09.2026 21:38");
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

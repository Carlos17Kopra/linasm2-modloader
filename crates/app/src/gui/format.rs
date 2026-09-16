//! Zahlen und Zeitstempel in der Schreibweise des Entwurfs.

/// Dateigröße als „3,8 GB“ – Dezimalpräfixe und Komma, wie im Entwurf.
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

/// Zeitstempel als „14.09.2026 21:38“.
///
/// Eingabe ist RFC 3339 in UTC, wie `sm2_core::import::now_rfc3339` es
/// schreibt. Lässt sich die Zeichenkette nicht als solche lesen, wird sie
/// unverändert durchgereicht: eine von Hand bearbeitete oder aus einer
/// älteren Fassung stammende Angabe soll sichtbar bleiben, nicht als
/// „—“ verschwinden.
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

/// Hash gekürzt, wie im Entwurf: vorne acht, hinten vier Stellen.
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
    fn groessen_werden_mit_komma_geschrieben() {
        assert_eq!(human_size(3_800_000_000), "3,8 GB");
        assert_eq!(human_size(640_000_000), "640,0 MB");
        assert_eq!(human_size(4_200_000), "4,2 MB");
        assert_eq!(human_size(512), "512 Byte");
    }

    #[test]
    fn zeitstempel_werden_deutsch_geschrieben() {
        assert_eq!(human_time("2026-09-14T21:38:07Z"), "14.09.2026 21:38");
    }

    #[test]
    fn eine_unlesbare_angabe_bleibt_stehen_statt_zu_verschwinden() {
        assert_eq!(human_time("von Hand eingetragen"), "von Hand eingetragen");
        assert_eq!(human_time("2026-09T21:38:07Z"), "2026-09T21:38:07Z");
    }

    #[test]
    fn der_hash_wird_vorne_und_hinten_gezeigt() {
        let hash = "b1f4c9a0deadbeefcafe00112233445566778899aabbccddeeff0011223344";
        assert_eq!(short_hash(hash), "b1f4c9a0…3344");
        assert_eq!(short_hash("abc"), "abc", "nichts zu kürzen, nichts zu erfinden");
    }
}

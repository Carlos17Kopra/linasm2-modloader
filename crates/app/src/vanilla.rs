//! Die Sicherung vor einem Start ohne Mods – gemeinsam von Kommandozeile
//! und Oberfläche benutzt.
//!
//! Ein Vanilla-Start deaktiviert jeden Eintrag in `pak_config.yaml`. Ohne
//! eine Sicherung davor wäre die vorher mühsam zusammengestellte Auswahl
//! weg. Die Regel steht bewusst hier und nicht doppelt in beiden
//! Oberflächen: sie ist der einzige Weg zurück, und zwei Kopien davon wären
//! zwei Gelegenheiten, sie unterschiedlich falsch zu machen.

use crate::app_state::AppState;
use anyhow::{Context, Result};
use sm2_core::import::now_rfc3339;
use sm2_core::profile::Profile;
use std::path::PathBuf;

/// Namenspräfix, unter dem der bisherige Zustand gesichert wird. Jeder Lauf
/// hängt einen Zeitstempel an (siehe `snapshot_and_disable_all`), damit zwei
/// Vanilla-Starts hintereinander niemals denselben Profilnamen – und damit
/// dieselbe Datei, siehe `Profile::file_stem` – treffen und einander
/// überschreiben.
pub const VANILLA_SNAPSHOT_PREFIX: &str = "vor Vanilla-Start";

/// Was die Sicherung angelegt hat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub name: String,
    pub path: PathBuf,
}

/// Sichert den aktuellen Zustand als Profil und deaktiviert anschließend
/// alle Einträge.
///
/// Ist bereits kein Eintrag aktiv (z. B. weil dies der zweite Vanilla-Start
/// in Folge ist), wird kein neuer Schnappschuss angelegt und `None`
/// zurückgegeben: es gibt nichts zu schützen, und ein Schnappschuss „alles
/// deaktiviert“ wäre wertlos und würde die Profilliste nur zumüllen.
///
/// Enthält keine E/A jenseits von `Profile::save` – insbesondere kein
/// `persist()` und kein `launch()` –, ist also unabhängig vom eigentlichen
/// Spielstart testbar.
pub fn snapshot_and_disable_all(state: &mut AppState) -> Result<Option<Snapshot>> {
    let snapshot = if state.config.entries.iter().any(|e| !e.disabled) {
        let name = format!("{VANILLA_SNAPSHOT_PREFIX} {}", timestamp_for_snapshot_name());
        let profile = Profile::from_config(&name, &state.config);
        let path = profile.save(&state.profiles_dir()).context(
            "bisheriger Zustand konnte nicht gesichert werden – Start ohne Sicherung wird verweigert",
        )?;
        Some(Snapshot { name, path })
    } else {
        None
    };

    for entry in &mut state.config.entries {
        entry.disabled = true;
    }
    Ok(snapshot)
}

/// Menschenlesbarer Zeitstempel („2026-09-14 21:40“) für den Namen einer
/// Vanilla-Sicherung, auf die Minute genau.
///
/// Leitet sich aus `sm2_core::import::now_rfc3339` ab (Sekunden und das
/// `T`/`Z` von RFC-3339 entfernt), statt dessen Kalenderrechnung
/// („civil_from_days“) ein zweites Mal zu implementieren.
pub fn timestamp_for_snapshot_name() -> String {
    let rfc3339 = now_rfc3339();
    let (date, time) = rfc3339.split_once('T').unwrap_or((&rfc3339, ""));
    let minute_precision = time.get(0..5).unwrap_or(time);
    format!("{date} {minute_precision}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn der_zeitstempel_ist_auf_die_minute_genau() {
        let stamp = timestamp_for_snapshot_name();
        assert_eq!(stamp.len(), 16, "erwartet wird 'JJJJ-MM-TT hh:mm': {stamp}");
        assert_eq!(&stamp[4..5], "-", "Trennzeichen im Datum");
        assert_eq!(&stamp[10..11], " ", "Leerzeichen zwischen Datum und Uhrzeit");
        assert_eq!(&stamp[13..14], ":", "Trennzeichen in der Uhrzeit");
    }
}

//! The backup taken before a launch without mods — used by the command
//! line and the interface alike.
//!
//! A vanilla launch disables every entry in `pak_config.yaml`. Without a
//! backup beforehand, the selection painstakingly put together earlier
//! would be gone. The rule deliberately lives here rather than twice over
//! in both interfaces: it is the only way back, and two copies of it would
//! be two opportunities to get it wrong in different ways.

use crate::app_state::AppState;
use anyhow::{Context, Result};
use sm2_core::import::now_rfc3339;
use sm2_core::profile::Profile;
use sm2_core::t;
use std::path::PathBuf;

/// The name prefix under which the previous state is backed up. Every run
/// appends a timestamp (see `snapshot_and_disable_all`) so that two vanilla
/// launches in a row can never hit the same profile name — and therefore
/// the same file, see `Profile::file_stem` — and overwrite each other.
///
/// Stays German by decision, unlike every other user-facing string in this
/// program: it is matched by prefix and already written into existing
/// profile names on users' disks, so translating it would rename data that
/// is already there. It is shown to the user even in an English interface
/// — a known limitation, not an oversight — and whether to eventually
/// split the stored form from the displayed form is left open for later.
pub const VANILLA_SNAPSHOT_PREFIX: &str = "vor Vanilla-Start";

/// What the backup created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub name: String,
    pub path: PathBuf,
}

/// Backs up the current state as a profile and then disables all entries.
///
/// If no entry is active anyway (because this is the second vanilla launch
/// in a row, say), no new snapshot is created and `None` is returned: there
/// is nothing to protect, and an "everything disabled" snapshot would be
/// worthless and would only clutter up the profile list.
///
/// Contains no I/O beyond `Profile::save` — in particular no `persist()`
/// and no `launch()` — so it can be tested independently of actually
/// starting the game.
pub fn snapshot_and_disable_all(state: &mut AppState) -> Result<Option<Snapshot>> {
    let snapshot = if state.config.entries.iter().any(|e| !e.disabled) {
        let name = format!("{VANILLA_SNAPSHOT_PREFIX} {}", timestamp_for_snapshot_name());
        let profile = Profile::from_config(&name, &state.config);
        let path = profile.save(&state.profiles_dir()).context(t!("cli.play.vanilla_backup_failed"))?;
        Some(Snapshot { name, path })
    } else {
        None
    };

    for entry in &mut state.config.entries {
        entry.disabled = true;
    }
    Ok(snapshot)
}

/// Human-readable timestamp ("2026-09-14 21:40") for the name of a vanilla
/// backup, accurate to the minute.
///
/// Derived from `sm2_core::import::now_rfc3339` (seconds and RFC 3339's
/// `T`/`Z` removed) rather than implementing its calendar arithmetic
/// ("civil_from_days") a second time.
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
    fn the_timestamp_is_precise_to_the_minute() {
        let stamp = timestamp_for_snapshot_name();
        assert_eq!(stamp.len(), 16, "expected 'YYYY-MM-DD hh:mm': {stamp}");
        assert_eq!(&stamp[4..5], "-", "separator inside the date");
        assert_eq!(&stamp[10..11], " ", "space between date and time");
        assert_eq!(&stamp[13..14], ":", "separator inside the time");
    }
}

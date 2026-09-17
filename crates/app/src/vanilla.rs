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
use sm2_core::i18n::{lookup_in, Language};
use sm2_core::import::now_rfc3339;
use sm2_core::profile::Profile;
use sm2_core::t;
use std::path::PathBuf;

/// The name a snapshot of the previous state is saved under: the label in
/// the language that is active right now, followed by a timestamp so that
/// two vanilla launches in a row can never hit the same profile name — and
/// therefore the same file, see `Profile::file_stem` — and overwrite each
/// other.
///
/// The name is written once and never rewritten, so a profile keeps the
/// wording of the run that made it. Recognising one again is therefore
/// `is_snapshot_name`'s job and not a matter of the active language.
pub fn snapshot_name() -> String {
    format!("{} {}", t!("label.before_vanilla_launch"), timestamp_for_snapshot_name())
}

/// Whether `name` belongs to a profile this program wrote before a vanilla
/// start, rather than one the user named.
///
/// Asks every language instead of only the active one. The profiles on a
/// user's disk carry the wording of whichever interface language was set
/// when they were made; matching only the current language would strip the
/// "automatic" badge off every snapshot from before a language switch, and
/// renaming them to repair that is exactly what this program does not do
/// to data it finds.
pub fn is_snapshot_name(name: &str) -> bool {
    Language::ALL
        .iter()
        .any(|language| name.starts_with(&lookup_in(*language, "label.before_vanilla_launch")))
}

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
        let name = snapshot_name();
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

    /// The badge on the profiles page asks every language, not just the
    /// active one: a snapshot made under a German interface has to keep
    /// being recognised as automatic after a switch to English, and the
    /// other way round. Its name on disk is never rewritten.
    #[test]
    fn a_snapshot_name_is_recognised_whatever_language_wrote_it() {
        assert!(is_snapshot_name("vor Vanilla-Start 2026-09-14 21:40"));
        assert!(is_snapshot_name("before vanilla launch 2026-09-14 21:40"));
        assert!(!is_snapshot_name("Coop night"), "a name the user chose is not a snapshot");
    }

    #[test]
    fn a_snapshot_is_named_in_the_active_language() {
        let _held = crate::app_state::language_test_lock();

        sm2_core::i18n::set_language(sm2_core::i18n::Language::English);
        let english = snapshot_name();
        assert!(english.starts_with("before vanilla launch"), "{english}");

        sm2_core::i18n::set_language(sm2_core::i18n::Language::German);
        let german = snapshot_name();
        assert!(german.starts_with("vor Vanilla-Start"), "{german}");
    }

    #[test]
    fn the_timestamp_is_precise_to_the_minute() {
        let stamp = timestamp_for_snapshot_name();
        assert_eq!(stamp.len(), 16, "expected 'YYYY-MM-DD hh:mm': {stamp}");
        assert_eq!(&stamp[4..5], "-", "separator inside the date");
        assert_eq!(&stamp[10..11], " ", "space between date and time");
        assert_eq!(&stamp[13..14], ":", "separator inside the time");
    }
}

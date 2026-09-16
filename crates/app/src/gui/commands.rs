//! The short commands the interface can carry out right away: launching,
//! profiles, settings, the decision to restore.
//!
//! Anything that takes noticeable time lives in `tasks` instead.

use super::{App, Dialog, LaunchChoice, Notice, NoticeAction, Section};
use crate::app_state::AppState;
use crate::vanilla;
use sm2_core::launch::{self, LaunchMode};
use sm2_core::paths::GamePaths;
use sm2_core::profile::Profile;
use sm2_core::saves;
use std::path::PathBuf;

impl App {
    /// Evaluates the launch choice. A start without mods goes through the
    /// confirmation dialog first — it discards the current selection.
    pub(super) fn launch(&mut self) {
        if self.state.is_none() {
            self.set_warning("Start nicht möglich – Spielverzeichnis unbekannt.");
            return;
        }
        match self.launch_choice {
            LaunchChoice::Vanilla => self.dialog = Some(Dialog::Vanilla),
            LaunchChoice::NoEac => self.perform_launch(false, true),
            LaunchChoice::Steam => self.perform_launch(false, false),
        }
    }

    /// The launch itself, in the same order as `play` on the command line:
    /// snapshot, save backup, write the configuration, start.
    ///
    /// That order is not a matter of taste. The save backup runs before
    /// the configuration is written, so that a failure never leaves a
    /// written configuration behind for a game that was never started. And
    /// `persist()` is only fatal on a vanilla start: there, disabling
    /// everything is the whole point of the call, otherwise it changes at
    /// most the outcome of the reconciliation.
    pub(super) fn perform_launch(&mut self, vanilla_start: bool, no_eac: bool) {
        if no_eac && !self.no_eac_available {
            self.set_warning(
                "Start ohne EAC ist auf diesem System nicht möglich – umu-launcher wurde nicht \
                 gefunden.",
            );
            return;
        }

        if vanilla_start {
            let Some(state) = &mut self.state else { return };
            match vanilla::snapshot_and_disable_all(state) {
                Ok(Some(snapshot)) => {
                    let name = snapshot.name.clone();
                    self.notices.push(
                        Notice::info(format!(
                            "Bisheriger Zustand als Profil „{name}“ gesichert. Darüber kommst du \
                             zurück."
                        ))
                        .with_action(NoticeAction::ShowProfile(name)),
                    );
                    self.refresh_profiles();
                }
                Ok(None) => {
                    self.notices.push(Notice::info(
                        "Bereits vollständig deaktiviert – keine neue Sicherung angelegt.",
                    ));
                }
                Err(e) => {
                    self.set_warning(format!(
                        "Nichts verändert und nichts gestartet – {e:#}"
                    ));
                    return;
                }
            }
        }

        self.run_auto_backup(vanilla_start);

        if !self.persist() && vanilla_start {
            // The warning is already in the status bar. Starting with the
            // mods still active would be the opposite of what the user
            // asked for.
            return;
        }

        let Some(state) = &self.state else { return };
        let mode = if no_eac { LaunchMode::NoEac } else { LaunchMode::Steam };
        match launch::launch(&state.paths, mode) {
            Ok(()) => {
                let how = if no_eac {
                    "ohne EAC gestartet – Multiplayer ist damit nicht möglich."
                } else if vanilla_start {
                    "ohne Mods gestartet – alle Einträge deaktiviert."
                } else {
                    "über Steam gestartet."
                };
                self.set_status(format!("Spiel {how}"));
            }
            Err(e) => self.set_warning(format!("Start fehlgeschlagen – {e}")),
        }
    }

    /// Makes a savegame backup before the start if the setting calls for
    /// one. A failure only warns: the automatic backup is a convenience,
    /// not a hard requirement.
    fn run_auto_backup(&mut self, vanilla_start: bool) {
        if !self.settings().auto_backup {
            return;
        }
        let label = if vanilla_start { "vor Vanilla-Start" } else { "vor Modded-Start" };
        let Some(state) = &self.state else { return };
        let Some(backups) = self.backups_dir() else { return };

        match state.paths.save_dir(state.settings.steam_user.as_deref()) {
            Ok(save_dir) => match saves::backup(&save_dir, &backups, Some(label)) {
                Ok(entry) => {
                    self.notices
                        .push(Notice::info(format!("Savegame gesichert: {}", entry.created_at)));
                    self.refresh_backups();
                }
                Err(e) => self
                    .notices
                    .push(Notice::warning(format!("Save-Backup fehlgeschlagen – {e}"))),
            },
            Err(e) => {
                self.notices.push(Notice::warning(format!("Kein Save-Backup möglich – {e}")))
            }
        }
    }

    pub(super) fn save_profile(&mut self) {
        let name = self.profile_name.trim().to_owned();
        if name.is_empty() {
            self.set_warning("Bitte einen Namen für das Profil eingeben.");
            return;
        }
        let (Some(state), Some(dir)) = (&self.state, self.profiles_dir()) else {
            self.set_warning("Speichern nicht möglich – Spielverzeichnis unbekannt.");
            return;
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.set_warning(format!("{} konnte nicht angelegt werden – {e}", dir.display()));
            return;
        }
        match Profile::from_config(&name, &state.config).save(&dir) {
            Ok(_) => {
                self.profile_name.clear();
                self.refresh_profiles();
                self.set_status(format!("Profil „{name}“ gespeichert."));
            }
            Err(e) => self.set_warning(format!("Profil „{name}“ nicht gespeichert – {e}")),
        }
    }

    /// Applies a profile: activation and load order are replaced. Paks the
    /// profile knows about but that are missing are skipped and reported —
    /// which is what the text above the table promises.
    pub(super) fn apply_profile(&mut self, name: &str) {
        if !self.can_modify() {
            self.set_warning(self.blocked_reason("Anwenden"));
            return;
        }
        let Some(profile) = self.profiles.iter().find(|p| p.name == name).cloned() else { return };
        let Some(state) = &mut self.state else { return };

        let present = match state.paths.list_paks() {
            Ok(present) => present,
            Err(e) => {
                self.set_warning(format!("Mods-Verzeichnis nicht lesbar – {e}"));
                return;
            }
        };
        let (config, missing) = profile.apply(&present);
        state.config = config;

        for pak in &missing {
            self.notices.push(Notice::warning(format!(
                "{pak} aus dem Profil ist nicht installiert und wurde übersprungen."
            )));
        }
        if self.persist() {
            let active = self.entries().iter().filter(|e| !e.disabled).count();
            self.set_status(format!("Profil „{name}“ angewendet – {active} Mods aktiv."));
        }
    }

    pub(super) fn delete_profile(&mut self) {
        let Some(Dialog::DeleteProfile { name }) = self.dialog.clone() else { return };
        self.dialog = None;
        let Some(dir) = self.profiles_dir() else { return };
        let Some(profile) = self.profiles.iter().find(|p| p.name == name) else { return };

        let path = profile.path_in(&dir);
        match std::fs::remove_file(&path) {
            Ok(()) => {
                self.refresh_profiles();
                self.set_status(format!("Profil „{name}“ gelöscht."));
            }
            Err(e) => self.set_warning(format!("Profil „{name}“ nicht gelöscht – {e}")),
        }
    }

    /// Asks for the game directory and checks it before it lands in the
    /// settings — an unchecked path there would otherwise produce the same
    /// error message on every further start.
    pub(super) fn pick_game_dir(&mut self) {
        let Some(dir) = rfd::FileDialog::new()
            .set_title("Verzeichnis von Space Marine 2 wählen")
            .pick_folder()
        else {
            return;
        };

        let library = library_root_of(&dir);
        if let Err(e) = GamePaths::from_game_dir(&dir, &library) {
            self.set_warning(format!("{e}"));
            return;
        }

        self.fallback_settings.game_dir = Some(dir);
        if let Some(state) = &mut self.state {
            state.settings.game_dir = self.fallback_settings.game_dir.clone();
        }
        self.save_settings();
        self.load();
    }

    pub(super) fn confirm_steam_user(&mut self) {
        let Some(Dialog::SteamUser { picked }) = self.dialog.clone() else { return };
        self.dialog = None;
        let Some(user) = picked else {
            self.set_warning("Kein Nutzerprofil gewählt.");
            return;
        };

        self.fallback_settings.steam_user = Some(user.clone());
        if let Some(state) = &mut self.state {
            state.settings.steam_user = Some(user.clone());
        }
        self.save_settings();
        self.refresh_steam_users();
        self.refresh_backups();
        self.set_status(format!(
            "Steam-Nutzerprofil {user} gesetzt – Savegame-Funktionen freigegeben."
        ));
    }

    pub(super) fn toggle_auto_backup(&mut self) {
        let next = !self.settings().auto_backup;
        self.fallback_settings.auto_backup = next;
        if let Some(state) = &mut self.state {
            state.settings.auto_backup = next;
        }
        self.save_settings();
        let word = if next { "ein" } else { "aus" };
        self.set_status(format!("Automatisches Backup vor dem Start {word}."));
    }

    /// Decides whether a restore runs right away or has to pass through
    /// the warning dialog first.
    ///
    /// If Steam is running, its cloud sync can overwrite the restored save
    /// again — then and only then does the loader ask (the same line
    /// `--force` draws on the command line). Without Steam running, the
    /// promise above the table is enough: the loader always backs up the
    /// current save beforehand.
    pub(super) fn ask_restore(&mut self, index: usize) {
        if self.saves_blocked.is_some() {
            self.set_warning("Wiederherstellen nicht möglich – Steam-Nutzerprofil nicht gewählt.");
            return;
        }
        if index >= self.backups.len() {
            return;
        }
        if saves::steam_is_running() {
            self.dialog = Some(Dialog::Restore { index, force: false });
        } else {
            self.start_restore(index);
        }
    }

    /// Opens "Backup umbenennen" prefilled with the current label.
    pub(super) fn ask_rename_backup(&mut self, index: usize) {
        let Some(entry) = self.backups.get(index) else { return };
        self.dialog =
            Some(Dialog::RenameBackup { index, label: entry.label.clone().unwrap_or_default() });
    }

    /// Writes the new label. This only touches the loader's own backup
    /// directory, never the saves — which is why, unlike backing up and
    /// restoring, it is allowed even while the Steam user profile is still
    /// unresolved, and needs no background thread.
    pub(super) fn confirm_rename_backup(&mut self) {
        let Some(Dialog::RenameBackup { index, label }) = self.dialog.clone() else { return };
        self.dialog = None;
        let Some(entry) = self.backups.get(index).cloned() else { return };
        let Some(backups) = self.backups_dir() else { return };

        let label = label.trim().to_owned();
        let label = (!label.is_empty()).then_some(label);
        match saves::rename(&entry, &backups, label.as_deref()) {
            Ok(renamed) => {
                self.refresh_backups();
                match renamed.label {
                    Some(label) => self
                        .set_status(format!("Backup {} heißt jetzt „{label}“.", renamed.created_at)),
                    None => self
                        .set_status(format!("Etikett von Backup {} entfernt.", renamed.created_at)),
                }
            }
            Err(e) => self.set_warning(format!("Backup nicht umbenannt – {e}")),
        }
    }

    pub(super) fn delete_backup(&mut self) {
        let Some(Dialog::DeleteBackup { index }) = self.dialog.clone() else { return };
        self.dialog = None;
        let Some(entry) = self.backups.get(index).cloned() else { return };

        match saves::delete(&entry) {
            Ok(()) => {
                // The "verified" badge hangs off the timestamp. If the
                // entry stayed, a backup created later in the same second
                // would carry a check that never happened.
                self.verified.remove(&entry.created_at);
                self.refresh_backups();
                self.set_status(format!("Backup {} gelöscht.", entry.created_at));
            }
            Err(e) => self.set_warning(format!("Backup nicht gelöscht – {e}")),
        }
    }

    pub(super) fn show_backup_in_files(&mut self, index: usize) {
        let Some(entry) = self.backups.get(index) else { return };
        let Some(dir) = entry.archive.parent().map(PathBuf::from) else { return };
        self.open_folder(dir);
    }

    pub(super) fn confirm_restore(&mut self) {
        let Some(Dialog::Restore { index, force }) = self.dialog.clone() else { return };
        if !force {
            self.set_warning(
                "Bitte zuerst bestätigen, dass die Cloud-Synchronisation den Stand überschreiben \
                 kann.",
            );
            return;
        }
        self.dialog = None;
        self.start_restore(index);
    }
}

/// Derives the Steam library above a game directory — the same rule
/// `AppState::open_with` uses to resolve a `game_dir` set by hand.
fn library_root_of(game_dir: &std::path::Path) -> PathBuf {
    game_dir
        .ancestors()
        .find(|a| a.join("steamapps/common").is_dir())
        .map(PathBuf::from)
        .unwrap_or_else(|| game_dir.to_path_buf())
}

/// An `AppState` is only needed here to touch the fields; without this the
/// import would count as unused.
const _: Option<fn(&AppState)> = None;
const _: Option<fn(Section)> = None;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_is_derived_from_the_game_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let library = tmp.path().join("SteamLibrary");
        let game = library.join("steamapps/common/Space Marine 2");
        std::fs::create_dir_all(&game).unwrap();

        assert_eq!(library_root_of(&game), library, "the root above steamapps is expected");
    }

    #[test]
    fn without_a_steamapps_above_it_the_directory_itself_is_the_root() {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("Spiele/Space Marine 2");
        std::fs::create_dir_all(&game).unwrap();

        assert_eq!(
            library_root_of(&game),
            game,
            "a game unpacked by hand into some other place must not fail"
        );
    }
}

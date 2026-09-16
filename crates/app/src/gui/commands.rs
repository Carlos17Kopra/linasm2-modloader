//! Die kurzen, sofort ausführbaren Befehle der Oberfläche: Starten,
//! Profile, Einstellungen, Wiederherstellen-Entscheidung.
//!
//! Alles, was spürbar dauert, steht stattdessen in `tasks`.

use super::{App, Dialog, LaunchChoice, Notice, NoticeAction, Section};
use crate::app_state::AppState;
use crate::vanilla;
use sm2_core::launch::{self, LaunchMode};
use sm2_core::paths::GamePaths;
use sm2_core::profile::Profile;
use sm2_core::saves;
use std::path::PathBuf;

impl App {
    /// Wertet die Startauswahl aus. Ein Start ohne Mods geht zuerst durch
    /// den Bestätigungsdialog – er verwirft die aktuelle Auswahl.
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

    /// Der eigentliche Start, in derselben Reihenfolge wie `play` auf der
    /// Kommandozeile: Sicherung, Save-Backup, Speichern, Start.
    ///
    /// Die Reihenfolge ist keine Geschmacksfrage. Das Save-Backup läuft vor
    /// dem Speichern der Konfiguration, damit ein Fehlschlag nie eine
    /// bereits geschriebene Konfiguration bei einem nie gestarteten Spiel
    /// hinterlässt. Und `persist()` ist nur beim Vanilla-Start hart: dort
    /// ist das Deaktivieren der ganze Zweck des Aufrufs, sonst ändert es
    /// höchstens das Ergebnis des Abgleichs.
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
            // Die Warnung steht bereits in der Statusleiste. Ein Start mit
            // unverändert aktiven Mods wäre das Gegenteil dessen, was der
            // Nutzer wollte.
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

    /// Legt vor dem Start ein Savegame-Backup an, wenn die Einstellung das
    /// vorsieht. Ein Fehlschlag warnt nur: das automatische Backup ist eine
    /// Komfortfunktion, kein hartes Erfordernis.
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

    /// Wendet ein Profil an: Aktivierung und Reihenfolge werden ersetzt.
    /// Paks, die das Profil kennt, die aber fehlen, werden übersprungen und
    /// gemeldet – so steht es auch über der Tabelle.
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

    /// Fragt nach dem Spielverzeichnis und prüft es, bevor es in den
    /// Einstellungen landet – ein ungeprüfter Pfad dort führte sonst bei
    /// jedem weiteren Start zu derselben Fehlermeldung.
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

    /// Entscheidet, ob eine Wiederherstellung sofort läuft oder erst durch
    /// den Warndialog muss.
    ///
    /// Läuft Steam, kann dessen Cloud-Synchronisation den zurückgespielten
    /// Stand wieder überschreiben – dann und nur dann fragt der Loader nach
    /// (dieselbe Grenze wie `--force` auf der Kommandozeile). Ohne
    /// laufendes Steam genügt die Zusage über der Tabelle: der Loader
    /// sichert den aktuellen Stand vorher immer automatisch.
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

    /// Öffnet „Backup umbenennen“ mit dem aktuellen Etikett vorbelegt.
    pub(super) fn ask_rename_backup(&mut self, index: usize) {
        let Some(entry) = self.backups.get(index) else { return };
        self.dialog =
            Some(Dialog::RenameBackup { index, label: entry.label.clone().unwrap_or_default() });
    }

    /// Schreibt das neue Etikett. Das rührt nur an das Backup-Verzeichnis
    /// des Loaders, nicht an die Spielstände – deshalb ist das, anders als
    /// Sichern und Wiederherstellen, auch bei ungeklärtem Steam-Nutzerprofil
    /// erlaubt und braucht keinen Hintergrundthread.
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
                // Das „geprüft“-Abzeichen hängt am Zeitstempel. Bliebe der
                // Eintrag stehen, trüge ein später in derselben Sekunde
                // angelegtes Backup eine Prüfung, die nie stattgefunden hat.
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

/// Leitet aus einem Spielverzeichnis die Steam-Bibliothek darüber ab –
/// dieselbe Regel, nach der `AppState::open_with` einen von Hand gesetzten
/// `game_dir` auflöst.
fn library_root_of(game_dir: &std::path::Path) -> PathBuf {
    game_dir
        .ancestors()
        .find(|a| a.join("steamapps/common").is_dir())
        .map(PathBuf::from)
        .unwrap_or_else(|| game_dir.to_path_buf())
}

/// Ein `AppState` wird hier nur zum Anfassen der Felder gebraucht; der
/// Import hält die Datei sonst für ungenutzt.
const _: Option<fn(&AppState)> = None;
const _: Option<fn(Section)> = None;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn die_bibliothek_wird_aus_dem_spielverzeichnis_abgeleitet() {
        let tmp = tempfile::tempdir().unwrap();
        let library = tmp.path().join("SteamLibrary");
        let game = library.join("steamapps/common/Space Marine 2");
        std::fs::create_dir_all(&game).unwrap();

        assert_eq!(library_root_of(&game), library, "erwartet wird die Wurzel über steamapps");
    }

    #[test]
    fn ohne_steamapps_darueber_bleibt_das_verzeichnis_selbst_die_wurzel() {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("Spiele/Space Marine 2");
        std::fs::create_dir_all(&game).unwrap();

        assert_eq!(
            library_root_of(&game),
            game,
            "ein von Hand irgendwohin entpacktes Spiel darf nicht scheitern"
        );
    }
}

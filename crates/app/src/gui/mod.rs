//! Die grafische Oberfläche, eins zu eins nach dem Entwurf
//! `SM2 Mod Loader GUI v2 modern.dc.html`.
//!
//! Aufbau des Fensters, von außen nach innen:
//!
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │ Kopfleiste: Überschrift · Startauswahl        │  58 px
//! ├──────────┬───────────────────────────────────┤
//! │ Seiten-  │ Inhalt: Mods / Profile /          │
//! │ leiste   │ Savegames / Einstellungen         │
//! │ 208 px   │                                   │
//! ├──────────┴───────────────────────────────────┤
//! │ Statusleiste: Fortschritt · Meldung          │  30 px
//! └──────────────────────────────────────────────┘
//! ```
//!
//! Die eigene Fenstertitelleiste des Entwurfs ist bewusst nicht
//! nachgebaut – der Entwurf hält in seiner Schlussbemerkung selbst fest,
//! dass sie nur Dekoration der Skizze ist und das echte Fenster
//! Systemdekoration bekommt.
//!
//! Ereignisse werden nicht sofort ausgeführt, sondern als `Action`
//! gesammelt und nach dem Zeichnen angewendet. `egui` zeichnet unmittelbar:
//! ein Klick in einer Zeile fällt an, während gerade über die Liste
//! iteriert wird – eine sofortige Änderung genau dieser Liste wäre der
//! klassische Weg in einen Ausleihfehler oder eine halb gezeichnete Zeile.

mod commands;
mod dialogs;
mod format;
mod icons;
mod mods_page;
mod profiles_page;
mod saves_page;
mod settings_page;
mod side_bar;
mod status_bar;
mod tasks;
mod theme;
mod top_bar;
mod widgets;

use crate::app_state::{self, AppState};
use sm2_core::launch;
use sm2_core::library::ModInfo;
use sm2_core::pak_config::PakEntry;
use sm2_core::paths::AppDirs;
use sm2_core::platform::{Current, Platform};
use sm2_core::profile::{list_profiles, Profile};
use sm2_core::saves::{self, BackupEntry};
use sm2_core::settings::Settings;
use std::collections::HashSet;
use std::path::PathBuf;

/// Startet die Oberfläche. Kehrt zurück, wenn der Nutzer das Fenster
/// schließt.
pub fn run() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 700.0])
            .with_min_inner_size([860.0, 560.0])
            .with_title("SM2 Mod Loader")
            .with_app_id("sm2-modloader"),
        ..Default::default()
    };

    eframe::run_native(
        "SM2 Mod Loader",
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(App::new(cc.egui_ctx.clone())))
        }),
    )
}

/// Bereich der Seitenleiste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Mods,
    Profiles,
    Saves,
    Settings,
}

/// Auswahl der Startart in der Kopfleiste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchChoice {
    Steam,
    Vanilla,
    NoEac,
}

impl LaunchChoice {
    fn label(self) -> &'static str {
        match self {
            Self::Steam => "Mit Mods (über Steam)",
            Self::Vanilla => "Ohne Mods (Vanilla)",
            Self::NoEac => "Ohne EAC (kein Multiplayer)",
        }
    }
}

/// Offenes modales Fenster.
#[derive(Debug, Clone, PartialEq)]
pub enum Dialog {
    /// „Wiederherstellen, obwohl Steam läuft?“ – nur dieser Dialog kann
    /// Spielstände überschreiben, deshalb hängt er an einem eigenen
    /// Bestätigungsschalter.
    Restore { index: usize, force: bool },
    /// „Ohne Mods starten?“
    Vanilla,
    /// „Steam-Nutzerprofil wählen“
    SteamUser { picked: Option<String> },
    /// „Profil löschen?“
    DeleteProfile { name: String },
    /// „Backup umbenennen“ – das Etikett ist das, woran man ein Backup
    /// später wiedererkennt.
    RenameBackup { index: usize, label: String },
    /// „Backup löschen?“
    DeleteBackup { index: usize },
}

/// Was eine Hinweiszeile als Schaltfläche anbietet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoticeAction {
    /// „Verzeichnis öffnen“ – bei schreibgeschütztem Mods-Verzeichnis.
    OpenModsDir,
    /// „Profil anwenden“ – nach einem Vanilla-Start, mit dem Namen der
    /// Sicherung.
    ShowProfile(String),
}

/// Eine Zeile der Hinweisleiste über der Mod-Liste.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub kind: app_state::NoticeKind,
    pub text: String,
    pub action: Option<NoticeAction>,
}

impl Notice {
    fn from_state(notice: app_state::Notice) -> Self {
        Self { kind: notice.kind, text: notice.text, action: None }
    }

    fn info(text: impl Into<String>) -> Self {
        Self { kind: app_state::NoticeKind::Info, text: text.into(), action: None }
    }

    fn warning(text: impl Into<String>) -> Self {
        Self { kind: app_state::NoticeKind::Warning, text: text.into(), action: None }
    }

    fn with_action(mut self, action: NoticeAction) -> Self {
        self.action = Some(action);
        self
    }
}

/// Laufendes Ziehen einer Zeile der Mod-Liste.
#[derive(Debug, Clone)]
pub struct Drag {
    /// Gezogenes Pak.
    pub pak: String,
    /// Stelle, an der die Einfügemarke steht – gezählt in der unveränderten
    /// Liste, `entries.len()` bedeutet „ans Ende“.
    pub drop_index: Option<usize>,
}

/// Eine vom Nutzer ausgelöste Absicht, gesammelt beim Zeichnen und danach
/// angewendet (siehe Modulkommentar).
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    ShowSection(Section),
    SelectMod(String),
    ClearSelection,
    ToggleMod(String),
    MoveMod { pak: String, delta: isize },
    DropMod { pak: String, before: usize },
    Import,
    PickGameDir,
    RetryDiscover,
    OpenFolder(PathBuf),
    SetLaunchChoice(LaunchChoice),
    ToggleLaunchMenu,
    Launch,
    ConfirmVanilla,
    SetFilter(String),
    SetProfileName(String),
    SetBackupLabel(String),
    DragStart(String),
    DragOver(usize),
    SaveProfile,
    ApplyProfile(String),
    AskDeleteProfile(String),
    ConfirmDeleteProfile,
    CreateBackup,
    VerifyBackup(usize),
    AskRenameBackup(usize),
    SetRenameLabel(String),
    ConfirmRenameBackup,
    AskDeleteBackup(usize),
    ConfirmDeleteBackup,
    ShowBackupInFiles(usize),
    AskRestore(usize),
    SetRestoreForce(bool),
    ConfirmRestore,
    OpenSteamUserDialog,
    PickSteamUser(String),
    ConfirmSteamUser,
    ToggleAutoBackup,
    CloseDialog,
    DismissNotice(usize),
    TriggerNotice(usize),
    CancelTask,
}

/// Der gesamte Zustand der Oberfläche.
pub struct App {
    /// Handgriff auf den Zeichenkontext, damit ein Hintergrundthread ein
    /// Neuzeichnen anfordern kann, sobald er fertig ist.
    egui_ctx: egui::Context,
    /// Basisverzeichnisse – auch dann bekannt, wenn das Spiel nicht
    /// gefunden wurde.
    dirs: Option<AppDirs>,
    /// Einstellungen, solange kein `AppState` existiert. Sobald einer da
    /// ist, gilt dessen Kopie (siehe `settings`/`settings_mut`).
    fallback_settings: Settings,
    /// Fachlicher Zustand, oder `None`, wenn das Spiel nicht gefunden wurde.
    state: Option<AppState>,
    /// Grund, warum kein `AppState` zustande kam.
    open_error: Option<String>,
    /// Ist das Mods-Verzeichnis beschreibbar? Einmal beim Laden ermittelt.
    writable: bool,
    /// Steht ein Direktstart unter Umgehung von EAC zur Verfügung?
    no_eac_available: bool,

    section: Section,
    notices: Vec<Notice>,
    filter: String,
    selected: Option<String>,
    launch_choice: LaunchChoice,
    launch_menu_open: bool,
    dialog: Option<Dialog>,
    status: String,
    status_is_warning: bool,

    profiles: Vec<Profile>,
    profile_name: String,

    backups: Vec<BackupEntry>,
    backup_label: String,
    /// Zeitstempel der Backups, die in dieser Sitzung geprüft wurden.
    verified: HashSet<String>,
    /// Grund, warum die Savegame-Funktionen gesperrt sind – im Entwurf der
    /// Zustand „Mehrere Steam-Profile“.
    saves_blocked: Option<String>,
    /// Im Prefix gefundene Steam-Nutzerprofile, für die Auswahl.
    steam_users: Vec<String>,

    drag: Option<Drag>,
    task: Option<tasks::Running>,
}

impl App {
    fn new(egui_ctx: egui::Context) -> Self {
        let mut app = Self {
            egui_ctx,
            dirs: None,
            fallback_settings: Settings::default(),
            state: None,
            open_error: None,
            writable: false,
            no_eac_available: launch::no_eac_available(),
            section: Section::Mods,
            notices: Vec::new(),
            filter: String::new(),
            selected: None,
            launch_choice: LaunchChoice::Steam,
            launch_menu_open: false,
            dialog: None,
            status: String::from("Bereit."),
            status_is_warning: false,
            profiles: Vec::new(),
            profile_name: String::new(),
            backups: Vec::new(),
            backup_label: String::new(),
            verified: HashSet::new(),
            saves_blocked: None,
            steam_users: Vec::new(),
            drag: None,
            task: None,
        };
        app.load();
        app
    }

    /// Lädt oder lädt neu: Basisverzeichnisse, Einstellungen, Spielpfade,
    /// Bibliothek, Konfiguration, Profile und Backups.
    fn load(&mut self) {
        let (dirs, settings) = match app_state::load_dirs_and_settings() {
            Ok(pair) => pair,
            Err(e) => {
                self.open_error = Some(format!("{e:#}"));
                self.set_warning(format!("Anwendungsverzeichnisse nicht nutzbar – {e:#}"));
                return;
            }
        };
        self.dirs = Some(dirs.clone());
        self.fallback_settings = settings.clone();

        match AppState::open_with(dirs, settings) {
            Ok(mut state) => {
                self.notices = state.take_notices().into_iter().map(Notice::from_state).collect();
                self.writable = state.mods_dir_is_writable();
                self.open_error = None;
                self.state = Some(state);
                if !self.writable {
                    self.notices.push(
                        Notice::warning(
                            "Das Mods-Verzeichnis ist schreibgeschützt. Aktivieren, Reihenfolge \
                             und Import sind deshalb gesperrt – Lesen und Starten gehen weiter.",
                        )
                        .with_action(NoticeAction::OpenModsDir),
                    );
                    self.set_warning("Nur Lesen möglich – Änderungen werden nicht gespeichert.");
                } else {
                    self.set_status("Bereit.");
                }
            }
            Err(e) => {
                self.state = None;
                self.writable = false;
                self.open_error = Some(format!("{e:#}"));
                self.set_warning(
                    "Spielverzeichnis unbekannt – Mods und Ladereihenfolge nicht lesbar.",
                );
            }
        }

        self.refresh_profiles();
        self.refresh_backups();
        self.refresh_steam_users();
    }

    fn refresh_profiles(&mut self) {
        let Some(dir) = self.profiles_dir() else { return };
        self.profiles = list_profiles(&dir).unwrap_or_default();
    }

    fn refresh_backups(&mut self) {
        let Some(dir) = self.backups_dir() else { return };
        self.backups = saves::list_backups(&dir).unwrap_or_default();
    }

    /// Ermittelt, ob das Savegame-Verzeichnis eindeutig bestimmbar ist. Bei
    /// mehreren Steam-Nutzerprofilen im Prefix sind laut Entwurf alle
    /// Savegame-Funktionen gesperrt, bis eines gewählt wurde.
    fn refresh_steam_users(&mut self) {
        let Some(state) = &self.state else {
            self.saves_blocked = Some(String::from("Spielverzeichnis unbekannt."));
            return;
        };
        match state.paths.save_dir(state.settings.steam_user.as_deref()) {
            Ok(_) => {
                self.saves_blocked = None;
            }
            Err(sm2_core::Error::AmbiguousSaveUser(users)) => {
                self.steam_users = users;
                self.saves_blocked = Some(format!(
                    "Im Proton-Prefix liegen {} Steam-Nutzerprofile. Der Loader kann nicht \
                     entscheiden, welche Spielstände gemeint sind – bis zur Auswahl sind alle \
                     Savegame-Funktionen gesperrt.",
                    self.steam_users.len()
                ));
            }
            Err(e) => {
                self.saves_blocked = Some(e.to_string());
            }
        }
    }

    fn settings(&self) -> &Settings {
        self.state.as_ref().map_or(&self.fallback_settings, |s| &s.settings)
    }

    fn profiles_dir(&self) -> Option<PathBuf> {
        self.dirs.as_ref().map(|d| d.data.join("profiles"))
    }

    fn backups_dir(&self) -> Option<PathBuf> {
        self.dirs.as_ref().map(|d| d.data.join("backups/saves"))
    }

    fn set_status(&mut self, text: impl Into<String>) {
        self.status = text.into();
        self.status_is_warning = false;
    }

    fn set_warning(&mut self, text: impl Into<String>) {
        self.status = text.into();
        self.status_is_warning = true;
    }

    /// Sichert die Einstellungen und meldet einen Fehlschlag in der
    /// Statusleiste, statt ihn zu verschlucken.
    fn save_settings(&mut self) {
        let Some(dirs) = self.dirs.clone() else { return };
        let settings = self.settings().clone();
        self.fallback_settings = settings.clone();
        if let Err(e) = settings.save(&dirs.config.join("settings.toml")) {
            self.set_warning(format!("Einstellungen konnten nicht gespeichert werden – {e}"));
        }
    }

    /// Schreibt Bibliothek und Konfiguration und meldet das Ergebnis.
    /// Gibt zurück, ob das Speichern gelungen ist – der Aufrufer entscheidet,
    /// ob er seine Erfolgsmeldung noch setzen darf.
    fn persist(&mut self) -> bool {
        let Some(state) = &mut self.state else { return false };
        match state.persist() {
            Ok(()) => {
                let extra: Vec<Notice> =
                    state.take_notices().into_iter().map(Notice::from_state).collect();
                self.notices.extend(extra);
                true
            }
            Err(e) => {
                self.set_warning(format!("Nicht gespeichert – {e:#}"));
                false
            }
        }
    }

    /// Anzeigename eines Paks: der Name aus der Bibliothek, sonst der
    /// Dateiname – dieselbe Regel wie in `cli.rs`s `list`.
    fn display_name(&self, pak: &str) -> String {
        self.mod_info(pak).map_or_else(|| pak.to_string(), |info| info.name.clone())
    }

    fn mod_info(&self, pak: &str) -> Option<&ModInfo> {
        self.state.as_ref().and_then(|s| s.library.mods.get(pak))
    }

    fn entries(&self) -> &[PakEntry] {
        self.state.as_ref().map_or(&[], |s| s.config.entries.as_slice())
    }

    /// Dürfen Aktivierung, Reihenfolge und Import gerade verändert werden?
    ///
    /// Nein, solange das Mods-Verzeichnis schreibgeschützt ist – und nein,
    /// solange ein Hintergrundauftrag läuft: dessen Arbeitskopie von
    /// Bibliothek und Konfiguration würde eine zwischenzeitliche Änderung
    /// beim Zurückschreiben überschreiben.
    fn can_modify(&self) -> bool {
        self.state.is_some() && self.writable && self.task.is_none()
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::ShowSection(section) => {
                self.section = section;
                self.launch_menu_open = false;
            }
            Action::SelectMod(pak) => self.selected = Some(pak),
            Action::ClearSelection => self.selected = None,
            Action::ToggleMod(pak) => self.toggle_mod(&pak),
            Action::MoveMod { pak, delta } => self.move_mod(&pak, delta),
            Action::DropMod { pak, before } => {
                self.drag = None;
                self.drop_mod(&pak, before);
            }
            Action::Import => self.start_import(),
            Action::PickGameDir => self.pick_game_dir(),
            Action::RetryDiscover => {
                self.load();
                if self.state.is_none() {
                    self.set_warning("Steam-Bibliotheken erneut durchsucht – nichts gefunden.");
                }
            }
            Action::OpenFolder(path) => self.open_folder(path),
            Action::SetLaunchChoice(choice) => {
                self.launch_choice = choice;
                self.launch_menu_open = false;
            }
            Action::ToggleLaunchMenu => self.launch_menu_open = !self.launch_menu_open,
            Action::Launch => self.launch(),
            Action::ConfirmVanilla => {
                self.dialog = None;
                self.perform_launch(true, false);
            }
            Action::SetFilter(filter) => self.filter = filter,
            Action::SetProfileName(name) => self.profile_name = name,
            Action::SetBackupLabel(label) => self.backup_label = label,
            Action::DragStart(pak) => {
                self.selected = Some(pak.clone());
                self.drag = Some(Drag { pak, drop_index: None });
            }
            Action::DragOver(index) => {
                if let Some(drag) = &mut self.drag {
                    drag.drop_index = Some(index);
                }
            }
            Action::SaveProfile => self.save_profile(),
            Action::ApplyProfile(name) => self.apply_profile(&name),
            Action::AskDeleteProfile(name) => self.dialog = Some(Dialog::DeleteProfile { name }),
            Action::ConfirmDeleteProfile => self.delete_profile(),
            Action::CreateBackup => self.start_backup(),
            Action::VerifyBackup(index) => self.start_verify(index),
            Action::AskRenameBackup(index) => self.ask_rename_backup(index),
            Action::SetRenameLabel(value) => {
                if let Some(Dialog::RenameBackup { label, .. }) = &mut self.dialog {
                    *label = value;
                }
            }
            Action::ConfirmRenameBackup => self.confirm_rename_backup(),
            Action::AskDeleteBackup(index) => {
                if index < self.backups.len() {
                    self.dialog = Some(Dialog::DeleteBackup { index });
                }
            }
            Action::ConfirmDeleteBackup => self.delete_backup(),
            Action::ShowBackupInFiles(index) => self.show_backup_in_files(index),
            Action::AskRestore(index) => self.ask_restore(index),
            Action::SetRestoreForce(value) => {
                if let Some(Dialog::Restore { force, .. }) = &mut self.dialog {
                    *force = value;
                }
            }
            Action::ConfirmRestore => self.confirm_restore(),
            Action::OpenSteamUserDialog => {
                let picked = self
                    .settings()
                    .steam_user
                    .clone()
                    .or_else(|| self.steam_users.first().cloned());
                self.dialog = Some(Dialog::SteamUser { picked });
            }
            Action::PickSteamUser(user) => {
                if let Some(Dialog::SteamUser { picked }) = &mut self.dialog {
                    *picked = Some(user);
                }
            }
            Action::ConfirmSteamUser => self.confirm_steam_user(),
            Action::ToggleAutoBackup => self.toggle_auto_backup(),
            Action::CloseDialog => self.dialog = None,
            Action::DismissNotice(index) => {
                if index < self.notices.len() {
                    self.notices.remove(index);
                }
            }
            Action::TriggerNotice(index) => self.run_notice_action(index),
            Action::CancelTask => {
                if let Some(task) = &self.task {
                    task.cancel();
                    self.set_status("Abbruch angefordert – der laufende Schritt wird beendet.");
                }
            }
        }
    }

    fn toggle_mod(&mut self, pak: &str) {
        if !self.can_modify() {
            self.set_warning(self.blocked_reason("Änderung"));
            return;
        }
        let Some(state) = &mut self.state else { return };
        let Some(entry) = state.config.entries.iter_mut().find(|e| e.pak == pak) else { return };
        entry.disabled = !entry.disabled;
        let now_disabled = entry.disabled;
        if self.persist() {
            let name = self.display_name(pak);
            let verb = if now_disabled { "deaktiviert" } else { "aktiviert" };
            self.set_status(format!("{name} {verb} – in pak_config.yaml geschrieben."));
        }
    }

    fn move_mod(&mut self, pak: &str, delta: isize) {
        if !self.can_modify() {
            self.set_warning(self.blocked_reason("Verschieben"));
            return;
        }
        let Some(state) = &mut self.state else { return };
        let Some(from) = state.config.entries.iter().position(|e| e.pak == pak) else { return };
        let to = from as isize + delta;
        if to < 0 || to as usize >= state.config.entries.len() {
            return;
        }
        let to = to as usize;
        state.config.entries.swap(from, to);
        let total = state.config.entries.len();
        self.selected = Some(pak.to_string());
        if self.persist() {
            self.set_status(format!(
                "Ladereihenfolge geändert – Position {} von {total}.",
                to + 1
            ));
        }
    }

    /// Schiebt das gezogene Pak vor die Einfügemarke.
    ///
    /// Die Marke zählt in der unveränderten Liste; lag das gezogene Pak
    /// vorher darüber, rutscht der Zielindex deshalb um eins zurück. Ohne
    /// diese Korrektur landete ein nach unten gezogenes Pak stets eine
    /// Position zu tief.
    fn drop_mod(&mut self, pak: &str, before: usize) {
        if !self.can_modify() {
            self.set_warning(self.blocked_reason("Verschieben"));
            return;
        }
        let Some(state) = &mut self.state else { return };
        let Some(from) = state.config.entries.iter().position(|e| e.pak == pak) else { return };
        let Some(to) = drop_target(from, before) else { return };

        let moved = state.config.entries.remove(from);
        state.config.entries.insert(to, moved);
        let total = state.config.entries.len();
        self.selected = Some(pak.to_string());
        if self.persist() {
            let name = self.display_name(pak);
            self.set_status(format!("{name} auf Position {} von {total} gezogen.", to + 1));
        }
    }

    /// Warum eine Änderung gerade nicht geht – in der Reihenfolge, in der es
    /// den Nutzer betrifft.
    fn blocked_reason(&self, what: &str) -> String {
        if self.state.is_none() {
            format!("{what} nicht möglich – Spielverzeichnis unbekannt.")
        } else if self.task.is_some() {
            format!("{what} nicht möglich, solange ein Vorgang läuft.")
        } else {
            format!("{what} nicht möglich – Mods-Verzeichnis ist schreibgeschützt.")
        }
    }

    fn open_folder(&mut self, path: PathBuf) {
        if let Err(e) = std::fs::create_dir_all(&path) {
            self.set_warning(format!("{} nicht vorhanden – {e}", path.display()));
            return;
        }
        match Current::open_folder(&path) {
            Ok(()) => self.set_status(format!("{} im Dateimanager geöffnet.", path.display())),
            Err(e) => self.set_warning(format!("Öffnen fehlgeschlagen – {e}")),
        }
    }

    fn run_notice_action(&mut self, index: usize) {
        let Some(notice) = self.notices.get(index) else { return };
        match notice.action.clone() {
            Some(NoticeAction::OpenModsDir) => {
                if let Some(state) = &self.state {
                    let dir = state.paths.mods_dir();
                    self.open_folder(dir);
                }
            }
            Some(NoticeAction::ShowProfile(name)) => {
                self.section = Section::Profiles;
                self.set_status(format!("Profil „{name}“ – mit Anwenden zurückstellen."));
            }
            None => {}
        }
    }
}

/// Rechnet die Einfügemarke einer Zieh-Bewegung in den Zielindex um.
///
/// `before` ist die Stelle in der **unveränderten** Liste, vor die das Pak
/// soll. Nach dem Entfernen an `from` verschiebt sich alles dahinter um eins
/// nach vorn, deshalb die Korrektur. Ein Ablegen auf die eigene Position
/// (davor oder dahinter) ergibt `None` – dann ist nichts zu tun, und ohne
/// diese Prüfung entstünde eine Statusmeldung über eine Verschiebung, die
/// gar nicht stattgefunden hat.
fn drop_target(from: usize, before: usize) -> Option<usize> {
    let to = if from < before { before.checked_sub(1)? } else { before };
    (to != from).then_some(to)
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_task(&ctx);

        let mut actions: Vec<Action> = Vec::new();

        egui::Panel::top("top_bar")
            .exact_size(theme::metric::TOP_BAR_HEIGHT)
            .frame(
                egui::Frame::new().fill(theme::color::TOP_BAR).inner_margin(egui::Margin {
                    left: 14,
                    right: 14,
                    top: 0,
                    bottom: 0,
                }),
            )
            .show_separator_line(false)
            .show(ui, |ui| top_bar::show(self, ui, &mut actions));

        egui::Panel::bottom("status_bar")
            .exact_size(theme::metric::STATUS_BAR_HEIGHT)
            .frame(
                egui::Frame::new().fill(theme::color::PANEL).inner_margin(egui::Margin {
                    left: 14,
                    right: 14,
                    top: 0,
                    bottom: 0,
                }),
            )
            .show_separator_line(false)
            .show(ui, |ui| status_bar::show(self, ui, &mut actions));

        egui::Panel::left("side_bar")
            .exact_size(theme::metric::SIDE_BAR_WIDTH)
            .resizable(false)
            .frame(
                egui::Frame::new().fill(theme::color::PANEL).inner_margin(egui::Margin {
                    left: 10,
                    right: 10,
                    top: 12,
                    bottom: 12,
                }),
            )
            .show_separator_line(false)
            .show(ui, |ui| side_bar::show(self, ui, &mut actions));

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::color::WINDOW)
                    .inner_margin(egui::Margin::same(theme::metric::CONTENT_PADDING as i8)),
            )
            .show(ui, |ui| match self.section {
                Section::Mods => mods_page::show(self, ui, &mut actions),
                Section::Profiles => profiles_page::show(self, ui, &mut actions),
                Section::Saves => saves_page::show(self, ui, &mut actions),
                Section::Settings => settings_page::show(self, ui, &mut actions),
            });

        dialogs::show(self, &ctx, &mut actions);
        self.draw_panel_borders(&ctx);
        self.handle_dropped_files(&ctx, &mut actions);
        self.handle_keyboard(&ctx, &mut actions);

        for action in actions {
            self.apply(action);
        }
    }
}

impl App {
    /// Zieht die Trennlinien zwischen den Panels nach.
    ///
    /// `show_separator_line(false)` schaltet die eigenen Linien von `egui`
    /// ab, weil sie die Farbe aus `Visuals` nehmen und außerdem einen
    /// Schatten mitbringen; der Entwurf verlangt einen genau 1 px breiten
    /// Strich in `BORDER_SOFT`.
    fn draw_panel_borders(&self, ctx: &egui::Context) {
        let screen = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("panel_borders"),
        ));
        let stroke = egui::Stroke::new(1.0, theme::color::BORDER_SOFT);

        let top = screen.top() + theme::metric::TOP_BAR_HEIGHT;
        painter.hline(screen.x_range(), top - 0.5, stroke);

        let bottom = screen.bottom() - theme::metric::STATUS_BAR_HEIGHT;
        painter.hline(screen.x_range(), bottom + 0.5, stroke);

        let side = screen.left() + theme::metric::SIDE_BAR_WIDTH;
        painter.vline(side - 0.5, egui::Rangef::new(top, bottom), stroke);
    }

    /// Dateien, die der Nutzer auf das Fenster gezogen hat, importieren –
    /// der Entwurf nennt das in der Zeile über der Liste ausdrücklich.
    fn handle_dropped_files(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect()
        });
        if dropped.is_empty() {
            return;
        }
        if !self.can_modify() {
            self.set_warning(self.blocked_reason("Import"));
            return;
        }
        self.section = Section::Mods;
        self.begin_import(dropped);
        let _ = actions;
    }

    /// Alt + Pfeiltaste verschiebt den ausgewählten Mod – die
    /// Tastaturalternative zum Ziehen, die der Entwurf in der Zeile über der
    /// Liste vorsieht.
    fn handle_keyboard(&self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let Some(pak) = &self.selected else { return };
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        ctx.input(|i| {
            if !i.modifiers.alt {
                return;
            }
            if i.key_pressed(egui::Key::ArrowUp) {
                actions.push(Action::MoveMod { pak: pak.clone(), delta: -1 });
            }
            if i.key_pressed(egui::Key::ArrowDown) {
                actions.push(Action::MoveMod { pak: pak.clone(), delta: 1 });
            }
        });
    }
}


/// Überschrift und Erläuterung über einer Karte – Profile und Savegames
/// benutzen dieselbe Form.
fn page_heading(ui: &mut egui::Ui, title: &str, description: &str, max_width: f32) {
    ui.label(
        egui::RichText::new(title).font(theme::medium(15.0)).color(theme::color::TEXT_STRONG),
    );
    ui.add_space(4.0);
    let width = max_width.min(ui.available_width());
    let galley = ui.painter().layout(
        description.to_owned(),
        theme::sans(12.0),
        theme::color::TEXT_DIM2,
        width,
    );
    let (rect, _) = ui.allocate_exact_size(
        egui::Vec2::new(ui.available_width(), galley.size().y),
        egui::Sense::hover(),
    );
    ui.painter().galley(rect.min, galley, theme::color::TEXT_DIM2);
    ui.add_space(theme::metric::CONTENT_GAP);
}

/// Die Fläche einer Karte: Füllung, 1-px-Rahmen, 12-px-Rundung.
fn draw_card(ui: &egui::Ui, rect: egui::Rect) {
    let radius = egui::CornerRadius::same(12);
    ui.painter().rect_filled(rect, radius, theme::color::CARD);
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.0, theme::color::BORDER_SOFT),
        egui::StrokeKind::Inside,
    );
}

/// Der Spaltenkopf einer Tabelle: gesperrte Großbuchstaben auf dunklerem
/// Grund, unten abgeschlossen durch eine Trennlinie.
fn draw_column_head(
    ui: &egui::Ui,
    rect: egui::Rect,
    spec: &[widgets::Column],
    titles: &[&str],
) {
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::ZERO, theme::color::TABLE_HEAD);
    painter.hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(1.0, theme::color::BORDER_SOFT),
    );

    let inner = rect.shrink2(egui::Vec2::new(theme::metric::CARD_PADDING, 0.0));
    let cells = widgets::columns(inner, spec, theme::metric::COLUMN_GAP);
    for (cell, title) in cells.iter().zip(titles) {
        if title.is_empty() {
            continue;
        }
        let galley = painter.layout_job(widgets::tracked_text(
            title,
            theme::sans(10.5),
            theme::color::TEXT_FAINT,
            0.75,
        ));
        painter.galley(
            egui::Pos2::new(cell.left(), cell.center().y - galley.size().y / 2.0),
            galley,
            theme::color::TEXT_FAINT,
        );
    }
}

/// Text in der Mitte einer leeren Tabelle.
fn empty_hint(ui: &mut egui::Ui, text: &str) {
    let rect = ui.available_rect_before_wrap();
    ui.painter().text(
        egui::Pos2::new(rect.center().x, rect.top() + 48.0),
        egui::Align2::CENTER_CENTER,
        text,
        theme::sans(12.5),
        theme::color::TEXT_DIM2,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nach_unten_ziehen_landet_genau_vor_der_einfuegemarke() {
        // Aus Position 0 vor Position 3 gezogen: nach dem Entfernen rutscht
        // alles dahinter um eins vor, das Ziel ist also 2.
        assert_eq!(drop_target(0, 3), Some(2));
    }

    #[test]
    fn nach_oben_ziehen_braucht_keine_korrektur() {
        assert_eq!(drop_target(4, 1), Some(1));
    }

    #[test]
    fn ablegen_auf_der_eigenen_position_veraendert_nichts() {
        assert_eq!(drop_target(2, 2), None, "Marke direkt über der eigenen Zeile");
        assert_eq!(drop_target(2, 3), None, "Marke direkt unter der eigenen Zeile");
    }

    #[test]
    fn ans_ende_ziehen_trifft_die_letzte_position() {
        // Sieben Einträge, das erste Pak ans Ende: Marke steht auf 7.
        assert_eq!(drop_target(0, 7), Some(6));
    }
}

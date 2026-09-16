//! The graphical interface, built one to one from the design
//! `SM2 Mod Loader GUI v2 modern.dc.html`.
//!
//! Layout of the window, from the outside in:
//!
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │ Top bar: heading · launch choice             │  58 px
//! ├──────────┬───────────────────────────────────┤
//! │ Sidebar  │ Content: mods / profiles /        │
//! │ 208 px   │ savegames / settings              │
//! ├──────────┴───────────────────────────────────┤
//! │ Status bar: progress · message               │  30 px
//! └──────────────────────────────────────────────┘
//! ```
//!
//! The design's own window title bar is deliberately not reproduced. The
//! design says so itself in its closing note: the title bar is only
//! decoration of the mockup, and the real window gets the system's
//! decoration.
//!
//! Events are not carried out on the spot. They are collected as `Action`
//! values and applied once drawing is done. `egui` draws immediately: a
//! click on a row arrives while the list is still being iterated over, and
//! changing that very list right there is the classic route into a borrow
//! error or a half-drawn row.

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
mod toasts;
mod top_bar;
mod widgets;

use crate::app_state::{self, AppState};
use sm2_core::i18n::Language;
use sm2_core::launch;
use sm2_core::library::ModInfo;
use sm2_core::pak_config::PakEntry;
use sm2_core::paths::AppDirs;
use sm2_core::platform::{Current, Platform};
use sm2_core::profile::{list_profiles, Profile};
use sm2_core::saves::{self, BackupEntry};
use sm2_core::settings::Settings;
use sm2_core::t;
use std::collections::HashSet;
use std::path::PathBuf;

/// Starts the interface. Returns once the user closes the window.
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

/// A section of the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Mods,
    Profiles,
    Saves,
    Settings,
}

/// The kind of launch chosen in the top bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchChoice {
    Steam,
    Vanilla,
    NoEac,
}

impl LaunchChoice {
    fn label(self) -> String {
        match self {
            Self::Steam => t!("gui.top_bar.choice_steam"),
            Self::Vanilla => t!("gui.top_bar.choice_vanilla"),
            Self::NoEac => t!("gui.top_bar.choice_no_eac"),
        }
    }
}

/// An open modal window.
#[derive(Debug, Clone, PartialEq)]
pub enum Dialog {
    /// The "restore despite Steam running?" dialog
    /// (`gui.dialog.restore_title`) — this is the only dialog that can
    /// overwrite savegames, which is why it hangs off a confirmation
    /// toggle of its own.
    Restore { index: usize, force: bool },
    /// The "launch without mods?" dialog (`gui.dialog.vanilla_title`).
    Vanilla,
    /// The "choose Steam user profile" dialog
    /// (`gui.dialog.steam_user_title`).
    SteamUser { picked: Option<String> },
    /// The "choose language" dialog (`gui.dialog.language_title`).
    Language { picked: Option<Language> },
    /// The "delete profile?" dialog (`gui.dialog.delete_profile_title`).
    DeleteProfile { name: String },
    /// The "rename backup" dialog (`gui.dialog.rename_backup_title`) — the
    /// label is what lets you recognise a backup again later on.
    RenameBackup { index: usize, label: String },
    /// The "delete backup?" dialog (`gui.dialog.delete_backup_title`).
    DeleteBackup { index: usize },
}

/// What a notice row offers as a button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoticeAction {
    /// "Verzeichnis öffnen" — for a read-only mods directory.
    OpenModsDir,
    /// "Profil anwenden" — after a vanilla launch, carrying the name of the
    /// saved profile.
    ShowProfile(String),
}

/// One row of the notice bar above the mod list.
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

/// A drag of a mod-list row that is currently in progress.
#[derive(Debug, Clone)]
pub struct Drag {
    /// The pak being dragged.
    pub pak: String,
    /// Where the insertion marker sits — counted in the unchanged list,
    /// where `entries.len()` means "at the end".
    pub drop_index: Option<usize>,
}

/// An intent triggered by the user, collected while drawing and applied
/// afterwards (see the module comment).
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
    ImportBackup,
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
    OpenLanguageDialog,
    PickLanguage(Language),
    ConfirmLanguage,
    ToggleAutoBackup,
    CloseDialog,
    DismissNotice(usize),
    DismissToast(u64),
    TriggerNotice(usize),
    CancelTask,
}

/// The entire state of the interface.
pub struct App {
    /// A handle on the drawing context, so that a background thread can ask
    /// for a repaint as soon as it is done.
    egui_ctx: egui::Context,
    /// The base directories — known even when the game was not found.
    dirs: Option<AppDirs>,
    /// The settings for as long as no `AppState` exists. Once one is there,
    /// its copy is the one that counts (see `settings`/`settings_mut`).
    fallback_settings: Settings,
    /// The domain state, or `None` if the game was not found.
    state: Option<AppState>,
    /// Why no `AppState` could be built.
    open_error: Option<String>,
    /// Is the mods directory writable? Determined once while loading.
    writable: bool,
    /// Is a direct launch bypassing EAC available?
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
    /// Timestamps of the backups verified during this session.
    verified: HashSet<String>,
    /// Why the savegame functions are locked — in the design this is the
    /// "Mehrere Steam-Profile" state.
    saves_blocked: Option<String>,
    /// The Steam user profiles found in the prefix, for the picker.
    steam_users: Vec<String>,

    drag: Option<Drag>,
    task: Option<tasks::Running>,
    /// The transient messages over the lower right corner — see
    /// `toasts`. Fed by `set_status`/`set_warning`.
    toasts: toasts::Toasts,
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
            status: t!("gui.message.ready"),
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
            toasts: toasts::Toasts::default(),
        };
        app.load();
        app
    }

    /// Loads, or reloads: base directories, settings, game paths, library,
    /// configuration, profiles and backups.
    fn load(&mut self) {
        let (dirs, settings) = match app_state::load_dirs_and_settings() {
            Ok(pair) => pair,
            Err(e) => {
                self.open_error = Some(format!("{e:#}"));
                self.set_warning(t!("gui.message.app_dirs_unusable", detail = format!("{e:#}")));
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
                        Notice::warning(t!("gui.message.mods_dir_read_only"))
                            .with_action(NoticeAction::OpenModsDir),
                    );
                    self.set_warning(t!("gui.message.read_only_mode"));
                } else {
                    self.set_status(t!("gui.message.ready"));
                }
            }
            Err(e) => {
                self.state = None;
                self.writable = false;
                self.open_error = Some(format!("{e:#}"));
                self.set_warning(t!("gui.message.game_dir_unknown"));
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

    /// Works out whether the savegame directory can be determined
    /// unambiguously. With several Steam user profiles in the prefix, the
    /// design locks every savegame function until one has been chosen.
    fn refresh_steam_users(&mut self) {
        let Some(state) = &self.state else {
            self.saves_blocked = Some(t!("gui.message.save_dir_unknown"));
            return;
        };
        match state.paths.save_dir(state.settings.steam_user.as_deref()) {
            Ok(_) => {
                self.saves_blocked = None;
            }
            Err(sm2_core::Error::AmbiguousSaveUser(users)) => {
                self.steam_users = users;
                self.saves_blocked = Some(t!(
                    "gui.message.ambiguous_steam_profiles",
                    count = self.steam_users.len()
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

    /// Reports a result: into the status bar and, on top of that, as a
    /// toast. Everything that has happened goes through here, which is why
    /// no action needs a toast call of its own.
    fn set_status(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.push_toast(&text, toasts::ToastKind::Success);
        self.set_busy(text);
    }

    fn set_warning(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.push_toast(&text, toasts::ToastKind::Warning);
        self.status = text;
        self.status_is_warning = true;
    }

    /// Reports work that is *running*: status bar only, no toast. The
    /// progress bar right next to it already says the same thing, and the
    /// result follows in a moment and gets a toast of its own.
    fn set_busy(&mut self, text: impl Into<String>) {
        self.status = text.into();
        self.status_is_warning = false;
    }

    fn push_toast(&mut self, text: &str, kind: toasts::ToastKind) {
        let now = self.egui_ctx.input(|input| input.time);
        self.toasts.push(text, kind, now);
    }

    /// Saves the settings and reports a failure in the status bar instead
    /// of swallowing it.
    fn save_settings(&mut self) {
        let Some(dirs) = self.dirs.clone() else { return };
        let settings = self.settings().clone();
        self.fallback_settings = settings.clone();
        if let Err(e) = settings.save(&dirs.config.join("settings.toml")) {
            self.set_warning(t!("gui.message.settings_save_failed", detail = e));
        }
    }

    /// Writes library and configuration and reports the result. Returns
    /// whether saving succeeded — from that the caller decides whether it
    /// may still post its own success message.
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
                self.set_warning(t!("gui.message.persist_failed", detail = format!("{e:#}")));
                false
            }
        }
    }

    /// Display name of a pak: the name from the library, otherwise the file
    /// name — the same rule as in `cli.rs`'s `list`.
    fn display_name(&self, pak: &str) -> String {
        self.mod_info(pak).map_or_else(|| pak.to_string(), |info| info.name.clone())
    }

    fn mod_info(&self, pak: &str) -> Option<&ModInfo> {
        self.state.as_ref().and_then(|s| s.library.mods.get(pak))
    }

    fn entries(&self) -> &[PakEntry] {
        self.state.as_ref().map_or(&[], |s| s.config.entries.as_slice())
    }

    /// May activation, order and import be changed right now?
    ///
    /// No, not while the mods directory is read-only — and no, not while a
    /// background job is running: its working copy of library and
    /// configuration would overwrite any change made in the meantime once
    /// it writes back.
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
                    self.set_warning(t!("gui.message.discover_retry_nothing_found"));
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
            Action::ImportBackup => self.start_backup_import(),
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
            Action::OpenLanguageDialog => {
                self.dialog = Some(Dialog::Language { picked: Some(self.settings().language()) });
            }
            Action::PickLanguage(language) => {
                if let Some(Dialog::Language { picked }) = &mut self.dialog {
                    *picked = Some(language);
                }
            }
            Action::ConfirmLanguage => self.confirm_language(),
            Action::ToggleAutoBackup => self.toggle_auto_backup(),
            Action::CloseDialog => self.dialog = None,
            Action::DismissToast(id) => self.toasts.dismiss(id),
            Action::DismissNotice(index) => {
                if index < self.notices.len() {
                    self.notices.remove(index);
                }
            }
            Action::TriggerNotice(index) => self.run_notice_action(index),
            Action::CancelTask => {
                if let Some(task) = &self.task {
                    task.cancel();
                    self.set_status(t!("gui.message.task_cancel_requested"));
                }
            }
        }
    }

    fn toggle_mod(&mut self, pak: &str) {
        if !self.can_modify() {
            self.set_warning(self.blocked_reason(&t!("gui.message.action_change")));
            return;
        }
        let Some(state) = &mut self.state else { return };
        let Some(entry) = state.config.entries.iter_mut().find(|e| e.pak == pak) else { return };
        entry.disabled = !entry.disabled;
        let now_disabled = entry.disabled;
        if self.persist() {
            let name = self.display_name(pak);
            let verb = if now_disabled {
                t!("gui.message.state_disabled")
            } else {
                t!("gui.message.state_enabled")
            };
            self.set_status(t!("gui.message.mod_toggled", name = name, verb = verb));
        }
    }

    fn move_mod(&mut self, pak: &str, delta: isize) {
        if !self.can_modify() {
            self.set_warning(self.blocked_reason(&t!("gui.message.action_move")));
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
            self.set_status(t!(
                "gui.message.load_order_changed",
                position = to + 1,
                total = total
            ));
        }
    }

    /// Moves the dragged pak in front of the insertion marker.
    ///
    /// The marker counts in the unchanged list, so if the dragged pak sat
    /// above it before, the target index moves back by one. Without that
    /// correction a pak dragged downwards always landed one position too
    /// low.
    fn drop_mod(&mut self, pak: &str, before: usize) {
        if !self.can_modify() {
            self.set_warning(self.blocked_reason(&t!("gui.message.action_move")));
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
            self.set_status(t!(
                "gui.message.pak_dragged",
                name = name,
                position = to + 1,
                total = total
            ));
        }
    }

    /// Why a change is not possible right now — in the order in which it
    /// affects the user.
    fn blocked_reason(&self, what: &str) -> String {
        if self.state.is_none() {
            t!("gui.message.blocked_no_game_dir", what = what)
        } else if self.task.is_some() {
            t!("gui.message.blocked_task_running", what = what)
        } else {
            t!("gui.message.blocked_read_only", what = what)
        }
    }

    fn open_folder(&mut self, path: PathBuf) {
        if let Err(e) = std::fs::create_dir_all(&path) {
            self.set_warning(t!("gui.message.folder_missing", path = path.display(), detail = e));
            return;
        }
        match Current::open_folder(&path) {
            Ok(()) => self.set_status(t!("gui.message.folder_opened", path = path.display())),
            Err(e) => self.set_warning(t!("gui.message.folder_open_failed", detail = e)),
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
                self.set_status(t!("gui.message.profile_notice_hint", name = &name));
            }
            None => {}
        }
    }
}

/// Converts the insertion marker of a drag into the target index.
///
/// `before` is the place in the **unchanged** list that the pak should go
/// in front of. Removing it at `from` shifts everything behind it forward
/// by one, hence the correction. Dropping on its own position (in front of
/// or behind itself) yields `None` — there is nothing to do then, and
/// without that check we would post a status message about a move that
/// never happened.
fn drop_target(from: usize, before: usize) -> Option<usize> {
    let to = if from < before { before.checked_sub(1)? } else { before };
    (to != from).then_some(to)
}

/// Writes the chosen language into the settings and switches the running
/// program over. Separate from the dialog so that it can be tested
/// without a window.
fn apply_language(settings: &mut Settings, language: Language) {
    settings.language = Some(language.code().to_string());
    sm2_core::i18n::set_language(language);
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

        self.toasts.prune(ctx.input(|input| input.time));
        toasts::show(self, &ctx, &mut actions);
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
    /// Draws in the dividers between the panels.
    ///
    /// `show_separator_line(false)` switches off `egui`'s own lines,
    /// because they take their colour from `Visuals` and bring a shadow
    /// along with them; the design calls for a stroke exactly 1 px wide in
    /// `BORDER_SOFT`.
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

    /// Imports files the user has dragged onto the window — the design
    /// names this explicitly in the row above the list.
    fn handle_dropped_files(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect()
        });
        if dropped.is_empty() {
            return;
        }
        if !self.can_modify() {
            self.set_warning(self.blocked_reason(&t!("gui.message.action_import")));
            return;
        }
        self.section = Section::Mods;
        self.begin_import(dropped);
        let _ = actions;
    }

    /// Alt + arrow key moves the selected mod — the keyboard alternative to
    /// dragging that the design provides for in the row above the list.
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


/// A heading and its explanatory text above a card — profiles and savegames
/// use the same shape.
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

/// The surface of a card: fill, 1 px border, 12 px corner radius.
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

/// The column header of a table: letter-spaced capitals on a darker ground,
/// closed off at the bottom by a divider.
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

/// Text in the middle of an empty table.
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
    use crate::app_state::language_test_lock;
    use sm2_core::i18n::{set_language, Language};

    /// `LaunchChoice::label()` is one of the two literals Task 6 left for
    /// this task on purpose (see the module doc comment) — it must actually
    /// come from the catalogue in both languages, not just compile.
    #[test]
    fn launch_choice_labels_come_from_the_catalogue_in_both_languages() {
        let _held = language_test_lock();
        set_language(Language::German);
        assert_eq!(LaunchChoice::Steam.label(), "Mit Mods (über Steam)");
        assert_eq!(LaunchChoice::Vanilla.label(), "Ohne Mods (Vanilla)");
        assert_eq!(LaunchChoice::NoEac.label(), "Ohne EAC (kein Multiplayer)");
        set_language(Language::English);
        assert_eq!(LaunchChoice::Steam.label(), "With mods (via Steam)");
        assert_eq!(LaunchChoice::Vanilla.label(), "Without mods (Vanilla)");
        assert_eq!(LaunchChoice::NoEac.label(), "Without EAC (no multiplayer)");
    }

    /// `blocked_reason` builds its sentence around a `{what}` placeholder
    /// filled in by the caller (see `apply_profile`/`toggle_mod`/`move_mod`)
    /// — a wrong placeholder name would show up as a literal `{what}` on
    /// screen, which only a rendered-text test like this one catches.
    #[test]
    fn blocked_reason_texts_substitute_the_action_word_in_both_languages() {
        let _held = language_test_lock();
        set_language(Language::English);
        assert_eq!(
            sm2_core::t!("gui.message.blocked_no_game_dir", what = "Apply"),
            "Apply not possible – game directory unknown."
        );
        assert_eq!(
            sm2_core::t!("gui.message.blocked_task_running", what = "Move"),
            "Move not possible while a task is running."
        );
        assert_eq!(
            sm2_core::t!("gui.message.blocked_read_only", what = "Import"),
            "Import not possible – mods directory is read-only."
        );
        set_language(Language::German);
        assert_eq!(
            sm2_core::t!("gui.message.blocked_no_game_dir", what = "Anwenden"),
            "Anwenden nicht möglich – Spielverzeichnis unbekannt."
        );
        set_language(Language::English);
    }

    /// `toggle_mod` fills `{verb}` with either `state_enabled` or
    /// `state_disabled` — checks both combinations render as one sentence,
    /// not as a literal placeholder.
    #[test]
    fn mod_toggled_message_substitutes_the_state_word() {
        let _held = language_test_lock();
        set_language(Language::English);
        assert_eq!(
            sm2_core::t!("gui.message.mod_toggled", name = "Some Mod", verb = "enabled"),
            "Some Mod enabled – written to pak_config.yaml."
        );
        set_language(Language::German);
        assert_eq!(
            sm2_core::t!("gui.message.mod_toggled", name = "Some Mod", verb = "deaktiviert"),
            "Some Mod deaktiviert – in pak_config.yaml geschrieben."
        );
        set_language(Language::English);
    }

    /// The default status shown before anything has happened — one of the
    /// two literals Task 6 left for this task (see the module doc comment).
    #[test]
    fn the_ready_status_comes_from_the_catalogue() {
        let _held = language_test_lock();
        set_language(Language::German);
        assert_eq!(sm2_core::t!("gui.message.ready"), "Bereit.");
        set_language(Language::English);
        assert_eq!(sm2_core::t!("gui.message.ready"), "Ready.");
    }

    #[test]
    fn dragging_down_lands_exactly_in_front_of_the_insertion_marker() {
        // Dragged from position 0 to in front of position 3: after the
        // removal everything behind it moves up by one, so the target is 2.
        assert_eq!(drop_target(0, 3), Some(2));
    }

    #[test]
    fn dragging_up_needs_no_correction() {
        assert_eq!(drop_target(4, 1), Some(1));
    }

    #[test]
    fn dropping_on_its_own_position_changes_nothing() {
        assert_eq!(drop_target(2, 2), None, "marker directly above its own row");
        assert_eq!(drop_target(2, 3), None, "marker directly below its own row");
    }

    #[test]
    fn dragging_to_the_end_hits_the_last_position() {
        // Seven entries, the first pak going to the end: the marker sits
        // at 7.
        assert_eq!(drop_target(0, 7), Some(6));
    }

    /// The picker's job in one line: remember the choice and switch the
    /// program over. Everything else about it is painting.
    #[test]
    fn confirming_a_language_stores_the_code_and_switches_over() {
        let _held = language_test_lock();
        let mut settings = Settings::default();

        apply_language(&mut settings, Language::German);

        assert_eq!(settings.language.as_deref(), Some("de"));
        assert_eq!(sm2_core::i18n::language(), Language::German);
        sm2_core::i18n::set_language(Language::English);
    }
}

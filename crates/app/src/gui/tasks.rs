//! Background jobs: import, backup, verify, restore.
//!
//! A pak is several gigabytes in size; unpacking, copying and hashing take
//! noticeable time. Running that on the paint thread would freeze the
//! window — and the design explicitly demands the opposite ("Import läuft
//! — das Fenster bleibt bedienbar"). Every job therefore runs on a thread
//! of its own and reports its progress back through a shared counter.
//!
//! The thread works on **copies** of the library and the configuration and
//! hands them back when it is done; only the paint thread writes them to
//! disk, via `AppState::persist`. That is why `App::can_modify` locks
//! activation, ordering and import while a job is running: a change made in
//! the meantime would be overwritten by the state coming back.

use super::{App, Notice, Section};
use sm2_core::library::Library;
use sm2_core::pak_config::PakConfig;
use sm2_core::paths::GamePaths;
use sm2_core::saves::{self, BackupEntry};
use sm2_core::import;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, PoisonError};

/// Progress of a running job, the way the status bar shows it.
#[derive(Debug, Clone, Default)]
pub struct Progress {
    /// 0.0 to 1.0, for the bar.
    pub fraction: f32,
    /// German description of the step currently running.
    pub label: String,
}

/// A running job.
pub struct Running {
    cancellable: bool,
    cancel: Arc<AtomicBool>,
    progress: Arc<Mutex<Progress>>,
    outcome: mpsc::Receiver<Outcome>,
}

impl Running {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Can this job be cancelled? Only the import can — it stops between
    /// two files. Aborting a backup or a restore mid-write would leave
    /// half a state behind.
    pub fn is_cancellable(&self) -> bool {
        self.cancellable
    }

    pub fn progress(&self) -> Progress {
        self.progress.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

/// The result of a job, in a form the paint thread can process.
enum Outcome {
    /// Import: the changed state plus one message per file. `error` is set
    /// when a file failed — the paks imported successfully before it are
    /// still in `library`/`config` and must not be lost (the same promise
    /// the command line's `install` command makes).
    Imported {
        library: Box<Library>,
        config: PakConfig,
        messages: Vec<String>,
        imported: usize,
        error: Option<String>,
        cancelled: bool,
    },
    BackedUp(Result<BackupEntry, String>),
    /// Import of a backup from another launcher — a plain ZIP without
    /// a manifest of ours next to it.
    ImportedBackup(Result<BackupEntry, String>),
    Verified { created_at: String, result: Result<(), String> },
    Restored { created_at: String, result: Result<BackupEntry, String> },
}

/// Starts a thread and returns the handle to it.
fn spawn<F>(ctx: egui::Context, cancellable: bool, work: F) -> Running
where
    F: FnOnce(&Arc<AtomicBool>, &Arc<Mutex<Progress>>) -> Outcome + Send + 'static,
{
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(Mutex::new(Progress::default()));
    let (sender, outcome) = mpsc::channel();

    let worker_cancel = Arc::clone(&cancel);
    let worker_progress = Arc::clone(&progress);
    std::thread::spawn(move || {
        let result = work(&worker_cancel, &worker_progress);
        // The receiver may be gone if the window has been closed in the
        // meantime — that is not an error, just a result nobody picks up
        // any more.
        let _ = sender.send(result);
        ctx.request_repaint();
    });

    Running { cancellable, cancel, progress, outcome }
}

fn report(progress: &Arc<Mutex<Progress>>, fraction: f32, label: impl Into<String>) {
    let mut guard = progress.lock().unwrap_or_else(PoisonError::into_inner);
    guard.fraction = fraction.clamp(0.0, 1.0);
    guard.label = label.into();
}

impl App {
    /// Picks up a finished result if there is one, and keeps the display
    /// moving while there is not.
    pub(super) fn poll_task(&mut self, ctx: &egui::Context) {
        let Some(task) = &self.task else { return };
        let outcome = match task.outcome.try_recv() {
            Ok(outcome) => outcome,
            Err(mpsc::TryRecvError::Empty) => {
                // The bar has to keep moving even when nobody touches the
                // mouse.
                ctx.request_repaint_after(std::time::Duration::from_millis(80));
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.task = None;
                self.set_warning("Der Vorgang wurde unerwartet beendet.");
                return;
            }
        };
        self.task = None;
        self.finish(outcome);
    }

    fn finish(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Imported { library, config, messages, imported, error, cancelled } => {
                if let Some(state) = &mut self.state {
                    state.library = *library;
                    state.config = config;
                }
                let saved = self.persist();
                for message in messages {
                    self.notices.push(Notice::info(message));
                }
                self.refresh_profiles();
                match (error, cancelled) {
                    (Some(error), _) => self.set_warning(format!("Import abgebrochen – {error}")),
                    (None, true) => self.set_status(format!(
                        "Import abgebrochen – {imported} bereits übernommene Mods bleiben eingetragen."
                    )),
                    (None, false) if !saved => {}
                    (None, false) => self.set_status(format!(
                        "{imported} Mods importiert – deaktiviert, am Ende der Ladereihenfolge."
                    )),
                }
            }
            Outcome::BackedUp(Ok(entry)) => {
                self.refresh_backups();
                self.set_status(format!("Backup angelegt: {}", entry.created_at));
            }
            Outcome::BackedUp(Err(error)) => {
                self.set_warning(format!("Backup fehlgeschlagen – {error}"));
            }
            Outcome::ImportedBackup(Ok(entry)) => {
                self.refresh_backups();
                self.set_status(format!(
                    "Backup importiert: {} – als „{}“ in der Liste.",
                    entry.created_at,
                    entry.label.as_deref().unwrap_or("ohne Etikett")
                ));
            }
            Outcome::ImportedBackup(Err(error)) => {
                self.set_warning(format!("Import fehlgeschlagen – {error}"));
            }
            Outcome::Verified { created_at, result } => match result {
                Ok(()) => {
                    self.verified.insert(created_at.clone());
                    self.set_status(format!(
                        "Backup {created_at} geprüft – alle Hashes stimmen mit dem Manifest überein."
                    ));
                }
                Err(error) => {
                    self.verified.remove(&created_at);
                    self.set_warning(format!("Backup {created_at} ist nicht in Ordnung – {error}"));
                }
            },
            Outcome::Restored { created_at, result } => match result {
                Ok(safety) => {
                    self.refresh_backups();
                    self.set_status(format!(
                        "Wiederhergestellt: {created_at} – vorheriger Stand automatisch als \
                         „{}“ gesichert.",
                        safety.label.as_deref().unwrap_or("vor Wiederherstellung")
                    ));
                }
                Err(error) => {
                    self.refresh_backups();
                    self.set_warning(format!("Wiederherstellung fehlgeschlagen – {error}"));
                }
            },
        }
    }

    /// Opens the file dialog and starts importing the chosen files.
    pub(super) fn start_import(&mut self) {
        if !self.can_modify() {
            self.set_warning(self.blocked_reason("Import"));
            return;
        }
        let files = rfd::FileDialog::new()
            .set_title("Mods importieren")
            .add_filter("Mods und Archive", &["pak", "zip", "7z", "rar"])
            .add_filter("Alle Dateien", &["*"])
            .pick_files();
        let Some(files) = files else { return };
        if files.is_empty() {
            return;
        }
        self.begin_import(files);
    }

    /// Starts the import without a file dialog — for files the user has
    /// dragged onto the window.
    pub(super) fn begin_import(&mut self, files: Vec<PathBuf>) {
        let Some(state) = &self.state else { return };
        let paths = state.paths.clone();
        let library = state.library.clone();
        let config = state.config.clone();
        let ctx = self.egui_ctx.clone();

        self.section = Section::Mods;
        self.set_status(format!("Import läuft – {} Datei(en).", files.len()));
        self.task = Some(spawn(ctx, true, move |cancel, progress| {
            import_files(&paths, library, config, &files, cancel, progress)
        }));
    }

    pub(super) fn start_backup(&mut self) {
        if self.saves_blocked.is_some() {
            self.set_warning("Kein Backup möglich – Steam-Nutzerprofil nicht gewählt.");
            return;
        }
        let Some(state) = &self.state else { return };
        let save_dir = match state.paths.save_dir(state.settings.steam_user.as_deref()) {
            Ok(dir) => dir,
            Err(e) => {
                self.set_warning(format!("Kein Backup möglich – {e}"));
                return;
            }
        };
        let Some(backups) = self.backups_dir() else { return };
        let label = self.backup_label.trim().to_owned();
        let label = (!label.is_empty()).then_some(label);
        let ctx = self.egui_ctx.clone();

        self.backup_label.clear();
        self.set_status("Backup wird angelegt …");
        self.task = Some(spawn(ctx, false, move |_cancel, progress| {
            report(progress, 0.3, String::from("Spielstände werden gelesen und gehasht"));
            let result = saves::backup(&save_dir, &backups, label.as_deref())
                .map_err(|e| e.to_string());
            Outcome::BackedUp(result)
        }));
    }

    /// Imports a backup from another launcher. Unlike `start_backup` this
    /// needs no save directory: nothing is read from the Proton prefix and
    /// nothing written to it, so the import also works before the game has
    /// ever been started on this machine.
    pub(super) fn start_backup_import(&mut self) {
        let Some(backups) = self.backups_dir() else {
            self.set_warning("Import nicht möglich – Datenverzeichnis unbekannt.");
            return;
        };
        let Some(archive) = rfd::FileDialog::new()
            .set_title("Backup importieren")
            .add_filter("ZIP-Archive", &["zip"])
            .add_filter("Alle Dateien", &["*"])
            .pick_file()
        else {
            return;
        };
        let label = self.backup_label.trim().to_owned();
        let label = (!label.is_empty()).then_some(label);
        let ctx = self.egui_ctx.clone();

        self.backup_label.clear();
        self.set_status("Backup wird importiert …");
        self.task = Some(spawn(ctx, false, move |_cancel, progress| {
            report(progress, 0.4, String::from("Archiv wird geprüft und entpackt"));
            let result = saves::import_archive(&archive, &backups, label.as_deref())
                .map_err(|e| e.to_string());
            Outcome::ImportedBackup(result)
        }));
    }

    pub(super) fn start_verify(&mut self, index: usize) {
        if self.saves_blocked.is_some() {
            self.set_warning("Prüfen nicht möglich – Steam-Nutzerprofil nicht gewählt.");
            return;
        }
        let Some(entry) = self.backups.get(index).cloned() else { return };
        let created_at = entry.created_at.clone();
        let ctx = self.egui_ctx.clone();

        self.set_status(format!("Backup {created_at} wird geprüft …"));
        self.task = Some(spawn(ctx, false, move |_cancel, progress| {
            report(progress, 0.5, format!("Hashes von {} werden nachgerechnet", entry.created_at));
            let result = saves::verify(&entry).map_err(|e| e.to_string());
            Outcome::Verified { created_at: entry.created_at.clone(), result }
        }));
    }

    /// Plays a backup back in. The only caller is the restore dialog — it
    /// has already shown the warning about Steam's cloud sync.
    pub(super) fn start_restore(&mut self, index: usize) {
        let Some(entry) = self.backups.get(index).cloned() else { return };
        let Some(state) = &self.state else { return };
        let save_dir = match state.paths.save_dir(state.settings.steam_user.as_deref()) {
            Ok(dir) => dir,
            Err(e) => {
                self.set_warning(format!("Wiederherstellen nicht möglich – {e}"));
                return;
            }
        };
        let Some(backups) = self.backups_dir() else { return };
        let created_at = entry.created_at.clone();
        let ctx = self.egui_ctx.clone();

        self.set_status(format!("Backup {created_at} wird zurückgespielt …"));
        self.task = Some(spawn(ctx, false, move |_cancel, progress| {
            report(progress, 0.4, String::from("Vorheriger Stand wird zuerst gesichert"));
            let result = saves::restore(&entry, &save_dir, &backups).map_err(|e| e.to_string());
            Outcome::Restored { created_at: entry.created_at.clone(), result }
        }));
    }
}

/// The import itself, on the background thread.
///
/// Follows `cli.rs`'s `run_install` step by step, including the working
/// copy for a `.pak` handed over on its own: for that one `extract_paks`
/// returns the original path, and `import_pak` moves the file — without a
/// copy it would vanish from the directory the user put it in.
fn import_files(
    paths: &GamePaths,
    mut library: Library,
    mut config: PakConfig,
    files: &[PathBuf],
    cancel: &Arc<AtomicBool>,
    progress: &Arc<Mutex<Progress>>,
) -> Outcome {
    let mut messages = Vec::new();
    let mut imported = 0;

    let temp = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(e) => {
            return Outcome::Imported {
                library: Box::new(library),
                config,
                messages,
                imported,
                error: Some(format!("temporäres Verzeichnis nicht anlegbar – {e}")),
                cancelled: false,
            }
        }
    };

    let total = files.len() as f32;
    for (index, file) in files.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Outcome::Imported {
                library: Box::new(library),
                config,
                messages,
                imported,
                error: None,
                cancelled: true,
            };
        }

        let shown = file.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        report(
            progress,
            index as f32 / total,
            format!("Entpacken: {shown} — Datei {} von {}", index + 1, files.len()),
        );

        let extracted = match import::extract_paks(file, temp.path()) {
            Ok(paks) => paks,
            Err(e) => {
                return Outcome::Imported {
                    library: Box::new(library),
                    config,
                    messages,
                    imported,
                    error: Some(format!("{shown} konnte nicht gelesen werden – {e}")),
                    cancelled: false,
                }
            }
        };

        for (position, pak) in extracted.iter().enumerate() {
            report(
                progress,
                (index as f32 + position as f32 / extracted.len().max(1) as f32) / total,
                format!(
                    "Übernehmen: {shown} — Pak {} von {}",
                    position + 1,
                    extracted.len()
                ),
            );

            let working_copy = match working_copy_of(pak, temp.path()) {
                Ok(path) => path,
                Err(e) => {
                    return Outcome::Imported {
                        library: Box::new(library),
                        config,
                        messages,
                        imported,
                        error: Some(e),
                        cancelled: false,
                    }
                }
            };

            let source = file.display().to_string();
            match import::import_pak(paths, &mut library, &mut config, &working_copy, Some(&source))
            {
                Ok(outcome) => match outcome.duplicate_of {
                    Some(existing) => messages.push(format!(
                        "{} ist inhaltsgleich mit {existing} und wurde übersprungen.",
                        outcome.pak
                    )),
                    None => {
                        imported += 1;
                        messages.push(format!(
                            "{} importiert – deaktiviert, am Ende der Ladereihenfolge.",
                            outcome.pak
                        ));
                    }
                },
                Err(e) => {
                    return Outcome::Imported {
                        library: Box::new(library),
                        config,
                        messages,
                        imported,
                        error: Some(format!(
                            "{} konnte nicht importiert werden – {e}",
                            working_copy.display()
                        )),
                        cancelled: false,
                    }
                }
            }
        }
    }

    report(progress, 1.0, String::from("fertig"));
    Outcome::Imported {
        library: Box::new(library),
        config,
        messages,
        imported,
        error: None,
        cancelled: false,
    }
}

/// Returns a path below `temp` that `import_pak` may move the file away
/// from. If `pak` already lies there (unpacked from an archive), nothing is
/// copied.
fn working_copy_of(pak: &Path, temp: &Path) -> Result<PathBuf, String> {
    if pak.starts_with(temp) {
        return Ok(pak.to_path_buf());
    }
    let target = unique_copy_target(temp, pak);
    std::fs::copy(pak, &target)
        .map_err(|e| format!("{} konnte nicht gelesen werden – {e}", pak.display()))?;
    Ok(target)
}

/// A free name for a working copy — the same job as the function of the
/// same name in `cli.rs`, here for the background thread.
fn unique_copy_target(temp: &Path, source: &Path) -> PathBuf {
    let name = source.file_name().unwrap_or_default();
    let candidate = temp.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let stem = source.file_stem().and_then(|s| s.to_str()).unwrap_or("mod");
    let extension = source.extension().and_then(|s| s.to_str()).unwrap_or("pak");
    let mut n = 2;
    loop {
        let candidate = temp.join(format!("{stem}_{n}.{extension}"));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pak_from_the_archive_is_not_copied_a_second_time() {
        let temp = tempfile::tempdir().unwrap();
        let extracted = temp.path().join("mod.pak");
        std::fs::write(&extracted, b"inhalt").unwrap();

        let working = working_copy_of(&extracted, temp.path()).unwrap();

        assert_eq!(working, extracted, "an already extracted file needs no copy");
    }

    #[test]
    fn an_individually_selected_pak_stays_where_it_is() {
        let temp = tempfile::tempdir().unwrap();
        let downloads = tempfile::tempdir().unwrap();
        let original = downloads.path().join("mod.pak");
        std::fs::write(&original, b"inhalt").unwrap();

        let working = working_copy_of(&original, temp.path()).unwrap();

        assert!(working.starts_with(temp.path()), "the working copy must live in the temp folder");
        assert!(original.exists(), "the user's original file must not be touched");
    }

    #[test]
    fn two_files_with_the_same_name_do_not_collide() {
        let temp = tempfile::tempdir().unwrap();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        std::fs::write(first.path().join("mod.pak"), b"eins").unwrap();
        std::fs::write(second.path().join("mod.pak"), b"zwei").unwrap();

        let a = working_copy_of(&first.path().join("mod.pak"), temp.path()).unwrap();
        let b = working_copy_of(&second.path().join("mod.pak"), temp.path()).unwrap();

        assert_ne!(a, b, "the second copy must not overwrite the first");
        assert_eq!(std::fs::read(&a).unwrap(), b"eins");
        assert_eq!(std::fs::read(&b).unwrap(), b"zwei");
    }
}

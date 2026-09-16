//! Hintergrundaufträge: Import, Backup, Prüfen, Wiederherstellen.
//!
//! Ein Pak ist mehrere Gigabyte groß; Entpacken, Kopieren und Hashen dauern
//! spürbar. Liefe das im Zeichentakt, stünde das Fenster still – der Entwurf
//! verlangt ausdrücklich das Gegenteil („Import läuft — das Fenster bleibt
//! bedienbar“). Jeder Auftrag läuft deshalb in einem eigenen Thread und
//! meldet seinen Fortschritt über einen geteilten Zähler zurück.
//!
//! Der Thread arbeitet auf **Kopien** von Bibliothek und Konfiguration und
//! gibt sie am Ende zurück; erst der Zeichen-Thread schreibt sie über
//! `AppState::persist` auf die Platte. Deshalb sperrt `App::can_modify`
//! Aktivierung, Reihenfolge und Import, solange ein Auftrag läuft: eine
//! zwischenzeitliche Änderung würde vom zurückgegebenen Stand überschrieben.

use super::{App, Notice, Section};
use sm2_core::library::Library;
use sm2_core::pak_config::PakConfig;
use sm2_core::paths::GamePaths;
use sm2_core::saves::{self, BackupEntry};
use sm2_core::import;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, PoisonError};

/// Fortschritt eines laufenden Auftrags, wie ihn die Statusleiste zeigt.
#[derive(Debug, Clone, Default)]
pub struct Progress {
    /// 0.0 bis 1.0 für den Balken.
    pub fraction: f32,
    /// Deutsche Beschreibung des gerade laufenden Schrittes.
    pub label: String,
}

/// Ein laufender Auftrag.
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

    /// Kann dieser Auftrag abgebrochen werden? Nur der Import – er setzt
    /// zwischen zwei Dateien ab. Ein Backup oder eine Wiederherstellung
    /// mitten im Schreiben abzubrechen hinterließe einen halben Stand.
    pub fn is_cancellable(&self) -> bool {
        self.cancellable
    }

    pub fn progress(&self) -> Progress {
        self.progress.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

/// Ergebnis eines Auftrags, so wie der Zeichen-Thread es verarbeiten kann.
enum Outcome {
    /// Import: der veränderte Stand plus die Meldungen je Datei. `error`
    /// ist gesetzt, wenn eine Datei gescheitert ist – die davor bereits
    /// erfolgreich importierten Paks stecken trotzdem in `library`/`config`
    /// und dürfen nicht verloren gehen (dieselbe Zusage wie beim
    /// `install`-Befehl der Kommandozeile).
    Imported {
        library: Box<Library>,
        config: PakConfig,
        messages: Vec<String>,
        imported: usize,
        error: Option<String>,
        cancelled: bool,
    },
    BackedUp(Result<BackupEntry, String>),
    Verified { created_at: String, result: Result<(), String> },
    Restored { created_at: String, result: Result<BackupEntry, String> },
}

/// Startet einen Thread und liefert den Griff darauf.
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
        // Der Empfänger kann weg sein, wenn das Fenster inzwischen
        // geschlossen wurde – das ist kein Fehler, nur ein Ergebnis, das
        // niemand mehr abholt.
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
    /// Nimmt ein fertiges Ergebnis entgegen, falls eines vorliegt, und hält
    /// die Anzeige währenddessen in Bewegung.
    pub(super) fn poll_task(&mut self, ctx: &egui::Context) {
        let Some(task) = &self.task else { return };
        let outcome = match task.outcome.try_recv() {
            Ok(outcome) => outcome,
            Err(mpsc::TryRecvError::Empty) => {
                // Der Balken muss sich bewegen, auch wenn niemand die Maus
                // rührt.
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

    /// Öffnet den Dateidialog und startet den Import der gewählten Dateien.
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

    /// Startet den Import ohne Dateidialog – für Dateien, die der Nutzer auf
    /// das Fenster gezogen hat.
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

    /// Spielt ein Backup zurück. Aufrufer ist ausschließlich der
    /// Wiederherstellen-Dialog – er hat die Warnung zu Steams
    /// Cloud-Synchronisation bereits gezeigt.
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

/// Der Import selbst, im Hintergrundthread.
///
/// Folgt Schritt für Schritt `cli.rs`s `run_install`, inklusive der
/// Arbeitskopie für ein einzeln angegebenes `.pak`: `extract_paks` gibt
/// dafür den Originalpfad zurück, und `import_pak` verschiebt die Datei –
/// ohne Kopie verschwände sie aus dem Verzeichnis, in das der Nutzer sie
/// gelegt hat.
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

/// Liefert einen Pfad unterhalb von `temp`, aus dem `import_pak` die Datei
/// wegbewegen darf. Liegt `pak` bereits dort (aus einem Archiv entpackt),
/// wird nichts kopiert.
fn working_copy_of(pak: &Path, temp: &Path) -> Result<PathBuf, String> {
    if pak.starts_with(temp) {
        return Ok(pak.to_path_buf());
    }
    let target = unique_copy_target(temp, pak);
    std::fs::copy(pak, &target)
        .map_err(|e| format!("{} konnte nicht gelesen werden – {e}", pak.display()))?;
    Ok(target)
}

/// Freier Name für eine Arbeitskopie – dieselbe Aufgabe wie `cli.rs`s
/// gleichnamige Funktion, hier für den Hintergrundthread.
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
    fn ein_pak_aus_dem_archiv_wird_nicht_noch_einmal_kopiert() {
        let temp = tempfile::tempdir().unwrap();
        let extracted = temp.path().join("mod.pak");
        std::fs::write(&extracted, b"inhalt").unwrap();

        let working = working_copy_of(&extracted, temp.path()).unwrap();

        assert_eq!(working, extracted, "eine bereits entpackte Datei braucht keine Kopie");
    }

    #[test]
    fn ein_einzeln_angegebenes_pak_bleibt_an_seinem_platz() {
        let temp = tempfile::tempdir().unwrap();
        let downloads = tempfile::tempdir().unwrap();
        let original = downloads.path().join("mod.pak");
        std::fs::write(&original, b"inhalt").unwrap();

        let working = working_copy_of(&original, temp.path()).unwrap();

        assert!(working.starts_with(temp.path()), "die Arbeitskopie muss im Temp-Ordner liegen");
        assert!(original.exists(), "die Originaldatei des Nutzers darf nicht angetastet werden");
    }

    #[test]
    fn zwei_gleichnamige_dateien_kollidieren_nicht() {
        let temp = tempfile::tempdir().unwrap();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        std::fs::write(first.path().join("mod.pak"), b"eins").unwrap();
        std::fs::write(second.path().join("mod.pak"), b"zwei").unwrap();

        let a = working_copy_of(&first.path().join("mod.pak"), temp.path()).unwrap();
        let b = working_copy_of(&second.path().join("mod.pak"), temp.path()).unwrap();

        assert_ne!(a, b, "die zweite Kopie darf die erste nicht überschreiben");
        assert_eq!(std::fs::read(&a).unwrap(), b"eins");
        assert_eq!(std::fs::read(&b).unwrap(), b"zwei");
    }
}

//! Command line interface: definition and execution of all subcommands.

use crate::app_state::{AppState, NoticeKind};
use crate::vanilla;
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use sm2_core::launch::{launch, no_eac_available, LaunchMode};
use sm2_core::pak_config::PakEntry;
use sm2_core::paths::GamePaths;
use sm2_core::platform::{Current, Platform};
use sm2_core::profile::{list_profiles, Profile};
use sm2_core::saves::BackupEntry;
use sm2_core::{import, saves};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "sm2-modloader", about = "Mod-Loader für Space Marine 2", version)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Zeigt alle Mods in Ladereihenfolge
    List,
    /// Aktiviert einen Mod
    Enable { pak: String },
    /// Deaktiviert einen Mod
    Disable { pak: String },
    /// Setzt die Ladereihenfolge; nicht genannte Mods behalten ihre Position dahinter
    Order { paks: Vec<String> },
    /// Importiert Mods aus .pak, .zip, .7z oder .rar
    Install { files: Vec<PathBuf> },
    /// Zeigt die erkannten Verzeichnisse
    Paths,
    /// Öffnet ein Verzeichnis im Dateimanager
    Open {
        #[arg(value_enum)]
        target: OpenTarget,
    },
    /// Profile verwalten
    #[command(subcommand)]
    Profile(ProfileCommand),
    /// Savegames sichern und wiederherstellen
    #[command(subcommand)]
    Save(SaveCommand),
    /// Startet das Spiel
    Play {
        /// Alle Mods deaktivieren
        #[arg(long)]
        vanilla: bool,
        /// Ohne EAC starten (kein Multiplayer)
        #[arg(long)]
        no_eac: bool,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum OpenTarget {
    /// Spielverzeichnis
    Game,
    /// Mods-Verzeichnis
    Mods,
    /// Savegame-Verzeichnis im Proton-Prefix
    Saves,
    /// Backup-Verzeichnis des Loaders
    Backups,
}

#[derive(Subcommand)]
enum ProfileCommand {
    List,
    /// Speichert den aktuellen Zustand als Profil
    Save { name: String },
    /// Wendet ein Profil an
    Apply { name: String },
    /// Löscht ein Profil
    Delete { name: String },
}

#[derive(Subcommand)]
enum SaveCommand {
    /// Legt ein Backup an
    Backup {
        #[arg(long)]
        tag: Option<String>,
    },
    List,
    /// Stellt ein Backup wieder her (Standard: das neueste)
    Restore {
        /// 1-basierter Index aus `save list` (Standard: 1, das neueste)
        #[arg(long)]
        index: Option<usize>,
        // `--index` shifts with every restore because `restore` itself
        // creates a new "before restore" backup (see `run_save_command`'s
        // doc comment). `--at`, by contrast, stays unambiguous no matter
        // how often you restore.
        /// Exakter Zeitstempel aus `save list` – eindeutig, verschiebt sich
        /// anders als `--index` nicht durch spätere Wiederherstellungen
        #[arg(long, conflicts_with = "index")]
        at: Option<String>,
        // Spec §6.5/§9 R2: cloud synchronization can overwrite the restored
        // state in the background. Refusing is therefore the default,
        // `--force` is the deliberate exception.
        /// Erzwingt die Wiederherstellung trotz laufendem Steam (Risiko:
        /// Cloud-Synchronisation kann den Stand überschreiben)
        #[arg(long)]
        force: bool,
    },
    /// Ändert das Etikett eines Backups (Standard: das neueste)
    Rename {
        /// 1-basierter Index aus `save list` (Standard: 1, das neueste)
        #[arg(long)]
        index: Option<usize>,
        /// Exakter Zeitstempel aus `save list`
        #[arg(long, conflicts_with = "index")]
        at: Option<String>,
        /// Neues Etikett; ohne Angabe wird das Etikett entfernt
        #[arg(long)]
        tag: Option<String>,
    },
    /// Löscht ein Backup endgültig (Standard: das neueste)
    Delete {
        /// 1-basierter Index aus `save list` (Standard: 1, das neueste)
        #[arg(long)]
        index: Option<usize>,
        /// Exakter Zeitstempel aus `save list`
        #[arg(long, conflicts_with = "index")]
        at: Option<String>,
        // A deleted backup is gone for good. Unlike `restore`, there is no
        // backup here that could undo the step. That is why even the
        // non-interactive path requires an explicit confirmation.
        /// Bestätigt das endgültige Löschen (erforderlich)
        #[arg(long)]
        yes: bool,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let mut state = AppState::open()?;
    // Notices from the reconciliation first, so they come before the actual
    // command's own output — just as before, when `AppState::open()` still
    // wrote them to stderr itself.
    print_notices(&mut state);

    let result = run_command(&mut state, cli.command);
    // And once more afterwards: `persist()` may have appended more notices
    // along the way that nobody would otherwise get to see.
    print_notices(&mut state);
    result
}

/// Writes all accumulated notices to stderr and clears the buffer.
fn print_notices(state: &mut AppState) {
    for notice in state.take_notices() {
        let prefix = match notice.kind {
            NoticeKind::Info => "Hinweis",
            NoticeKind::Warning => "Warnung",
            NoticeKind::Error => "Fehlt",
        };
        eprintln!("{prefix}: {}", notice.text);
    }
}

fn run_command(state: &mut AppState, command: Command) -> Result<()> {
    match command {
        Command::List => {
            if state.config.entries.is_empty() {
                println!("Keine Mods installiert.");
            }
            for (i, entry) in state.config.entries.iter().enumerate() {
                let marker = if entry.disabled { "○" } else { "●" };
                let name = state
                    .library
                    .mods
                    .get(&entry.pak)
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| entry.pak.clone());
                println!("{:>2}. {marker} {name}  ({})", i + 1, entry.pak);
            }
        }

        Command::Enable { pak } => {
            set_disabled(state, &pak, false)?;
            state.persist()?;
            println!("✓ {pak} aktiviert");
        }

        Command::Disable { pak } => {
            set_disabled(state, &pak, true)?;
            state.persist()?;
            println!("✓ {pak} deaktiviert");
        }

        Command::Order { paks } => run_order(state, paks)?,

        Command::Install { files } => run_install(state, &files)?,

        Command::Paths => {
            println!("Spiel:   {}", state.paths.game_dir.display());
            println!("Mods:    {}", state.paths.mods_dir().display());
            println!("Config:  {}", state.paths.pak_config_path().display());
            match state.save_dir() {
                Ok(p) => println!("Saves:   {}", p.display()),
                Err(e) => println!("Saves:   nicht verfügbar – {e}"),
            }
            println!("Backups: {}", state.backups_dir().display());
        }

        Command::Open { target } => {
            let path = match target {
                OpenTarget::Game => state.paths.game_dir.clone(),
                OpenTarget::Mods => state.paths.mods_dir(),
                OpenTarget::Saves => state.save_dir()?,
                OpenTarget::Backups => {
                    let dir = state.backups_dir();
                    std::fs::create_dir_all(&dir)
                        .with_context(|| format!("{} konnte nicht angelegt werden", dir.display()))?;
                    dir
                }
            };
            Current::open_folder(&path)?;
        }

        Command::Profile(cmd) => run_profile_command(state, cmd)?,
        Command::Save(cmd) => run_save_command(state, cmd)?,

        Command::Play { vanilla, no_eac } => run_play(state, vanilla, no_eac)?,
    }

    Ok(())
}

fn set_disabled(state: &mut AppState, pak: &str, disabled: bool) -> Result<()> {
    let entry = state
        .config
        .entries
        .iter_mut()
        .find(|e| e.pak == pak)
        .with_context(|| format!("{pak} ist nicht installiert"))?;
    entry.disabled = disabled;
    Ok(())
}

/// Sets a new load order. Named paks must be installed and must not be
/// named twice; paks that are not named keep their relative order and
/// are appended at the end.
fn run_order(state: &mut AppState, requested: Vec<String>) -> Result<()> {
    if requested.is_empty() {
        println!("Keine Paks angegeben.");
        return Ok(());
    }

    let mut seen = HashSet::new();
    for name in &requested {
        if !seen.insert(name.as_str()) {
            bail!("{name} wurde mehrfach genannt");
        }
    }

    let mut new_entries: Vec<PakEntry> = Vec::with_capacity(state.config.entries.len());
    for name in &requested {
        let Some(pos) = state.config.entries.iter().position(|e| &e.pak == name) else {
            bail!("{name} ist nicht installiert");
        };
        new_entries.push(state.config.entries.remove(pos));
    }
    new_entries.append(&mut state.config.entries);
    state.config.entries = new_entries;
    state.persist()?;
    println!("✓ Ladereihenfolge gesetzt");
    Ok(())
}

/// Imports every file given. Each successfully imported pak is saved
/// right away, not only after all files are done: if a later pak fails
/// — in the same file or in another one — the mods already imported stay
/// recorded permanently in `library.json` and `pak_config.yaml`,
/// instead of merely sitting in the mods directory without being noted
/// anywhere.
fn run_install(state: &mut AppState, files: &[PathBuf]) -> Result<()> {
    if files.is_empty() {
        println!("Keine Dateien angegeben.");
        return Ok(());
    }

    // Must live as long as the import calls that read from it: for archives
    // the extracted paks sit below this directory until they are imported.
    let tmp = tempfile::tempdir().context("temporäres Verzeichnis konnte nicht angelegt werden")?;

    for file in files {
        let extracted = import::extract_paks(file, tmp.path())
            .with_context(|| format!("{} konnte nicht gelesen werden", file.display()))?;

        for pak in &extracted {
            // For a single `.pak` as input, `extract_paks` returns the
            // original path unchanged (nothing was copied into `tmp`). A
            // direct import would then make exactly that file disappear
            // from the folder the user put it in (their download directory,
            // say), via `import_pak`'s rename step. For archives this does
            // not apply: their extracted paks already sit under `tmp`, and
            // the archive itself is left untouched.
            let working_copy = if pak.starts_with(tmp.path()) {
                pak.clone()
            } else {
                let target = unique_copy_target(tmp.path(), pak);
                std::fs::copy(pak, &target)
                    .with_context(|| format!("{} konnte nicht gelesen werden", pak.display()))?;
                target
            };

            let source = file.display().to_string();
            let outcome = import::import_pak(
                &state.paths,
                &mut state.library,
                &mut state.config,
                &working_copy,
                Some(&source),
            )
            .with_context(|| format!("{} konnte nicht importiert werden", working_copy.display()))?;

            match &outcome.duplicate_of {
                Some(existing) => {
                    println!("– {} ist inhaltsgleich mit {existing}, übersprungen", outcome.pak);
                }
                None => {
                    state.persist()?;
                    println!("✓ {} importiert (deaktiviert)", outcome.pak);
                }
            }
        }
    }
    Ok(())
}

/// Finds a name in `tmp` that is still free for a working copy of
/// `source`. If the original file name already collides — for example
/// because two of the given files share the same base name — a running
/// number is inserted before the extension. Same basic idea as
/// `sm2_core::import`'s internal `unique_target_in`, reproduced locally
/// here because that function stays crate-private.
fn unique_copy_target(tmp: &std::path::Path, source: &std::path::Path) -> PathBuf {
    let name = source.file_name().unwrap_or_default();
    let candidate = tmp.join(name);
    if !candidate.exists() {
        return candidate;
    }

    let stem = source.file_stem().and_then(|s| s.to_str()).unwrap_or("mod");
    let extension = source.extension().and_then(|s| s.to_str()).unwrap_or("pak");
    let mut n = 2;
    loop {
        let candidate = tmp.join(format!("{stem}_{n}.{extension}"));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

fn run_profile_command(state: &mut AppState, cmd: ProfileCommand) -> Result<()> {
    let dir = state.profiles_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("{} konnte nicht angelegt werden", dir.display()))?;

    match cmd {
        ProfileCommand::List => {
            let profiles = list_profiles(&dir)?;
            if profiles.is_empty() {
                println!("Keine Profile gespeichert.");
            }
            for p in profiles {
                let active = p.entries.iter().filter(|e| !e.disabled).count();
                println!("{}  ({active} aktiv von {})", p.name, p.entries.len());
            }
        }
        ProfileCommand::Save { name } => {
            let profile = Profile::from_config(&name, &state.config);
            let path = profile.save(&dir)?;
            println!("✓ Profil '{name}' gespeichert: {}", path.display());
        }
        ProfileCommand::Apply { name } => {
            let profile = find_profile_by_name(&dir, &name)?;

            let (new_config, missing) = profile.apply(&state.paths.list_paks()?);
            for pak in &missing {
                eprintln!("Warnung: {pak} aus dem Profil ist nicht installiert, übersprungen.");
            }
            state.config = new_config;
            state.persist()?;
            println!("✓ Profil '{}' angewendet", profile.name);
        }
        ProfileCommand::Delete { name } => {
            let profile = find_profile_by_name(&dir, &name)?;
            let path = profile.path_in(&dir);
            std::fs::remove_file(&path)
                .with_context(|| format!("{} konnte nicht gelöscht werden", path.display()))?;
            println!("✓ Profil '{}' gelöscht ({})", profile.name, path.display());
        }
    }
    Ok(())
}

/// Finds exactly one profile named `name` under `dir`, case-insensitively
/// (a convenience: the user does not have to type the name exactly).
/// `Profile::file_stem`, by contrast, hashes the exact name — two
/// profiles like "Test" and "test" therefore end up in two different
/// files. Without this check, an ambiguous name would silently pick the
/// first matching profile (by `list_profiles`'s alphabetical order), and
/// the other one would be effectively unreachable through `apply`/`delete`,
/// without the user ever finding out. An ambiguous match is reported
/// instead, listing all affected names so the user can supply the exact
/// one.
fn find_profile_by_name(dir: &std::path::Path, name: &str) -> Result<Profile> {
    let mut matches: Vec<Profile> =
        list_profiles(dir)?.into_iter().filter(|p| p.name.eq_ignore_ascii_case(name)).collect();

    match matches.len() {
        0 => bail!("Profil '{name}' nicht gefunden"),
        1 => Ok(matches.remove(0)),
        _ => {
            let names: Vec<&str> = matches.iter().map(|p| p.name.as_str()).collect();
            bail!(
                "mehrere Profile passen zu '{name}' und unterscheiden sich nur in \
                 Groß-/Kleinschreibung ({}). Bitte den exakten Namen angeben.",
                names.join(", ")
            );
        }
    }
}

fn run_save_command(state: &AppState, cmd: SaveCommand) -> Result<()> {
    run_save_command_with(state, cmd, saves::steam_is_running)
}

/// Core of `run_save_command`, with Steam detection as a parameter
/// instead of hard-wired — exactly the same seam idea as
/// `run_play`/`run_play_with` further below. `--force` is the only branch
/// here that can actually overwrite save data (Steam is running, cloud
/// sync could overwrite the restored state); without this injection
/// neither the refusal nor the bypass could be tested deterministically,
/// because `saves::steam_is_running()` reads this system's real `/proc`.
fn run_save_command_with(state: &AppState, cmd: SaveCommand, steam_running: impl Fn() -> bool) -> Result<()> {
    let backups = state.backups_dir();

    match cmd {
        SaveCommand::Backup { tag } => {
            let saves_dir = state.save_dir()?;
            let entry = saves::backup(&saves_dir, &backups, tag.as_deref())?;
            println!("✓ Backup: {}", entry.archive.display());
        }
        SaveCommand::List => {
            let list = saves::list_backups(&backups)?;
            if list.is_empty() {
                println!("Keine Backups vorhanden.");
            }
            for (i, entry) in list.iter().enumerate() {
                let label = entry.label.clone().unwrap_or_default();
                println!("{:>2}. {}  {label}", i + 1, entry.created_at);
            }
        }
        SaveCommand::Restore { index, at, force } => {
            let list = saves::list_backups(&backups)?;
            if list.is_empty() {
                bail!("keine Backups vorhanden");
            }
            let entry = resolve_backup_selection(&list, index, at.as_deref())?;

            // Echo what was actually selected BEFORE anything changes:
            // `restore` takes a "before restore" backup of its own before
            // restoring, and `list_backups` is newest-first — so every
            // restore shifts every later `--index`. The user sees here what
            // `--index`/`--at` actually hit, before the operation becomes
            // irreversible (see `resolve_backup_selection`'s doc comment).
            let label = entry.label.clone().unwrap_or_default();
            println!("Ausgewähltes Backup: {}  {label}", entry.created_at);

            if steam_running() {
                if !force {
                    bail!(
                        "Steam läuft. Die Cloud-Synchronisation kann den wiederhergestellten \
                         Stand überschreiben. Bitte Steam beenden und erneut versuchen, oder \
                         mit --force auf eigenes Risiko fortfahren."
                    );
                }
                eprintln!(
                    "Warnung: Steam läuft – --force erzwingt die Wiederherstellung trotz \
                     möglicher Cloud-Synchronisation."
                );
            }

            let saves_dir = state.save_dir()?;
            let safety_backup = saves::restore(entry, &saves_dir, &backups)?;
            println!("✓ Wiederhergestellt: {}", entry.created_at);
            println!("  Vorheriger Stand gesichert: {}", safety_backup.archive.display());
        }
        SaveCommand::Rename { index, at, tag } => {
            let list = saves::list_backups(&backups)?;
            if list.is_empty() {
                bail!("keine Backups vorhanden");
            }
            let entry = resolve_backup_selection(&list, index, at.as_deref())?;
            let renamed = saves::rename(entry, &backups, tag.as_deref())?;
            println!("✓ Umbenannt: {}  {}", renamed.created_at, renamed.label.as_deref().unwrap_or("—"));
            println!("  {}", renamed.archive.display());
        }
        SaveCommand::Delete { index, at, yes } => {
            let list = saves::list_backups(&backups)?;
            if list.is_empty() {
                bail!("keine Backups vorhanden");
            }
            let entry = resolve_backup_selection(&list, index, at.as_deref())?;
            let label = entry.label.clone().unwrap_or_default();
            if !yes {
                bail!(
                    "Löschen ist unwiderruflich. Ausgewählt wäre: {}  {label}. Mit --yes \
                     bestätigen.",
                    entry.created_at
                );
            }
            saves::delete(entry)?;
            println!("✓ Gelöscht: {}  {label}", entry.created_at);
        }
    }
    Ok(())
}

/// Resolves the 1-based backup index supplied by the user (omitted: the
/// newest backup, i.e. 1) to a 0-based vector index.
///
/// 0 is not a valid index (the user counts from 1). Without this check,
/// an explicit 0 would make `index.unwrap_or(1) - 1` underflow as a
/// `usize` (panic in a debug build, wraparound in a release build).
fn resolve_backup_index(requested: Option<usize>, count: usize) -> Result<usize> {
    let requested = requested.unwrap_or(1);
    if requested == 0 {
        bail!("Index muss mindestens 1 sein (1 = neuestes Backup)");
    }
    if requested > count {
        bail!("Backup {requested} gibt es nicht ({count} vorhanden)");
    }
    Ok(requested - 1)
}

/// Selects a backup from `list` either by its exact timestamp (`at`, as
/// `save list` shows it) or by its 1-based index (`index`, default: the
/// newest). `--at` is the unambiguous choice: `restore` takes a new
/// backup of its own before every restore, and `list_backups` sorts
/// newest first — so a backup chosen by `--index` shifts by one with
/// every restore. `clap`'s `conflicts_with` already prevents both from
/// being given at once.
fn resolve_backup_selection<'a>(
    list: &'a [BackupEntry],
    index: Option<usize>,
    at: Option<&str>,
) -> Result<&'a BackupEntry> {
    match at {
        Some(timestamp) => list
            .iter()
            .find(|e| e.created_at == timestamp)
            .with_context(|| format!("kein Backup mit Zeitstempel '{timestamp}' gefunden")),
        None => {
            let position = resolve_backup_index(index, list.len())?;
            Ok(&list[position])
        }
    }
}

/// On `play --vanilla`, backs up the previous state and reports the
/// result on the command line. The rule itself lives in `crate::vanilla`
/// — it applies to the UI just the same.
fn snapshot_and_disable_all_for_vanilla_start(state: &mut AppState) -> Result<()> {
    match vanilla::snapshot_and_disable_all(state)? {
        Some(snapshot) => {
            println!(
                "Hinweis: bisheriger Zustand als Profil '{}' gesichert ({}).",
                snapshot.name,
                snapshot.path.display()
            );
            println!("  Mit `profile apply \"{}\"` wiederherstellen.", snapshot.name);
        }
        None => eprintln!("Hinweis: bereits vollständig deaktiviert – keine neue Sicherung angelegt."),
    }
    Ok(())
}

/// Starts the game.
fn run_play(state: &mut AppState, vanilla: bool, no_eac: bool) -> Result<()> {
    run_play_with(state, vanilla, no_eac, launch)
}

/// Core of `run_play`, with the actual game launch as a parameter instead
/// of hard-wired: `run_play` itself always starts the real process via
/// `sm2_core::launch::launch`, but every branch before that (vanilla
/// wipe, auto backup, `persist()` error behavior, no-EAC gating) can be
/// tested this way without ever starting a real process. The tests below
/// pass a closure instead that only records whether it was called, and
/// with which `LaunchMode`.
///
/// Failure behavior for the save backup (deliberately the same for both
/// error sources): neither a missing save directory nor a failing
/// `saves::backup` aborts the launch — both are only reported as a
/// warning. The automatic backup is a convenience feature (it can be
/// turned off via `settings.toml`), not a hard requirement. The backup
/// attempt also happens before `state.persist()` and before the launch
/// itself, so a failure never leaves a written configuration behind for
/// a game that was never started. Unlike before, this now applies to
/// `--vanilla` as well: Spec §6.4 explicitly describes the vanilla launch
/// as identical to the modded one, backup included — and a user often
/// reaches for `--vanilla` right *after* something has already gone
/// wrong, which is exactly when the backup matters most.
///
/// Failure behavior for `persist()` itself: on an ordinary (non-vanilla)
/// launch, `persist()` at most changes the result of the reconciliation
/// from `AppState::open()` — nothing the user intended with this call.
/// If it fails (a read-only mods directory, say), that is only warned
/// about; the game starts anyway, with the configuration already on
/// disk, which is what the engine reads regardless. For `--vanilla`, by
/// contrast, `persist()` stays fatal: disabling all mods is the entire
/// point of the call, and a launch with mods still active would be the
/// opposite of what the user wanted.
///
/// The vanilla backup itself (see
/// `snapshot_and_disable_all_for_vanilla_start`) is always a hard
/// requirement: if it fails, nothing is changed and nothing is started —
/// without it there would be no way back to the previous setup.
fn run_play_with(
    state: &mut AppState,
    vanilla: bool,
    no_eac: bool,
    launch_game: impl FnOnce(&GamePaths, LaunchMode) -> sm2_core::Result<()>,
) -> Result<()> {
    if no_eac && !no_eac_available() {
        bail!(
            "Start ohne EAC ist auf diesem System nicht möglich – umu-launcher wurde nicht \
             gefunden (https://github.com/Open-Wine-Components/umu-launcher)."
        );
    }

    if vanilla {
        snapshot_and_disable_all_for_vanilla_start(state)?;
    }

    if state.settings.auto_backup {
        let label = if vanilla { "vor Vanilla-Start" } else { "vor Modded-Start" };
        match state.save_dir() {
            Ok(saves_dir) => match saves::backup(&saves_dir, &state.backups_dir(), Some(label)) {
                Ok(entry) => println!("✓ Save gesichert: {}", entry.archive.display()),
                Err(e) => eprintln!("Warnung: Save-Backup fehlgeschlagen – {e}"),
            },
            Err(e) => eprintln!("Warnung: kein Save-Backup möglich – {e}"),
        }
    }

    if vanilla {
        state.persist()?;
    } else if let Err(e) = state.persist() {
        eprintln!("Warnung: Konfiguration konnte nicht gespeichert werden – {e:#}");
    }

    let mode = if no_eac { LaunchMode::NoEac } else { LaunchMode::Steam };
    if no_eac {
        eprintln!("Hinweis: Start ohne EAC – Multiplayer ist damit nicht möglich.");
    }
    launch_game(&state.paths, mode)?;
    println!("✓ Spiel gestartet");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::test_fixture;
    use clap::CommandFactory;

    #[test]
    fn cli_command_structure_is_valid() {
        // Checks the clap structure itself (name collisions, invalid
        // attributes and so on) without parsing any real arguments.
        Cli::command().debug_assert();
    }

    #[test]
    fn resolve_backup_index_defaults_to_the_newest_backup() {
        assert_eq!(resolve_backup_index(None, 3).unwrap(), 0);
    }

    #[test]
    fn resolve_backup_index_rejects_zero() {
        assert!(resolve_backup_index(Some(0), 3).is_err());
    }

    #[test]
    fn resolve_backup_index_rejects_out_of_range() {
        assert!(resolve_backup_index(Some(4), 3).is_err());
        assert!(resolve_backup_index(Some(100), 0).is_err());
    }

    #[test]
    fn resolve_backup_index_converts_one_based_to_zero_based() {
        assert_eq!(resolve_backup_index(Some(1), 3).unwrap(), 0);
        assert_eq!(resolve_backup_index(Some(3), 3).unwrap(), 2);
    }

    // --- run_order -----------------------------------------------------

    #[test]
    fn run_order_with_no_arguments_is_a_no_op() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        state.config.entries = vec![PakEntry { pak: "a.pak".into(), disabled: false }];

        run_order(&mut state, vec![]).unwrap();

        assert_eq!(state.config.entries.len(), 1);
        assert!(!state.paths.pak_config_path().exists(), "a no-op must not write anything");
    }

    #[test]
    fn run_order_rejects_a_repeated_name() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        state.config.entries = vec![
            PakEntry { pak: "a.pak".into(), disabled: false },
            PakEntry { pak: "b.pak".into(), disabled: false },
        ];

        let err = run_order(&mut state, vec!["a.pak".into(), "a.pak".into()]).unwrap_err();
        assert!(err.to_string().contains("mehrfach"), "{err}");
    }

    #[test]
    fn run_order_rejects_an_unknown_pak() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        state.config.entries = vec![PakEntry { pak: "a.pak".into(), disabled: false }];

        let err = run_order(&mut state, vec!["fehlt.pak".into()]).unwrap_err();
        assert!(err.to_string().contains("nicht installiert"), "{err}");
    }

    #[test]
    fn run_order_keeps_every_present_pak_and_applies_the_requested_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        state.config.entries = vec![
            PakEntry { pak: "a.pak".into(), disabled: false },
            PakEntry { pak: "b.pak".into(), disabled: true },
            PakEntry { pak: "c.pak".into(), disabled: false },
        ];

        run_order(&mut state, vec!["c.pak".into(), "a.pak".into()]).unwrap();

        let names: Vec<&str> = state.config.entries.iter().map(|e| e.pak.as_str()).collect();
        assert_eq!(names, vec!["c.pak", "a.pak", "b.pak"], "every present pak must be preserved");
    }

    // --- vanilla snapshot ------------------------------------------------

    #[test]
    fn vanilla_snapshot_captures_the_state_from_before_the_wipe() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        state.config.entries = vec![
            PakEntry { pak: "a.pak".into(), disabled: false },
            PakEntry { pak: "b.pak".into(), disabled: true },
        ];

        snapshot_and_disable_all_for_vanilla_start(&mut state).unwrap();

        assert!(state.config.entries.iter().all(|e| e.disabled), "afterwards everything must be disabled");

        let profiles = list_profiles(&state.profiles_dir()).unwrap();
        assert_eq!(profiles.len(), 1);
        assert!(profiles[0].name.starts_with(vanilla::VANILLA_SNAPSHOT_PREFIX));
        let a = profiles[0].entries.iter().find(|e| e.pak == "a.pak").unwrap();
        assert!(!a.disabled, "the snapshot must show the state BEFORE the disabling");
    }

    #[test]
    fn two_consecutive_vanilla_runs_keep_the_first_snapshot_intact() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        state.config.entries = vec![PakEntry { pak: "a.pak".into(), disabled: false }];

        snapshot_and_disable_all_for_vanilla_start(&mut state).unwrap();
        // Second run: everything is already disabled by now.
        snapshot_and_disable_all_for_vanilla_start(&mut state).unwrap();

        let profiles = list_profiles(&state.profiles_dir()).unwrap();
        assert_eq!(
            profiles.len(),
            1,
            "the second vanilla start must not overwrite or duplicate the first snapshot"
        );
        assert!(
            profiles[0].entries.iter().any(|e| !e.disabled),
            "the one snapshot must still show the original, enabled state"
        );
    }

    #[test]
    fn vanilla_snapshot_is_skipped_when_nothing_is_enabled() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        state.config.entries = vec![PakEntry { pak: "a.pak".into(), disabled: true }];

        snapshot_and_disable_all_for_vanilla_start(&mut state).unwrap();

        let profiles = list_profiles(&state.profiles_dir()).unwrap();
        assert!(profiles.is_empty(), "with no enabled mods there is nothing to back up");
    }

    // --- run_install -----------------------------------------------------

    #[test]
    fn run_install_does_not_remove_the_source_file_for_a_bare_pak() {
        let tmp_state = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp_state.path());

        let source_dir = tempfile::tempdir().unwrap();
        let source_pak = source_dir.path().join("mein_mod.pak");
        std::fs::write(&source_pak, b"INHALT").unwrap();

        run_install(&mut state, std::slice::from_ref(&source_pak)).unwrap();

        assert!(source_pak.is_file(), "the user's source file must not disappear");
        assert_eq!(std::fs::read(&source_pak).unwrap(), b"INHALT");
        assert!(state.library.mods.contains_key("mein_mod.pak"));
    }

    #[test]
    fn run_install_persists_earlier_successes_when_a_later_file_fails() {
        let tmp_state = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp_state.path());

        let source_dir = tempfile::tempdir().unwrap();
        let first = source_dir.path().join("erstes.pak");
        let second = source_dir.path().join("zweites.pak");
        std::fs::write(&first, b"EINS").unwrap();
        std::fs::write(&second, b"ZWEI").unwrap();
        // Deliberately does not exist: simulates the third file failing.
        let third = source_dir.path().join("drittes.pak");
        let fourth = source_dir.path().join("viertes.pak");
        std::fs::write(&fourth, b"VIER").unwrap();

        let files = vec![first, second, third, fourth];
        let err = run_install(&mut state, &files).unwrap_err();
        assert!(err.to_string().contains("drittes.pak"), "{err}");

        // The first two files are already in the mods directory...
        assert!(state.paths.mods_dir().join("erstes.pak").is_file());
        assert!(state.paths.mods_dir().join("zweites.pak").is_file());

        // ...and are recorded permanently in both library.json and
        // pak_config.yaml, not only in the (in-memory) AppState.
        let saved_library = sm2_core::library::Library::load(&state.dirs.data.join("library.json")).unwrap();
        assert!(saved_library.mods.contains_key("erstes.pak"));
        assert!(saved_library.mods.contains_key("zweites.pak"));

        let saved_config = sm2_core::pak_config::PakConfig::load(&state.paths.pak_config_path()).unwrap();
        let names: Vec<&str> = saved_config.entries.iter().map(|e| e.pak.as_str()).collect();
        assert!(names.contains(&"erstes.pak"));
        assert!(names.contains(&"zweites.pak"));
        assert!(
            !names.contains(&"viertes.pak"),
            "after the failure the fourth file must no longer have been processed"
        );
    }

    // --- resolve_backup_selection (2a: --at alongside --index) ---------

    fn backup_entry(created_at: &str, label: Option<&str>) -> BackupEntry {
        BackupEntry {
            archive: PathBuf::from(format!("{created_at}.zip")),
            manifest: PathBuf::from(format!("{created_at}.json")),
            created_at: created_at.to_string(),
            label: label.map(str::to_string),
        }
    }

    #[test]
    fn resolve_backup_selection_without_arguments_defaults_to_the_newest() {
        let list = vec![
            backup_entry("2026-01-02T00:00:00Z", None),
            backup_entry("2026-01-01T00:00:00Z", None),
        ];

        let entry = resolve_backup_selection(&list, None, None).unwrap();

        assert_eq!(entry.created_at, "2026-01-02T00:00:00Z");
    }

    /// The actual reason for `--at`: a backup chosen by `--index` shifts
    /// with every restore. An exact timestamp, by contrast, stays
    /// unambiguous no matter how many restores happened in between.
    #[test]
    fn resolve_backup_selection_by_at_finds_the_exact_timestamp() {
        let list = vec![
            backup_entry("2026-01-02T00:00:00Z", None),
            backup_entry("2026-01-01T00:00:00Z", Some("alt")),
        ];

        let entry = resolve_backup_selection(&list, None, Some("2026-01-01T00:00:00Z")).unwrap();

        assert_eq!(entry.label.as_deref(), Some("alt"));
    }

    #[test]
    fn resolve_backup_selection_by_at_reports_an_unknown_timestamp_clearly() {
        let list = vec![backup_entry("2026-01-02T00:00:00Z", None)];

        let err = resolve_backup_selection(&list, None, Some("2099-01-01T00:00:00Z")).unwrap_err();

        assert!(err.to_string().contains("2099-01-01T00:00:00Z"), "{err}");
    }

    #[test]
    fn resolve_backup_selection_by_index_still_works_alongside_at() {
        let list = vec![
            backup_entry("2026-01-02T00:00:00Z", None),
            backup_entry("2026-01-01T00:00:00Z", None),
        ];

        let entry = resolve_backup_selection(&list, Some(2), None).unwrap();

        assert_eq!(entry.created_at, "2026-01-01T00:00:00Z");
    }

    // --- run_save_command_with / --force (review point 6) -----------------

    /// Builds a fixture with exactly one save user directory (analogous to
    /// `run_play`'s vanilla backup test) and takes a backup of the original
    /// content in it before that content is overwritten. That makes it
    /// possible to check afterwards whether `run_save_command_with` actually
    /// restored or not.
    fn fixture_with_one_backup(tmp: &std::path::Path) -> (AppState, PathBuf) {
        let state = test_fixture(tmp);
        let save_dir = tmp
            .join("steamapps/compatdata/2183900/pfx/drive_c/users/steamuser")
            .join("AppData/Local/Saber/Space Marine 2/storage/steam/user/76561198000000009/Main");
        std::fs::create_dir_all(&save_dir).unwrap();
        std::fs::write(save_dir.join("profile.sav"), b"ORIGINAL").unwrap();

        saves::backup(&save_dir, &state.backups_dir(), None).unwrap();
        std::fs::write(save_dir.join("profile.sav"), b"GEAENDERT").unwrap();

        (state, save_dir)
    }

    fn restore_default() -> SaveCommand {
        SaveCommand::Restore { index: None, at: None, force: false }
    }

    #[test]
    fn save_restore_refuses_when_steam_is_running_without_force() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, save_dir) = fixture_with_one_backup(tmp.path());

        let err = run_save_command_with(&state, restore_default(), || true).unwrap_err();

        assert!(err.to_string().contains("Steam"), "{err}");
        assert_eq!(
            std::fs::read(save_dir.join("profile.sav")).unwrap(),
            b"GEAENDERT",
            "without --force nothing may be restored"
        );
    }

    /// Also proves that `--force` is not inverted: `force: true` together
    /// with a running Steam must actually restore, not refuse.
    #[test]
    fn save_restore_force_overrides_the_steam_running_refusal() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, save_dir) = fixture_with_one_backup(tmp.path());

        run_save_command_with(
            &state,
            SaveCommand::Restore { index: None, at: None, force: true },
            || true,
        )
        .unwrap();

        assert_eq!(std::fs::read(save_dir.join("profile.sav")).unwrap(), b"ORIGINAL");
    }

    /// Control test: without a running Steam the restore happens as normal,
    /// regardless of `force` — the refusal depends solely on
    /// `steam_running()`, not on a swapped condition.
    #[test]
    fn save_restore_without_force_still_restores_when_steam_is_not_running() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, save_dir) = fixture_with_one_backup(tmp.path());

        run_save_command_with(&state, restore_default(), || false).unwrap();

        assert_eq!(std::fs::read(save_dir.join("profile.sav")).unwrap(), b"ORIGINAL");
    }

    // --- find_profile_by_name (2e: an ambiguous case-insensitive ---------
    // --- match is reported instead of being silently picked) -------------

    fn empty_profile(name: &str) -> Profile {
        Profile::from_config(name, &sm2_core::pak_config::PakConfig::default())
    }

    #[test]
    fn find_profile_by_name_matches_case_insensitively_when_unambiguous() {
        let dir = tempfile::tempdir().unwrap();
        empty_profile("Astartes").save(dir.path()).unwrap();

        let found = find_profile_by_name(dir.path(), "astartes").unwrap();

        assert_eq!(found.name, "Astartes");
    }

    #[test]
    fn find_profile_by_name_reports_ambiguity_instead_of_silently_picking_one() {
        let dir = tempfile::tempdir().unwrap();
        empty_profile("Test").save(dir.path()).unwrap();
        empty_profile("test").save(dir.path()).unwrap();

        let err = find_profile_by_name(dir.path(), "TEST").unwrap_err();

        let message = err.to_string();
        assert!(message.contains("Test") && message.contains("test"), "{message}");
    }

    #[test]
    fn find_profile_by_name_reports_when_nothing_matches() {
        let dir = tempfile::tempdir().unwrap();

        let err = find_profile_by_name(dir.path(), "gibt es nicht").unwrap_err();

        assert!(err.to_string().contains("gibt es nicht"));
    }

    // --- profile delete (2d) ----------------------------------------------

    #[test]
    fn profile_delete_removes_the_file_matched_case_insensitively() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        run_profile_command(&mut state, ProfileCommand::Save { name: "Zu Löschen".into() }).unwrap();
        let dir = state.profiles_dir();
        assert_eq!(list_profiles(&dir).unwrap().len(), 1);

        run_profile_command(&mut state, ProfileCommand::Delete { name: "zu löschen".into() }).unwrap();

        assert!(list_profiles(&dir).unwrap().is_empty());
    }

    #[test]
    fn profile_delete_reports_a_clear_error_for_an_unknown_name() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());

        let err = run_profile_command(&mut state, ProfileCommand::Delete { name: "unbekannt".into() })
            .unwrap_err();

        assert!(err.to_string().contains("nicht gefunden"), "{err}");
    }

    // --- run_play_with (2c/2f: auto backup for --vanilla, injectable -----
    // --- game launch) ------------------------------------------------------

    /// `no_eac_available()` reads this system's real `$PATH`. On the test
    /// machines here `umu-run` is not installed, but this test still must
    /// not fail on a system where it does exist anyway (installed by
    /// accident, say).
    #[test]
    fn run_play_rejects_no_eac_when_direct_launch_is_unavailable_and_never_launches() {
        if no_eac_available() {
            eprintln!("skipped: umu-launcher is installed on this system");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());

        let called = std::rc::Rc::new(std::cell::Cell::new(false));
        let called_clone = called.clone();
        let result = run_play_with(&mut state, false, true, move |_paths, _mode| {
            called_clone.set(true);
            Ok(())
        });

        assert!(result.is_err(), "without an available direct launch --no-eac must be rejected");
        assert!(!called.get(), "the game launch must never be invoked in that case");
    }

    /// A missing save directory (here: `test_fixture` has no Proton prefix)
    /// must not prevent the launch — the auto backup is a convenience
    /// feature, not a hard requirement (see `run_play_with`'s doc comment).
    #[test]
    fn run_play_warns_but_still_launches_when_auto_backup_has_no_save_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        state.settings.auto_backup = true;

        let called = std::rc::Rc::new(std::cell::Cell::new(None));
        let called_clone = called.clone();
        let result = run_play_with(&mut state, false, false, move |_paths, mode| {
            called_clone.set(Some(mode));
            Ok(())
        });

        assert!(result.is_ok(), "a missing save directory must not prevent the launch: {result:?}");
        assert_eq!(called.get(), Some(LaunchMode::Steam));
    }

    /// 2c: Spec §6.4 describes the vanilla launch as identical to the modded
    /// one, backup included — previously the auto backup ran only without
    /// `--vanilla`. The fixture deliberately gets a real save user folder
    /// here: with the original `test_fixture` (no Proton prefix) the auto
    /// backup attempt would only have warned instead of actually backing up,
    /// so a reintroduced `if !vanilla` would stay green and undetected
    /// (review point 5).
    #[test]
    fn run_play_vanilla_disables_everything_persists_and_still_launches() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        std::fs::write(state.paths.mods_dir().join("a.pak"), b"x").unwrap();
        state.config.entries = vec![PakEntry { pak: "a.pak".into(), disabled: false }];
        state.settings.auto_backup = true;

        let save_dir = tmp
            .path()
            .join("steamapps/compatdata/2183900/pfx/drive_c/users/steamuser")
            .join("AppData/Local/Saber/Space Marine 2/storage/steam/user/76561198000000009/Main");
        std::fs::create_dir_all(&save_dir).unwrap();
        std::fs::write(save_dir.join("profile.sav"), b"FORTSCHRITT").unwrap();

        let called = std::rc::Rc::new(std::cell::Cell::new(None));
        let called_clone = called.clone();
        run_play_with(&mut state, true, false, move |_paths, mode| {
            called_clone.set(Some(mode));
            Ok(())
        })
        .unwrap();

        assert!(state.config.entries.iter().all(|e| e.disabled), "a vanilla start must disable everything");
        assert_eq!(called.get(), Some(LaunchMode::Steam));
        let saved = sm2_core::pak_config::PakConfig::load(&state.paths.pak_config_path()).unwrap();
        assert!(
            saved.entries.iter().all(|e| e.disabled),
            "the disabled configuration must actually have landed on disk"
        );

        let backups = saves::list_backups(&state.backups_dir()).unwrap();
        assert_eq!(backups.len(), 1, "a vanilla start must create an auto backup as well (spec §6.4)");
        assert_eq!(backups[0].label.as_deref(), Some("vor Vanilla-Start"));
    }

    /// Uses a write attempt to check whether a `0o555` permission on `dir`
    /// really protects against write access, and restores the original
    /// permission afterwards. If the test process runs as root, the kernel
    /// overrides every file mode — a test that does not notice this would
    /// fail there for no reason (the same pattern as in `app_state.rs`'s
    /// `check_write_permission_fails_for_a_read_only_dir`).
    #[cfg(unix)]
    fn write_protection_is_effective(dir: &std::path::Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(dir).unwrap().permissions();
        perms.set_mode(0o555);
        std::fs::set_permissions(dir, perms).unwrap();

        let probe = dir.join(".probe");
        let bypassed = std::fs::write(&probe, b"").is_ok();
        let _ = std::fs::remove_file(&probe);

        let mut perms = std::fs::metadata(dir).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(dir, perms).unwrap();

        !bypassed
    }

    #[cfg(unix)]
    #[test]
    fn run_play_vanilla_aborts_without_launching_if_persist_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        state.config.entries = vec![PakEntry { pak: "a.pak".into(), disabled: false }];

        if !write_protection_is_effective(&state.paths.mods_dir()) {
            eprintln!("skipped: this process can apparently bypass write protection (root?)");
            return;
        }

        use std::os::unix::fs::PermissionsExt;
        let mods_dir = state.paths.mods_dir();
        let mut perms = std::fs::metadata(&mods_dir).unwrap().permissions();
        perms.set_mode(0o555);
        std::fs::set_permissions(&mods_dir, perms.clone()).unwrap();

        let called = std::rc::Rc::new(std::cell::Cell::new(false));
        let called_clone = called.clone();
        let result = run_play_with(&mut state, true, false, move |_paths, _mode| {
            called_clone.set(true);
            Ok(())
        });

        perms.set_mode(0o755);
        std::fs::set_permissions(&mods_dir, perms).unwrap();

        assert!(result.is_err(), "a failing persist() must be fatal with --vanilla");
        assert!(!called.get(), "the game must not start after a failed persist() with --vanilla");
    }

    #[cfg(unix)]
    #[test]
    fn run_play_non_vanilla_persist_failure_only_warns_and_still_launches() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());

        if !write_protection_is_effective(&state.paths.mods_dir()) {
            eprintln!("skipped: this process can apparently bypass write protection (root?)");
            return;
        }

        use std::os::unix::fs::PermissionsExt;
        let mods_dir = state.paths.mods_dir();
        let mut perms = std::fs::metadata(&mods_dir).unwrap().permissions();
        perms.set_mode(0o555);
        std::fs::set_permissions(&mods_dir, perms.clone()).unwrap();

        let called = std::rc::Rc::new(std::cell::Cell::new(false));
        let called_clone = called.clone();
        let result = run_play_with(&mut state, false, false, move |_paths, _mode| {
            called_clone.set(true);
            Ok(())
        });

        perms.set_mode(0o755);
        std::fs::set_permissions(&mods_dir, perms).unwrap();

        assert!(result.is_ok(), "a failing persist() may only warn without --vanilla: {result:?}");
        assert!(called.get(), "the game must be launched despite the failed persist()");
    }
}

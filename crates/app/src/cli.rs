//! Kommandozeilenoberfläche: Definition und Ausführung aller Unterbefehle.

use crate::app_state::AppState;
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use sm2_core::launch::{launch, no_eac_available, LaunchMode};
use sm2_core::pak_config::PakEntry;
use sm2_core::platform::{Current, Platform};
use sm2_core::profile::{list_profiles, Profile};
use sm2_core::{import, saves};
use std::collections::HashSet;
use std::path::PathBuf;

/// Name, unter dem `play --vanilla` den bisherigen Zustand sichert, bevor er
/// überschrieben wird.
const VANILLA_SNAPSHOT_NAME: &str = "vor Vanilla-Start";

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
        #[arg(long)]
        index: Option<usize>,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let mut state = AppState::open()?;

    match cli.command {
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
            set_disabled(&mut state, &pak, false)?;
            state.persist()?;
            println!("✓ {pak} aktiviert");
        }

        Command::Disable { pak } => {
            set_disabled(&mut state, &pak, true)?;
            state.persist()?;
            println!("✓ {pak} deaktiviert");
        }

        Command::Order { paks } => run_order(&mut state, paks)?,

        Command::Install { files } => run_install(&mut state, &files)?,

        Command::Paths => {
            println!("Spiel:   {}", state.paths.game_dir.display());
            println!("Mods:    {}", state.paths.mods_dir().display());
            println!("Config:  {}", state.paths.pak_config_path().display());
            match state.paths.save_dir() {
                Ok(p) => println!("Saves:   {}", p.display()),
                Err(e) => println!("Saves:   nicht verfügbar – {e}"),
            }
            println!("Backups: {}", state.backups_dir().display());
        }

        Command::Open { target } => {
            let path = match target {
                OpenTarget::Game => state.paths.game_dir.clone(),
                OpenTarget::Mods => state.paths.mods_dir(),
                OpenTarget::Saves => state.paths.save_dir()?,
                OpenTarget::Backups => {
                    let dir = state.backups_dir();
                    std::fs::create_dir_all(&dir)
                        .with_context(|| format!("{} konnte nicht angelegt werden", dir.display()))?;
                    dir
                }
            };
            Current::open_folder(&path)?;
        }

        Command::Profile(cmd) => run_profile_command(&mut state, cmd)?,
        Command::Save(cmd) => run_save_command(&state, cmd)?,

        Command::Play { vanilla, no_eac } => run_play(&mut state, vanilla, no_eac)?,
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

/// Setzt die Ladereihenfolge neu. Genannte Paks müssen installiert sein und
/// dürfen nicht doppelt genannt werden; nicht genannte Paks behalten ihre
/// relative Reihenfolge und werden ans Ende gehängt.
fn run_order(state: &mut AppState, requested: Vec<String>) -> Result<()> {
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

/// Importiert alle angegebenen Dateien. Jedes erfolgreich importierte Pak
/// wird sofort gespeichert (nicht erst am Ende aller Dateien): scheitert ein
/// späteres Pak – in derselben oder einer weiteren Datei –, bleiben bereits
/// erfolgreich importierte Mods dauerhaft in `library.json` und
/// `pak_config.yaml` eingetragen, statt nur als Datei im Mods-Verzeichnis zu
/// liegen, aber nirgends vermerkt zu sein.
fn run_install(state: &mut AppState, files: &[PathBuf]) -> Result<()> {
    if files.is_empty() {
        println!("Keine Dateien angegeben.");
        return Ok(());
    }

    // Muss so lange leben wie die Import-Aufrufe, die aus ihm lesen: für
    // Archive liegen die entpackten Paks bis zu ihrem Import unterhalb dieses
    // Verzeichnisses.
    let tmp = tempfile::tempdir().context("temporäres Verzeichnis konnte nicht angelegt werden")?;

    for file in files {
        let extracted = import::extract_paks(file, tmp.path())
            .with_context(|| format!("{} konnte nicht gelesen werden", file.display()))?;

        for pak in &extracted {
            let source = file.display().to_string();
            let outcome =
                import::import_pak(&state.paths, &mut state.library, &mut state.config, pak, Some(&source))
                    .with_context(|| format!("{} konnte nicht importiert werden", pak.display()))?;

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
            let profile = list_profiles(&dir)?
                .into_iter()
                .find(|p| p.name.eq_ignore_ascii_case(&name))
                .with_context(|| format!("Profil '{name}' nicht gefunden"))?;

            let (new_config, missing) = profile.apply(&state.paths.list_paks()?);
            for pak in &missing {
                eprintln!("Warnung: {pak} aus dem Profil ist nicht installiert, übersprungen.");
            }
            state.config = new_config;
            state.persist()?;
            println!("✓ Profil '{}' angewendet", profile.name);
        }
    }
    Ok(())
}

fn run_save_command(state: &AppState, cmd: SaveCommand) -> Result<()> {
    let backups = state.backups_dir();

    match cmd {
        SaveCommand::Backup { tag } => {
            let saves_dir = state.paths.save_dir()?;
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
        SaveCommand::Restore { index } => {
            let list = saves::list_backups(&backups)?;
            if list.is_empty() {
                bail!("keine Backups vorhanden");
            }
            let position = resolve_backup_index(index, list.len())?;
            let entry = &list[position];

            if saves::steam_is_running() {
                bail!(
                    "Steam läuft. Die Cloud-Synchronisation kann den wiederhergestellten Stand \
                     überschreiben. Bitte Steam beenden und erneut versuchen."
                );
            }

            let saves_dir = state.paths.save_dir()?;
            let safety_backup = saves::restore(entry, &saves_dir, &backups)?;
            println!("✓ Wiederhergestellt: {}", entry.created_at);
            println!("  Vorheriger Stand gesichert: {}", safety_backup.archive.display());
        }
    }
    Ok(())
}

/// Löst den 1-basierten, vom Nutzer angegebenen Backup-Index (ohne Angabe:
/// das neueste Backup, also 1) zu einem 0-basierten Vektorindex auf.
///
/// 0 ist kein gültiger Index (der Nutzer zählt ab 1) – ohne diese Prüfung
/// würde `index.unwrap_or(1) - 1` bei einer expliziten 0 als `usize`
/// unterlaufen (Panic im Debug-Build, Wraparound im Release-Build).
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

/// Startet das Spiel.
///
/// Ausfallverhalten (bewusst einheitlich für beide Fehlerquellen rund um das
/// Save-Backup): weder ein fehlendes Save-Verzeichnis noch ein
/// fehlschlagendes `saves::backup` brechen den Start ab – beides wird nur als
/// Warnung gemeldet. Das automatische Backup ist eine Komfortfunktion
/// (abschaltbar über `settings.toml`), kein hartes Erfordernis; der Nutzer
/// hat bereits über `auto_backup` zugestimmt, dass ein Backup versucht wird,
/// aber keine Zusage bekommen, dass ein Fehlschlag den Start verhindert. Der
/// Backup-Versuch geschieht außerdem vor `state.persist()` und vor dem Start
/// selbst – damit hinterlässt ein Fehlschlag nie eine bereits geschriebene
/// Konfiguration bei einem nie gestarteten Spiel.
///
/// Die Vanilla-Sicherung (Profil-Snapshot) ist dagegen ein hartes
/// Erfordernis: schlägt sie fehl, wird nichts verändert und nichts
/// gestartet – ohne sie gäbe es keinen Weg zurück zum bisherigen Setup.
fn run_play(state: &mut AppState, vanilla: bool, no_eac: bool) -> Result<()> {
    if no_eac && !no_eac_available() {
        bail!(
            "Start ohne EAC ist auf diesem System nicht möglich – umu-launcher wurde nicht \
             gefunden (https://github.com/Open-Wine-Components/umu-launcher)."
        );
    }

    if vanilla {
        let snapshot = Profile::from_config(VANILLA_SNAPSHOT_NAME, &state.config);
        let path = snapshot.save(&state.profiles_dir()).context(
            "bisheriger Zustand konnte nicht gesichert werden – Start ohne Sicherung wird verweigert",
        )?;
        println!(
            "Hinweis: bisheriger Zustand als Profil '{VANILLA_SNAPSHOT_NAME}' gesichert ({}).",
            path.display()
        );
        println!("  Mit `profile apply \"{VANILLA_SNAPSHOT_NAME}\"` wiederherstellen.");

        for entry in &mut state.config.entries {
            entry.disabled = true;
        }
    }

    if !vanilla && state.settings.auto_backup {
        match state.paths.save_dir() {
            Ok(saves_dir) => match saves::backup(&saves_dir, &state.backups_dir(), Some("vor Modded-Start")) {
                Ok(entry) => println!("✓ Save gesichert: {}", entry.archive.display()),
                Err(e) => eprintln!("Warnung: Save-Backup fehlgeschlagen – {e}"),
            },
            Err(e) => eprintln!("Warnung: kein Save-Backup möglich – {e}"),
        }
    }

    state.persist()?;

    let mode = if no_eac { LaunchMode::NoEac } else { LaunchMode::Steam };
    if no_eac {
        eprintln!("Hinweis: Start ohne EAC – Multiplayer ist damit nicht möglich.");
    }
    launch(&state.paths, mode)?;
    println!("✓ Spiel gestartet");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_command_structure_is_valid() {
        // Prüft die clap-Struktur selbst (Namenskollisionen, ungültige
        // Attribute etc.), ohne echte Argumente zu parsen.
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
}

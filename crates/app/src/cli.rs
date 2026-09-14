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

/// Namenspräfix, unter dem `play --vanilla` den bisherigen Zustand sichert,
/// bevor er überschrieben wird. Jeder Lauf hängt einen Zeitstempel an (siehe
/// `snapshot_and_disable_all_for_vanilla_start`), damit zwei Vanilla-Starts
/// hintereinander niemals denselben Profilnamen – und damit dieselbe Datei,
/// siehe `Profile::file_stem` – treffen und einander überschreiben.
const VANILLA_SNAPSHOT_PREFIX: &str = "vor Vanilla-Start";

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
            // Für ein einzelnes `.pak` als Eingabe liefert `extract_paks` den
            // Originalpfad unverändert zurück (nichts wurde nach `tmp`
            // kopiert) – ein direkter Import würde dann über `import_pak`s
            // rename-Schritt genau diese Datei aus dem Ordner verschwinden
            // lassen, in den der Nutzer sie gelegt hat (z. B. sein
            // Download-Verzeichnis). Für Archive gilt das nicht: deren
            // entpackte Paks liegen bereits unterhalb von `tmp`, das
            // Archiv selbst bleibt unangetastet.
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

/// Findet in `tmp` einen noch freien Namen für eine Arbeitskopie von
/// `source`. Kollidiert der ursprüngliche Dateiname bereits (z. B. weil zwei
/// angegebene Dateien denselben Basisnamen tragen), wird vor die Endung eine
/// laufende Nummer gehängt – dieselbe Grundidee wie `sm2_core::import`s
/// interne `unique_target_in`, hier lokal nachgebildet, da jene Funktion
/// crate-intern bleibt.
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

/// Sichert bei `play --vanilla` den aktuellen Zustand als Profil, bevor alle
/// Einträge deaktiviert werden, und deaktiviert sie anschließend.
///
/// Ist bereits kein Eintrag aktiv (z. B. weil dies der zweite
/// Vanilla-Start in Folge ist), wird kein neuer Schnappschuss angelegt: es
/// gibt nichts zu schützen, und ein Schnappschuss "alles deaktiviert" würde
/// über `Profile::file_stem` (Name + Zeitstempel) zwar nie eine frühere
/// Sicherung überschreiben, wäre aber wertlos und würde `profile list` nur
/// zumüllen. Ist dagegen mindestens ein Eintrag aktiv, hängt der Profilname
/// einen Zeitstempel an das Präfix `VANILLA_SNAPSHOT_PREFIX` an – so trifft
/// niemals ein zweiter Vanilla-Start denselben Dateinamen und überschreibt
/// die Sicherung, auf die ein späteres `profile apply` sich verlassen soll.
///
/// Enthält keine E/A jenseits von `Profile::save` – insbesondere kein
/// `persist()` und kein `launch()` –, ist also unabhängig vom eigentlichen
/// Spielstart testbar.
fn snapshot_and_disable_all_for_vanilla_start(state: &mut AppState) -> Result<()> {
    if state.config.entries.iter().any(|e| !e.disabled) {
        let snapshot_name = format!("{VANILLA_SNAPSHOT_PREFIX} {}", timestamp_for_snapshot_name());
        let snapshot = Profile::from_config(&snapshot_name, &state.config);
        let path = snapshot.save(&state.profiles_dir()).context(
            "bisheriger Zustand konnte nicht gesichert werden – Start ohne Sicherung wird verweigert",
        )?;
        println!("Hinweis: bisheriger Zustand als Profil '{snapshot_name}' gesichert ({}).", path.display());
        println!("  Mit `profile apply \"{snapshot_name}\"` wiederherstellen.");
    } else {
        eprintln!("Hinweis: bereits vollständig deaktiviert – keine neue Sicherung angelegt.");
    }

    for entry in &mut state.config.entries {
        entry.disabled = true;
    }
    Ok(())
}

/// Menschenlesbarer Zeitstempel ("2026-09-14 21:40") für den Namen einer
/// Vanilla-Sicherung, auf die Minute genau. Eigene, kleine Umsetzung statt
/// einer zusätzlichen Abhängigkeit; `sm2_core` hat eine vergleichbare
/// Funktion, hält sie aber bewusst `pub(crate)`.
fn timestamp_for_snapshot_name() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = now.div_euclid(86_400);
    let remainder = now.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let y = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let d = day_of_year - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02} {:02}:{:02}", remainder / 3600, (remainder % 3600) / 60)
}

/// Startet das Spiel.
///
/// Ausfallverhalten für das Save-Backup (bewusst einheitlich für beide
/// Fehlerquellen): weder ein fehlendes Save-Verzeichnis noch ein
/// fehlschlagendes `saves::backup` brechen den Start ab – beides wird nur als
/// Warnung gemeldet. Das automatische Backup ist eine Komfortfunktion
/// (abschaltbar über `settings.toml`), kein hartes Erfordernis. Der
/// Backup-Versuch geschieht außerdem vor `state.persist()` und vor dem Start
/// selbst – damit hinterlässt ein Fehlschlag nie eine bereits geschriebene
/// Konfiguration bei einem nie gestarteten Spiel.
///
/// Ausfallverhalten für `persist()` selbst: bei einem gewöhnlichen (nicht
/// Vanilla-)Start ändert `persist()` höchstens das Ergebnis des Abgleichs aus
/// `AppState::open()` – nichts, was der Nutzer mit diesem Aufruf beabsichtigt
/// hat. Schlägt es fehl (z. B. schreibgeschütztes Mods-Verzeichnis), wird das
/// nur gewarnt; das Spiel startet trotzdem, mit der auf der Platte bereits
/// vorhandenen (von der Engine ohnehin so gelesenen) Konfiguration. Bei
/// `--vanilla` bleibt `persist()` dagegen fatal: das Deaktivieren aller Mods
/// ist der ganze Zweck des Aufrufs, ein Start mit unverändert aktiven Mods
/// wäre das Gegenteil dessen, was der Nutzer wollte.
///
/// Die Vanilla-Sicherung selbst (siehe
/// `snapshot_and_disable_all_for_vanilla_start`) ist immer ein hartes
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
        snapshot_and_disable_all_for_vanilla_start(state)?;
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

    if vanilla {
        state.persist()?;
    } else if let Err(e) = state.persist() {
        eprintln!("Warnung: Konfiguration konnte nicht gespeichert werden – {e:#}");
    }

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
    use crate::app_state::test_fixture;
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

    // --- run_order -----------------------------------------------------

    #[test]
    fn run_order_with_no_arguments_is_a_no_op() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        state.config.entries = vec![PakEntry { pak: "a.pak".into(), disabled: false }];

        run_order(&mut state, vec![]).unwrap();

        assert_eq!(state.config.entries.len(), 1);
        assert!(!state.paths.pak_config_path().exists(), "ein No-Op darf nichts schreiben");
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
        assert_eq!(names, vec!["c.pak", "a.pak", "b.pak"], "jedes vorhandene Pak muss erhalten bleiben");
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

        assert!(state.config.entries.iter().all(|e| e.disabled), "danach muss alles deaktiviert sein");

        let profiles = list_profiles(&state.profiles_dir()).unwrap();
        assert_eq!(profiles.len(), 1);
        assert!(profiles[0].name.starts_with(VANILLA_SNAPSHOT_PREFIX));
        let a = profiles[0].entries.iter().find(|e| e.pak == "a.pak").unwrap();
        assert!(!a.disabled, "die Sicherung muss den Zustand VOR dem Deaktivieren zeigen");
    }

    #[test]
    fn two_consecutive_vanilla_runs_keep_the_first_snapshot_intact() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        state.config.entries = vec![PakEntry { pak: "a.pak".into(), disabled: false }];

        snapshot_and_disable_all_for_vanilla_start(&mut state).unwrap();
        // Zweiter Lauf: jetzt ist bereits alles deaktiviert.
        snapshot_and_disable_all_for_vanilla_start(&mut state).unwrap();

        let profiles = list_profiles(&state.profiles_dir()).unwrap();
        assert_eq!(
            profiles.len(),
            1,
            "der zweite Vanilla-Start darf die erste Sicherung nicht überschreiben oder verdoppeln"
        );
        assert!(
            profiles[0].entries.iter().any(|e| !e.disabled),
            "die einzige Sicherung muss weiterhin den ursprünglichen, aktiven Zustand zeigen"
        );
    }

    #[test]
    fn vanilla_snapshot_is_skipped_when_nothing_is_enabled() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        state.config.entries = vec![PakEntry { pak: "a.pak".into(), disabled: true }];

        snapshot_and_disable_all_for_vanilla_start(&mut state).unwrap();

        let profiles = list_profiles(&state.profiles_dir()).unwrap();
        assert!(profiles.is_empty(), "ohne aktive Mods gibt es nichts zu sichern");
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

        assert!(source_pak.is_file(), "die Quelldatei des Nutzers darf nicht verschwinden");
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
        // Existiert absichtlich nicht: simuliert die dritte, fehlschlagende Datei.
        let third = source_dir.path().join("drittes.pak");
        let fourth = source_dir.path().join("viertes.pak");
        std::fs::write(&fourth, b"VIER").unwrap();

        let files = vec![first, second, third, fourth];
        let err = run_install(&mut state, &files).unwrap_err();
        assert!(err.to_string().contains("drittes.pak"), "{err}");

        // Die ersten beiden Dateien liegen bereits im Mods-Verzeichnis...
        assert!(state.paths.mods_dir().join("erstes.pak").is_file());
        assert!(state.paths.mods_dir().join("zweites.pak").is_file());

        // ...und sind sowohl in library.json als auch in pak_config.yaml
        // dauerhaft vermerkt, nicht nur im (in-memory) AppState.
        let saved_library = sm2_core::library::Library::load(&state.dirs.data.join("library.json")).unwrap();
        assert!(saved_library.mods.contains_key("erstes.pak"));
        assert!(saved_library.mods.contains_key("zweites.pak"));

        let saved_config = sm2_core::pak_config::PakConfig::load(&state.paths.pak_config_path()).unwrap();
        let names: Vec<&str> = saved_config.entries.iter().map(|e| e.pak.as_str()).collect();
        assert!(names.contains(&"erstes.pak"));
        assert!(names.contains(&"zweites.pak"));
        assert!(
            !names.contains(&"viertes.pak"),
            "nach dem Fehlschlag darf die vierte Datei nicht mehr verarbeitet worden sein"
        );
    }
}

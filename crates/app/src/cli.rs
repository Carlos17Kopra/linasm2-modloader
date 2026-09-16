//! Kommandozeilenoberfläche: Definition und Ausführung aller Unterbefehle.

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
        // `--index` verschiebt sich mit jeder Wiederherstellung, weil
        // `restore` selbst ein neues "vor Wiederherstellung"-Backup anlegt
        // (siehe `run_save_command`s Doc-Kommentar) – `--at` bleibt dagegen
        // unabhängig davon eindeutig.
        /// Exakter Zeitstempel aus `save list` – eindeutig, verschiebt sich
        /// anders als `--index` nicht durch spätere Wiederherstellungen
        #[arg(long, conflicts_with = "index")]
        at: Option<String>,
        // Spec §6.5/§9 R2: Cloud-Synchronisation kann den zurückgespielten
        // Stand im Hintergrund überschreiben – der Normalfall ist deshalb
        // die Ablehnung, `--force` ist die bewusste Ausnahme.
        /// Erzwingt die Wiederherstellung trotz laufendem Steam (Risiko:
        /// Cloud-Synchronisation kann den Stand überschreiben)
        #[arg(long)]
        force: bool,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let mut state = AppState::open()?;
    // Meldungen aus dem Abgleich zuerst, damit sie vor der Ausgabe des
    // eigentlichen Befehls stehen – wie zuvor, als `AppState::open()` sie
    // noch selbst auf stderr schrieb.
    print_notices(&mut state);

    let result = run_command(&mut state, cli.command);
    // Und noch einmal danach: `persist()` kann unterwegs weitere Meldungen
    // angehängt haben, die sonst niemand zu sehen bekäme.
    print_notices(&mut state);
    result
}

/// Schreibt alle aufgelaufenen Meldungen nach stderr und leert den Puffer.
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

/// Findet genau ein Profil zu `name` unter `dir`, groß-/kleinschreibungs-
/// unabhängig (Bequemlichkeit: der Nutzer muss den Namen nicht exakt
/// treffen). `Profile::file_stem` hasht dagegen den exakten Namen – zwei
/// Profile wie "Test" und "test" landen also in zwei verschiedenen Dateien.
/// Ohne diese Prüfung würde ein mehrdeutiger Name klaglos das erste (nach
/// `list_profiles`s alphabetischer Sortierung) Treffer-Profil wählen und das
/// andere wäre über `apply`/`delete` faktisch unerreichbar, ohne dass der
/// Nutzer je davon erführe. Ein mehrdeutiger Treffer wird stattdessen mit
/// allen betroffenen Namen gemeldet, damit der Nutzer den exakten Namen
/// nachreichen kann.
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

/// Kern von `run_save_command`, mit der Steam-Erkennung als Parameter statt
/// fest verdrahtet – genau dieselbe Seam-Idee wie `run_play`/`run_play_with`
/// weiter unten. `--force` ist der einzige Zweig hier, der tatsächlich
/// Save-Daten überschreiben kann (Steam läuft, Cloud-Sync könnte den
/// zurückgespielten Stand überschreiben); ohne diese Injektion ließe sich
/// weder die Ablehnung noch die Umgehung deterministisch testen, weil
/// `saves::steam_is_running()` den echten `/proc` dieses Systems liest.
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

            // Echo, was tatsächlich ausgewählt wurde, BEVOR etwas verändert
            // wird: `restore` legt vor der Wiederherstellung selbst ein "vor
            // Wiederherstellung"-Backup an, und `list_backups` ist
            // neueste-zuerst – jede Wiederherstellung verschiebt also jeden
            // späteren `--index`. Der Nutzer sieht hier, was `--index`/`--at`
            // tatsächlich getroffen hat, bevor der Vorgang unumkehrbar wird
            // (siehe `resolve_backup_selection`s Doc-Kommentar).
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

/// Wählt ein Backup aus `list` entweder über den exakten Zeitstempel (`at`,
/// wie ihn `save list` anzeigt) oder über den 1-basierten Index (`index`,
/// Standard: das neueste). `--at` ist die unzweideutige Wahl: `restore`
/// legt vor jeder Wiederherstellung selbst ein neues Backup an, und
/// `list_backups` sortiert neueste zuerst – ein per `--index` gewähltes
/// Backup verschiebt sich also mit jeder Wiederherstellung um eins. `clap`s
/// `conflicts_with` verhindert bereits, dass beide zugleich angegeben
/// werden.
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

/// Sichert bei `play --vanilla` den bisherigen Zustand und meldet das
/// Ergebnis auf der Kommandozeile. Die Regel selbst steht in
/// `crate::vanilla` – sie gilt für die Oberfläche genauso.
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

/// Startet das Spiel.
fn run_play(state: &mut AppState, vanilla: bool, no_eac: bool) -> Result<()> {
    run_play_with(state, vanilla, no_eac, launch)
}

/// Kern von `run_play`, mit dem eigentlichen Spielstart als Parameter statt
/// fest verdrahtet: `run_play` selbst startet immer den echten Prozess über
/// `sm2_core::launch::launch`, aber jede Verzweigung davor (Vanilla-Wipe,
/// Auto-Backup, `persist()`-Fehlerverhalten, No-EAC-Gating) lässt sich so
/// testen, ohne je einen echten Prozess zu starten – die Tests unten
/// übergeben stattdessen eine Closure, die nur festhält, ob und mit welchem
/// `LaunchMode` sie aufgerufen wurde.
///
/// Ausfallverhalten für das Save-Backup (bewusst einheitlich für beide
/// Fehlerquellen): weder ein fehlendes Save-Verzeichnis noch ein
/// fehlschlagendes `saves::backup` brechen den Start ab – beides wird nur als
/// Warnung gemeldet. Das automatische Backup ist eine Komfortfunktion
/// (abschaltbar über `settings.toml`), kein hartes Erfordernis. Der
/// Backup-Versuch geschieht außerdem vor `state.persist()` und vor dem Start
/// selbst – damit hinterlässt ein Fehlschlag nie eine bereits geschriebene
/// Konfiguration bei einem nie gestarteten Spiel. Anders als zuvor gilt das
/// jetzt auch für `--vanilla`: Spec §6.4 beschreibt den Vanilla-Start
/// ausdrücklich als identisch zum Modded-Start, Backup eingeschlossen – ein
/// Nutzer greift zu `--vanilla` oft gerade *nachdem* schon etwas schiefging,
/// und genau dann ist das Backup am wichtigsten.
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
        assert!(profiles[0].name.starts_with(vanilla::VANILLA_SNAPSHOT_PREFIX));
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

    // --- resolve_backup_selection (2a: --at neben --index) ---------------

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

    /// Der eigentliche Grund für `--at`: ein per `--index` gewähltes Backup
    /// verschiebt sich mit jeder Wiederherstellung. Ein exakter Zeitstempel
    /// bleibt dagegen eindeutig, unabhängig davon, wie oft zwischenzeitlich
    /// wiederhergestellt wurde.
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

    // --- run_save_command_with / --force (Review-Punkt 6) -----------------

    /// Baut eine Fixture mit genau einem Save-Nutzerverzeichnis (analog zu
    /// `run_play`s Vanilla-Backup-Test) und legt darin ein Backup des
    /// Originalinhalts an, bevor der Inhalt überschrieben wird – so lässt
    /// sich anschließend prüfen, ob `run_save_command_with` tatsächlich
    /// wiederhergestellt hat oder nicht.
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
            "ohne --force darf nichts wiederhergestellt werden"
        );
    }

    /// Beweist zugleich, dass `--force` nicht invertiert ist: `force: true`
    /// zusammen mit einem laufenden Steam muss tatsächlich wiederherstellen,
    /// nicht ablehnen.
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

    /// Kontrolltest: ohne laufendes Steam wird ganz normal wiederhergestellt,
    /// unabhängig von `force` – die Ablehnung hängt ausschließlich an
    /// `steam_running()`, nicht an einer vertauschten Bedingung.
    #[test]
    fn save_restore_without_force_still_restores_when_steam_is_not_running() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, save_dir) = fixture_with_one_backup(tmp.path());

        run_save_command_with(&state, restore_default(), || false).unwrap();

        assert_eq!(std::fs::read(save_dir.join("profile.sav")).unwrap(), b"ORIGINAL");
    }

    // --- find_profile_by_name (2e: mehrdeutiger groß-/kleinschreibungs- --
    // --- unabhängiger Treffer wird gemeldet statt still gewählt) ---------

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

    // --- run_play_with (2c/2f: Auto-Backup für --vanilla, injizierbarer --
    // --- Spielstart) -------------------------------------------------------

    /// `no_eac_available()` liest den echten `$PATH` dieses Systems – auf
    /// den Testrechnern hier ist `umu-run` nicht installiert, aber dieser
    /// Test darf trotzdem nicht auf einem System scheitern, auf dem es (z. B.
    /// versehentlich) doch vorhanden ist.
    #[test]
    fn run_play_rejects_no_eac_when_direct_launch_is_unavailable_and_never_launches() {
        if no_eac_available() {
            eprintln!("übersprungen: umu-launcher ist auf diesem System installiert");
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

        assert!(result.is_err(), "ohne verfügbaren Direktstart muss --no-eac abgelehnt werden");
        assert!(!called.get(), "der Spielstart darf dabei nie aufgerufen werden");
    }

    /// Ein fehlendes Save-Verzeichnis (hier: `test_fixture` hat keinen
    /// Proton-Prefix) darf den Start nicht verhindern – das Auto-Backup ist
    /// eine Komfortfunktion, kein hartes Erfordernis (siehe Doc-Kommentar von
    /// `run_play_with`).
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

        assert!(result.is_ok(), "fehlendes Save-Verzeichnis darf den Start nicht verhindern: {result:?}");
        assert_eq!(called.get(), Some(LaunchMode::Steam));
    }

    /// 2c: Spec §6.4 beschreibt den Vanilla-Start als identisch zum Modded-
    /// Start, Backup eingeschlossen – zuvor lief das Auto-Backup nur ohne
    /// `--vanilla`. Die Fixture bekommt hier absichtlich einen echten
    /// Save-Nutzerordner: mit dem ursprünglichen `test_fixture` (kein
    /// Proton-Prefix) hätte der Auto-Backup-Versuch ohnehin nur gewarnt statt
    /// tatsächlich zu sichern – ein wieder eingeführtes `if !vanilla` bliebe
    /// dann unentdeckt grün (Review-Punkt 5).
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

        assert!(state.config.entries.iter().all(|e| e.disabled), "Vanilla-Start muss alles deaktivieren");
        assert_eq!(called.get(), Some(LaunchMode::Steam));
        let saved = sm2_core::pak_config::PakConfig::load(&state.paths.pak_config_path()).unwrap();
        assert!(
            saved.entries.iter().all(|e| e.disabled),
            "die deaktivierte Konfiguration muss tatsächlich auf der Platte gelandet sein"
        );

        let backups = saves::list_backups(&state.backups_dir()).unwrap();
        assert_eq!(backups.len(), 1, "ein Vanilla-Start muss ebenfalls ein Auto-Backup anlegen (Spec §6.4)");
        assert_eq!(backups[0].label.as_deref(), Some("vor Vanilla-Start"));
    }

    /// Prüft per Schreibversuch, ob eine `0o555`-Berechtigung auf `dir`
    /// tatsächlich vor Schreibzugriff schützt, und stellt die ursprüngliche
    /// Berechtigung danach wieder her. Läuft der Testprozess als root, hebt
    /// der Kernel jeden Dateimodus auf – ein Test, der das nicht erkennt,
    /// würde dort grundlos fehlschlagen (dasselbe Muster wie in
    /// `app_state.rs`s `check_write_permission_fails_for_a_read_only_dir`).
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
            eprintln!("übersprungen: Prozess kann den Schreibschutz offenbar übergehen (root?)");
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

        assert!(result.is_err(), "ein persist()-Fehlschlag muss bei --vanilla fatal sein");
        assert!(!called.get(), "das Spiel darf nach fehlgeschlagenem persist() bei --vanilla nicht starten");
    }

    #[cfg(unix)]
    #[test]
    fn run_play_non_vanilla_persist_failure_only_warns_and_still_launches() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());

        if !write_protection_is_effective(&state.paths.mods_dir()) {
            eprintln!("übersprungen: Prozess kann den Schreibschutz offenbar übergehen (root?)");
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

        assert!(result.is_ok(), "ein persist()-Fehlschlag darf ohne --vanilla nur warnen: {result:?}");
        assert!(called.get(), "das Spiel muss trotz gescheitertem persist() gestartet werden");
    }
}

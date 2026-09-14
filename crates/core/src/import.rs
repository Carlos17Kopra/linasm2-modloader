//! Import von Mods aus Archiven oder einzelnen `.pak`-Dateien.
//!
//! Design: ein importiertes Pak wird nie wieder verschoben; `pak_config.yaml`
//! bleibt die einzige Quelle der Wahrheit für Reihenfolge und
//! Aktivierungszustand. Ein Import hängt einen neuen Eintrag deaktiviert ans
//! Ende an, damit er niemals ein laufendes Setup verändert.

use crate::error::{Error, Result};
use crate::library::{hash_file, Library, ModInfo};
use crate::pak_config::{PakConfig, PakEntry};
use crate::paths::GamePaths;
use crate::platform::{Current, Platform};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Ergebnis eines Imports.
#[derive(Debug, PartialEq)]
pub struct ImportOutcome {
    pub pak: String,
    /// Gesetzt, wenn ein inhaltsgleicher Mod (laut Bibliothek) bereits vorhanden war.
    pub duplicate_of: Option<String>,
}

/// Holt alle `.pak`-Dateien aus einem Archiv nach `into`.
///
/// Unterstützt werden `.zip`, `.7z`, `.rar` sowie eine einzelne `.pak`-Datei
/// (wird dann unverändert zurückgegeben, ohne nach `into` kopiert zu
/// werden). Die Verzeichnisstruktur des Archivs wird abgeflacht – Mod-Archive
/// verpacken Paks gern in Ordner, für die Engine zählt nur die Datei.
///
/// Enthalten zwei Einträge denselben Dateinamen in unterschiedlichen Ordnern
/// (z. B. `a/mod.pak` und `b/mod.pak` – typisch für Archive mit mehreren
/// Installationsvarianten), werden beide behalten: der zweite bekommt eine
/// laufende Nummer vor der Endung (`mod.pak`, `mod_2.pak`, `mod_3.pak`, ...),
/// statt den ersten stillschweigend zu verdrängen.
pub fn extract_paks(archive: &Path, into: &Path) -> Result<Vec<PathBuf>> {
    let extension = archive
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_lowercase();

    let paks = match extension.as_str() {
        "pak" => vec![archive.to_path_buf()],
        "zip" => extract_zip(archive, into)?,
        "7z" => extract_seven_zip(archive, into)?,
        "rar" => extract_rar(archive, into)?,
        _ => return Err(Error::NoPakInArchive(archive.to_path_buf())),
    };

    if paks.is_empty() {
        return Err(Error::NoPakInArchive(archive.to_path_buf()));
    }
    Ok(paks)
}

/// Erkennt `.pak`-Dateien unabhängig von Groß-/Kleinschreibung der Endung.
fn is_pak_file(name: &str) -> bool {
    name.to_lowercase().ends_with(".pak")
}

/// Schneidet eine `.pak`-Endung unabhängig von Groß-/Kleinschreibung ab.
///
/// Nutzt `str::get` statt direkter Byte-Indizierung: bei einem Namen, der
/// nicht auf `.pak` endet, könnte die berechnete Schnittstelle sonst mitten
/// in einem mehrbyteigen UTF-8-Zeichen liegen (z. B. bei `"a€€"`) – `get`
/// liefert dann `None` statt einen Panic auszulösen.
fn strip_pak_suffix(name: &str) -> &str {
    let cut = name.len().saturating_sub(4);
    match name.get(cut..) {
        Some(suffix) if suffix.eq_ignore_ascii_case(".pak") => &name[..cut],
        _ => name,
    }
}

/// Findet in `into` einen noch freien Namen für eine Datei, die dort neu
/// abgelegt werden soll. Ist `desired` frei, wird er unverändert übernommen;
/// sonst wird – wie bei `unique_pak_name` – vor die Endung eine laufende
/// Nummer gehängt (`mod.pak` -> `mod_2.pak`, `mod_3.pak`, ...). Anders als
/// `unique_pak_name` prüft diese Variante nur den Inhalt von `into` selbst
/// (nicht Mods-Verzeichnis oder Konfiguration) – sie dient dem Entpacken,
/// nicht dem eigentlichen Import.
fn unique_target_in(into: &Path, desired: &OsStr) -> PathBuf {
    let candidate = into.join(desired);
    if !candidate.exists() {
        return candidate;
    }

    let desired_str = desired.to_string_lossy();
    let stem = strip_pak_suffix(&desired_str);
    let mut n = 2;
    loop {
        let candidate = into.join(format!("{stem}_{n}.pak"));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

fn extract_zip(archive: &Path, into: &Path) -> Result<Vec<PathBuf>> {
    let file = std::fs::File::open(archive).map_err(|e| Error::io(archive, e))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| Error::io(archive, std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;

    let mut targets: Vec<PathBuf> = Vec::new();

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| Error::io(archive, std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;
        if !entry.is_file() || !is_pak_file(entry.name()) {
            continue;
        }
        // Nur der Dateiname aus dem Archiveintrag wird übernommen – wehrt
        // zugleich Zip-Slip ab, da kein Verzeichnisanteil aus dem Archiv in
        // den Zielpfad einfließt. Ein Eintrag ohne Dateianteil (z. B. ein
        // reiner Verzeichniseintrag) wird übersprungen.
        let Some(file_name) = Path::new(entry.name()).file_name() else { continue };
        let target = unique_target_in(into, file_name);
        let mut out = std::fs::File::create(&target).map_err(|e| Error::io(&target, e))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| Error::io(&target, e))?;
        targets.push(target);
    }
    Ok(targets)
}

fn extract_seven_zip(archive: &Path, into: &Path) -> Result<Vec<PathBuf>> {
    let raw = into.join("__7z");
    std::fs::create_dir_all(&raw).map_err(|e| Error::io(&raw, e))?;
    sevenz_rust2::decompress_file(archive, &raw)
        .map_err(|e| Error::io(archive, std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;
    collect_paks_recursively(&raw, into)
}

/// `.rar` über ein externes Werkzeug – die `unrar`-Crate bindet unfreien
/// Quellcode ein und ist für eine Veröffentlichung nicht tragbar.
fn extract_rar(archive: &Path, into: &Path) -> Result<Vec<PathBuf>> {
    let raw = into.join("__rar");
    std::fs::create_dir_all(&raw).map_err(|e| Error::io(&raw, e))?;

    let tools: [(&str, &[&str]); 3] = [
        ("unar", &["-quiet", "-force-overwrite", "-output-directory"]),
        ("7z", &["x", "-y"]),
        ("7zz", &["x", "-y"]),
    ];

    // Wurde tatsächlich ein Werkzeug gefunden und ausgeführt, aber ist
    // gescheitert, ist das Problem das Archiv – nicht das fehlende
    // Werkzeug. `NoRarTool`s "bitte unar/7zip installieren" wäre dann eine
    // irreführende Empfehlung.
    let mut tool_ran = false;

    for (name, args) in &tools {
        let Some(tool_path) = Current::find_tool(name) else { continue };
        tool_ran = true;
        let mut cmd = std::process::Command::new(&tool_path);
        if *name == "unar" {
            cmd.args(*args).arg(&raw).arg(archive);
        } else {
            cmd.args(*args).arg(format!("-o{}", raw.display())).arg(archive);
        }
        // Das Werkzeug redet nicht mit dem Nutzer – nur unser eigenes
        // Ergebnis (Erfolg/Fehlschlag) zählt.
        cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        let status = cmd.status().map_err(|e| Error::io(archive, e))?;
        if status.success() {
            return collect_paks_recursively(&raw, into);
        }
    }

    if tool_ran {
        Err(Error::io(
            archive,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Archiv konnte nicht entpackt werden – ist es beschädigt?",
            ),
        ))
    } else {
        Err(Error::NoRarTool)
    }
}

/// Sucht `.pak`-Dateien in einem entpackten Baum und legt sie flach in
/// `into`. Landen zwei Dateien aus unterschiedlichen Unterordnern auf
/// demselben Zieldateinamen (z. B. `Option A/mod.pak` und `Option
/// B/mod.pak`), werden beide behalten: die zweite bekommt eine laufende
/// Nummer (`mod_2.pak`, `mod_3.pak`, ...) statt die erste zu verdrängen.
///
/// Jedes Verzeichnis wird vor der Verarbeitung nach Pfad sortiert, damit das
/// Ergebnis unabhängig von der (nicht zugesicherten) Reihenfolge von
/// `read_dir` deterministisch ist. Symlinks werden übersprungen: `unar`/`7z`
/// können Links aus einem Archiv wiederherstellen, und ein Link auf `/` oder
/// das Spielverzeichnis würde sonst dazu führen, dass fremde Dateien
/// außerhalb des Extraktionsordners eingesammelt werden.
fn collect_paks_recursively(from: &Path, into: &Path) -> Result<Vec<PathBuf>> {
    let mut targets: Vec<PathBuf> = Vec::new();
    collect_paks_into(from, into, &mut targets)?;
    targets.sort();
    Ok(targets)
}

fn collect_paks_into(dir: &Path, into: &Path, targets: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(dir)
        .map_err(|e| Error::io(dir, e))?
        .collect::<std::result::Result<Vec<_>, std::io::Error>>()
        .map_err(|e| Error::io(dir, e))?;
    entries.sort_by_key(std::fs::DirEntry::path);

    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type().map_err(|e| Error::io(&path, e))?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_paks_into(&path, into, targets)?;
        } else if let Some(file_name) =
            path.file_name().filter(|n| n.to_str().is_some_and(is_pak_file))
        {
            let target = unique_target_in(into, file_name);
            if path != target {
                std::fs::rename(&path, &target).map_err(|e| Error::io(&target, e))?;
            }
            targets.push(target);
        }
    }
    Ok(())
}

/// Findet einen im Mods-Verzeichnis und in der Konfiguration noch freien
/// Pak-Namen. Kollidiert `desired` mit einer bereits vorhandenen Datei oder
/// einem bereits eingetragenen Pak, wird vor die Endung eine laufende Nummer
/// gehängt (`mod.pak` -> `mod_2.pak`, `mod_3.pak`, ...). So überschreibt ein
/// Import nie eine bestehende Datei oder einen bestehenden Eintrag – auch
/// dann nicht, wenn deren Inhalt der Bibliothek unbekannt ist (z. B. von
/// Hand abgelegt und deshalb von der Hash-Dublettenprüfung nicht erfasst).
fn unique_pak_name(desired: &str, mods_dir: &Path, cfg: &PakConfig) -> String {
    let is_taken = |candidate: &str| {
        mods_dir.join(candidate).exists() || cfg.entries.iter().any(|e| e.pak == candidate)
    };

    if !is_taken(desired) {
        return desired.to_string();
    }

    let stem = strip_pak_suffix(desired);
    let mut n = 2;
    loop {
        let candidate = format!("{stem}_{n}.pak");
        if !is_taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Legt ein Pak im Spielverzeichnis ab und trägt es deaktiviert ans Ende
/// der Konfiguration ein.
///
/// Ist der Inhalt (laut Hash) bereits in der Bibliothek bekannt, wird nichts
/// kopiert oder eingetragen – `duplicate_of` nennt den vorhandenen Pak-Namen.
/// Kollidiert der Zieldateiname sonst mit einer vorhandenen Datei oder einem
/// vorhandenen Eintrag, wird ein alternativer Name vergeben (siehe
/// `unique_pak_name`), statt etwas zu überschreiben.
pub fn import_pak(
    paths: &GamePaths,
    lib: &mut Library,
    cfg: &mut PakConfig,
    pak: &Path,
    source: Option<&str>,
) -> Result<ImportOutcome> {
    let original_name = pak
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            Error::io(
                pak,
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "ungültiger Dateiname"),
            )
        })?
        .to_string();

    let hash = hash_file(pak)?;
    if let Some(existing) = lib.find_by_hash(&hash) {
        return Ok(ImportOutcome { pak: original_name, duplicate_of: Some(existing.pak.clone()) });
    }

    let size = std::fs::metadata(pak).map_err(|e| Error::io(pak, e))?.len();
    let mods_dir = paths.mods_dir();
    let final_name = unique_pak_name(&original_name, &mods_dir, cfg);
    let target = mods_dir.join(&final_name);

    // rename schlägt über Gerätegrenzen fehl – dann kopieren.
    if std::fs::rename(pak, &target).is_err() {
        if let Err(e) = std::fs::copy(pak, &target) {
            // Ein fehlgeschlagenes Kopieren kann eine unvollständige Datei
            // hinterlassen haben – die darf nicht liegen bleiben, sonst
            // hält `unique_pak_name` diesen Namen dauerhaft für belegt.
            let _ = std::fs::remove_file(&target);
            return Err(Error::io(&target, e));
        }
        let _ = std::fs::remove_file(pak);
    }

    let display_name = strip_pak_suffix(&final_name).replace(['_', '-'], " ");

    // Nach dem Ablegen frisch stat'en statt die vor dem Verschieben/Kopieren
    // gelesenen Metadaten wiederzuverwenden: `fs::copy` gibt keine Garantie,
    // dass die Änderungszeit erhalten bleibt, und `detect_altered`s billiger
    // Vorfilter muss den tatsächlichen Zustand der jetzt im Mods-Verzeichnis
    // liegenden Datei kennen. Schlägt der Stat-Aufruf ausnahmsweise fehl,
    // bleibt `mtime` `None` – der nächste Abgleich hasht dann einmalig statt
    // der fehlenden Information blind zu vertrauen.
    let mtime = std::fs::metadata(&target)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());

    // Position, die der Eintrag gleich unten in `cfg.entries` erhält – schon
    // hier vermerkt, damit `last_known_position` von Anfang an stimmt und
    // nicht erst auf den nächsten `persist()` warten muss (siehe
    // `ModInfo::last_known_position`).
    let position = cfg.entries.len();

    lib.mods.insert(
        final_name.clone(),
        ModInfo {
            pak: final_name.clone(),
            name: display_name,
            author: None,
            version: None,
            nexus_id: None,
            notes: None,
            hash,
            size,
            imported_at: now_rfc3339(),
            source: source.map(str::to_string),
            // Ein Import trägt immer deaktiviert ans Ende ein (siehe
            // Doc-Kommentar oben) – das ist zugleich der erste bekannte
            // Zustand für `PakConfig::reconcile`s Wiederherstellung.
            last_known_disabled: true,
            last_known_position: position,
            mtime,
        },
    );

    cfg.entries.push(PakEntry { pak: final_name.clone(), disabled: true });

    Ok(ImportOutcome { pak: final_name, duplicate_of: None })
}

/// RFC-3339-Zeitstempel für "jetzt" in UTC, ohne Datums-Crate.
pub fn now_rfc3339() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    format_utc(seconds)
}

/// Formatiert eine Unix-Zeit (Sekunden seit der Epoche, UTC) als RFC-3339.
///
/// Kalenderrechnung nach Howard Hinnants "civil_from_days"-Algorithmus.
pub fn format_utc(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    let remainder = unix.rem_euclid(86_400);
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

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year,
        m,
        d,
        remainder / 3600,
        (remainder % 3600) / 60,
        remainder % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Baut ein Spielverzeichnis, das `GamePaths::from_game_dir` akzeptiert.
    fn game_fixture() -> (tempfile::TempDir, GamePaths) {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("Space Marine 2");
        std::fs::create_dir_all(game.join("client_pc/root/mods")).unwrap();
        let paths = GamePaths::from_game_dir(&game, tmp.path()).unwrap();
        (tmp, paths)
    }

    fn zip_with(files: &[(&str, &[u8])], to: &Path) {
        let file = std::fs::File::create(to).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        for (name, content) in files {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn extracts_paks_from_zip_and_ignores_extras() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("mod.zip");
        zip_with(&[("readme.txt", b"hallo"), ("cool_mod.pak", b"PAKDATEN")], &archive);

        let target = tmp.path().join("out");
        std::fs::create_dir_all(&target).unwrap();
        let paks = extract_paks(&archive, &target).unwrap();

        assert_eq!(paks.len(), 1);
        assert!(paks[0].ends_with("cool_mod.pak"));
        assert_eq!(std::fs::read(&paks[0]).unwrap(), b"PAKDATEN");
    }

    #[test]
    fn extracts_paks_from_subdirectories() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("mod.zip");
        zip_with(&[("Mein Mod/v2/tief.pak", b"DATEN")], &archive);

        let target = tmp.path().join("out");
        std::fs::create_dir_all(&target).unwrap();
        let paks = extract_paks(&archive, &target).unwrap();

        assert_eq!(paks.len(), 1);
        assert!(paks[0].ends_with("tief.pak"), "Verzeichnisstruktur wird abgeflacht");
    }

    #[test]
    fn recognizes_uppercase_pak_extension_inside_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("mod.zip");
        zip_with(&[("GROSS.PAK", b"DATEN")], &archive);

        let target = tmp.path().join("out");
        std::fs::create_dir_all(&target).unwrap();
        let paks = extract_paks(&archive, &target).unwrap();

        assert_eq!(paks.len(), 1);
        assert!(paks[0].ends_with("GROSS.PAK"));
    }

    #[test]
    fn zip_without_pak_is_reported_clearly() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("leer.zip");
        zip_with(&[("readme.txt", b"nichts")], &archive);

        let target = tmp.path().join("out");
        std::fs::create_dir_all(&target).unwrap();

        assert!(matches!(
            extract_paks(&archive, &target).unwrap_err(),
            Error::NoPakInArchive(_)
        ));
    }

    #[test]
    fn zip_slip_paths_are_defended_against() {
        // Ein Archiv darf niemals außerhalb des Zielverzeichnisses schreiben.
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("boese.zip");
        zip_with(&[("../../entkommen.pak", b"DATEN")], &archive);

        let target = tmp.path().join("out");
        std::fs::create_dir_all(&target).unwrap();
        let paks = extract_paks(&archive, &target).unwrap();

        for p in &paks {
            assert!(p.starts_with(&target), "Datei landete außerhalb: {}", p.display());
        }
        assert!(!tmp.path().join("entkommen.pak").exists());
    }

    /// Zwei Einträge mit gleichem Dateinamen aus unterschiedlichen Ordnern
    /// (z. B. zwei Installationsvarianten) müssen beide erhalten bleiben –
    /// der zweite bekommt eine laufende Nummer (siehe Doc-Kommentar von
    /// `extract_paks`), statt den ersten stillschweigend zu verdrängen.
    #[test]
    fn same_basename_from_different_folders_gets_a_numbered_variant() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("mod.zip");
        zip_with(&[("a/mod.pak", b"ERSTER"), ("b/mod.pak", b"ZWEITER")], &archive);

        let target = tmp.path().join("out");
        std::fs::create_dir_all(&target).unwrap();
        let paks = extract_paks(&archive, &target).unwrap();

        assert_eq!(paks.len(), 2, "beide Varianten müssen erhalten bleiben");
        assert!(paks[0].ends_with("mod.pak"));
        assert!(paks[1].ends_with("mod_2.pak"), "der zweite Treffer bekommt eine laufende Nummer");
        assert_eq!(std::fs::read(&paks[0]).unwrap(), b"ERSTER");
        assert_eq!(std::fs::read(&paks[1]).unwrap(), b"ZWEITER");
    }

    /// `collect_paks_recursively` (genutzt von `.7z`/`.rar`) muss dieselbe
    /// Nummerierung anwenden wie der Zip-Pfad, und dabei unabhängig von der
    /// (nicht zugesicherten) `read_dir`-Reihenfolge ein deterministisches
    /// Ergebnis liefern.
    #[test]
    fn collect_paks_recursively_is_deterministic_and_keeps_same_basename_variants() {
        let tmp = tempfile::tempdir().unwrap();
        let raw = tmp.path().join("raw");
        std::fs::create_dir_all(raw.join("a")).unwrap();
        std::fs::create_dir_all(raw.join("b")).unwrap();
        std::fs::create_dir_all(raw.join("deep/x")).unwrap();
        std::fs::write(raw.join("a/mod.pak"), b"ERSTER").unwrap();
        std::fs::write(raw.join("b/mod.pak"), b"ZWEITER").unwrap();
        std::fs::write(raw.join("deep/x/y.pak"), b"TIEF").unwrap();

        let into = tmp.path().join("out");
        std::fs::create_dir_all(&into).unwrap();

        let result = collect_paks_recursively(&raw, &into).unwrap();

        assert_eq!(
            result,
            vec![into.join("mod.pak"), into.join("mod_2.pak"), into.join("y.pak")],
            "Reihenfolge und Benennung müssen deterministisch sein"
        );
        assert_eq!(std::fs::read(into.join("mod.pak")).unwrap(), b"ERSTER");
        assert_eq!(std::fs::read(into.join("mod_2.pak")).unwrap(), b"ZWEITER");
        assert_eq!(std::fs::read(into.join("y.pak")).unwrap(), b"TIEF");
    }

    /// `unar`/`7z` können Symlinks aus einem Archiv wiederherstellen. Ein
    /// Link auf ein fremdes Verzeichnis darf beim Einsammeln der Paks nicht
    /// verfolgt werden, sonst könnte ein bösartiges Archiv über einen Link
    /// auf `/` oder das Spielverzeichnis fremde Dateien einsammeln.
    #[test]
    fn collect_paks_recursively_does_not_follow_symlinked_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let raw = tmp.path().join("raw");
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&raw).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("fremd.pak"), b"FREMD").unwrap();
        std::os::unix::fs::symlink(&outside, raw.join("link")).unwrap();

        let into = tmp.path().join("out");
        std::fs::create_dir_all(&into).unwrap();

        let result = collect_paks_recursively(&raw, &into).unwrap();

        assert!(result.is_empty(), "ein Symlink auf ein fremdes Verzeichnis darf nicht verfolgt werden");
        assert!(!into.join("fremd.pak").exists());
    }

    /// Ein Name, der nicht auf `.pak` endet und dessen letzte Bytes mitten in
    /// einem mehrbyteigen UTF-8-Zeichen liegen würden, darf `strip_pak_suffix`
    /// nicht zum Absturz bringen.
    #[test]
    fn strip_pak_suffix_does_not_panic_on_non_ascii_names_without_pak_suffix() {
        assert_eq!(strip_pak_suffix("a€€"), "a€€");
        assert_eq!(strip_pak_suffix("mod.pak"), "mod");
        assert_eq!(strip_pak_suffix("MOD.PAK"), "MOD");
    }

    #[test]
    fn a_bare_pak_file_is_returned_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let pak = tmp.path().join("direkt.pak");
        std::fs::write(&pak, b"DATEN").unwrap();

        let target = tmp.path().join("out");
        std::fs::create_dir_all(&target).unwrap();
        let paks = extract_paks(&pak, &target).unwrap();

        assert_eq!(paks, vec![pak]);
    }

    /// Eine `.rar`-Datei mit ungültigem Inhalt kann kein reales Werkzeug
    /// erfolgreich entpacken. Das erwartete Ergebnis hängt davon ab, ob die
    /// Testumgebung überhaupt ein Werkzeug bereitstellt: ohne `unar`/`7z`/
    /// `7zz` im PATH ist die richtige Meldung `NoRarTool` ("bitte
    /// installieren"); ist eines vorhanden, aber scheitert es am ungültigen
    /// Archiv, wäre dieselbe Meldung irreführend – dort muss ein
    /// `Error::Io` kommen, das auf das kaputte Archiv hinweist.
    #[test]
    fn invalid_rar_archive_is_reported_clearly() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("kaputt.rar");
        std::fs::write(&archive, b"das ist kein rar-Archiv").unwrap();

        let target = tmp.path().join("out");
        std::fs::create_dir_all(&target).unwrap();

        let any_tool_available =
            ["unar", "7z", "7zz"].iter().any(|name| Current::find_tool(name).is_some());
        let error = extract_paks(&archive, &target).unwrap_err();

        if any_tool_available {
            assert!(
                matches!(error, Error::Io { .. }),
                "ein gefundenes, aber gescheitertes Werkzeug muss als E/A-Fehler gemeldet werden, nicht als NoRarTool: {error:?}"
            );
        } else {
            assert!(matches!(error, Error::NoRarTool));
        }
    }

    #[test]
    fn import_places_pak_and_registers_it_disabled() {
        let (_tmp, paths) = game_fixture();
        let source_dir = tempfile::tempdir().unwrap();
        let pak = source_dir.path().join("neu.pak");
        std::fs::write(&pak, b"PAKDATEN").unwrap();

        let mut lib = Library::default();
        let mut cfg = PakConfig { entries: vec![PakEntry { pak: "alt.pak".into(), disabled: false }] };

        let outcome = import_pak(&paths, &mut lib, &mut cfg, &pak, Some("mod.zip")).unwrap();

        assert_eq!(outcome, ImportOutcome { pak: "neu.pak".into(), duplicate_of: None });
        assert!(paths.mods_dir().join("neu.pak").is_file());
        assert_eq!(cfg.entries.last().unwrap().pak, "neu.pak");
        assert!(cfg.entries.last().unwrap().disabled, "Import darf nichts aktivieren");
        assert_eq!(cfg.entries[0].pak, "alt.pak", "bestehende Einträge bleiben vorn");
        assert_eq!(lib.mods["neu.pak"].source.as_deref(), Some("mod.zip"));
    }

    #[test]
    fn import_detects_content_identical_duplicate() {
        let (_tmp, paths) = game_fixture();
        let source_dir = tempfile::tempdir().unwrap();
        let first = source_dir.path().join("erst.pak");
        let second = source_dir.path().join("nochmal.pak");
        std::fs::write(&first, b"GLEICHER INHALT").unwrap();
        std::fs::write(&second, b"GLEICHER INHALT").unwrap();

        let mut lib = Library::default();
        let mut cfg = PakConfig::default();

        import_pak(&paths, &mut lib, &mut cfg, &first, None).unwrap();
        let outcome = import_pak(&paths, &mut lib, &mut cfg, &second, None).unwrap();

        assert_eq!(outcome.duplicate_of.as_deref(), Some("erst.pak"));
        assert_eq!(cfg.entries.len(), 1, "Dublette wird nicht erneut eingetragen");
    }

    /// Zwei inhaltlich unterschiedliche Paks mit demselben Dateinamen: der
    /// zweite Import darf die bereits vorhandene Datei nicht überschreiben,
    /// auch wenn deren Inhalt der Bibliothek unbekannt ist (z. B. von Hand
    /// abgelegt, nicht importiert).
    #[test]
    fn import_avoids_overwriting_an_untracked_file_with_the_same_name() {
        let (_tmp, paths) = game_fixture();
        std::fs::write(paths.mods_dir().join("mod.pak"), b"BEREITS DA").unwrap();

        let source_dir = tempfile::tempdir().unwrap();
        let pak = source_dir.path().join("mod.pak");
        std::fs::write(&pak, b"NEUER INHALT").unwrap();

        let mut lib = Library::default();
        let mut cfg = PakConfig::default();

        let outcome = import_pak(&paths, &mut lib, &mut cfg, &pak, None).unwrap();

        assert_ne!(outcome.pak, "mod.pak", "Name muss ausweichen, um nichts zu überschreiben");
        assert_eq!(
            std::fs::read(paths.mods_dir().join("mod.pak")).unwrap(),
            b"BEREITS DA",
            "die vorhandene Datei darf nicht verändert werden"
        );
        assert_eq!(std::fs::read(paths.mods_dir().join(&outcome.pak)).unwrap(), b"NEUER INHALT");
        assert_eq!(cfg.entries.last().unwrap().pak, outcome.pak);
    }

    #[test]
    fn formats_unix_time_as_rfc3339() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(1_757_700_000), "2025-09-12T18:00:00Z");
        // Schaltjahr.
        assert_eq!(format_utc(1_709_164_800), "2024-02-29T00:00:00Z");
        // Jahreswechsel, unabhängig geprüft mit `date -u -d @<epoch> +%FT%TZ`.
        assert_eq!(format_utc(1_767_225_599), "2025-12-31T23:59:59Z");
        assert_eq!(format_utc(1_767_225_600), "2026-01-01T00:00:00Z");
    }
}

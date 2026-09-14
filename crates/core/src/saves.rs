//! Sicherung und Wiederherstellung von Savegames.
//!
//! Space-Marine-2-Spielstände liegen in einem Proton-Prefix, den Steam Cloud
//! jederzeit überschreiben kann. Ein Bug hier vernichtet echten Spielstand –
//! deshalb überschreibt `restore` nie, ohne vorher den aktuellen Stand
//! selbst zu sichern, und `verify` prüft jedes Backup Byte für Byte gegen
//! sein Manifest, bevor es benutzt wird.

use crate::atomic::write_atomic;
use crate::error::{Error, Result};
use crate::import::now_rfc3339;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Hash und Größe einer einzelnen gesicherten Datei.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileRecord {
    pub hash: String,
    pub size: u64,
}

/// Begleitet jedes Backup und macht Beschädigung erkennbar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackupManifest {
    pub created_at: String,
    pub source: String,
    #[serde(default)]
    pub label: Option<String>,
    /// Schlüssel ist der Pfad relativ zum Save-Verzeichnis, mit '/' getrennt.
    pub files: BTreeMap<String, FileRecord>,
}

/// Referenz auf ein Backup: Archiv- und Manifest-Datei plus die für
/// `list_backups`/Anzeige nötigen Metadaten.
#[derive(Debug, Clone)]
pub struct BackupEntry {
    pub archive: PathBuf,
    pub manifest: PathBuf,
    pub created_at: String,
    pub label: Option<String>,
}

/// Listet alle Dateien unterhalb von `root` rekursiv auf, als Paare aus
/// Pfad relativ zu `root` (mit '/' getrennt, plattformunabhängig für das
/// Manifest) und dem absoluten Pfad. Das Ergebnis ist nach dem relativen
/// Pfad sortiert, damit ein Backup unabhängig von der (nicht zugesicherten)
/// `read_dir`-Reihenfolge deterministisch ist.
fn list_files_recursive(root: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut collected = Vec::new();
    let mut pending = vec![root.to_path_buf()];

    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|e| Error::io(&dir, e))? {
            let path = entry.map_err(|e| Error::io(&dir, e))?.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| Error::CorruptBackup("Pfad außerhalb des Save-Verzeichnisses".into()))?
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                collected.push((relative, path));
            }
        }
    }
    collected.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(collected)
}

/// Zeitstempel, der sich als Dateiname eignet: 2026-09-12_180000
fn timestamp_for_filename(rfc: &str) -> String {
    rfc.trim_end_matches('Z').replace(':', "").replace('T', "_")
}

/// Findet ein noch unbelegtes Paar aus Archiv- und Manifestnamen.
fn unique_backup_name(backup_root: &Path, base: &str) -> (PathBuf, PathBuf) {
    let mut attempt = 0u32;
    loop {
        let name = if attempt == 0 { base.to_string() } else { format!("{base}-{attempt}") };
        let archive = backup_root.join(format!("{name}.zip"));
        let manifest = backup_root.join(format!("{name}.json"));
        if !archive.exists() && !manifest.exists() {
            return (archive, manifest);
        }
        attempt += 1;
    }
}

/// Macht ein Etikett dateinamentauglich: nur alphanumerische Zeichen und
/// Bindestriche bleiben erhalten, alles andere wird zu '-', führende und
/// abschließende Bindestriche entfallen. Besteht das Etikett nur aus
/// Satzzeichen, ist das Ergebnis leer – der Aufrufer hängt es dann gar
/// nicht erst an den Dateinamen an (siehe `backup`), so dass nie ein
/// leerer oder mit '.' beginnender Namensteil entsteht.
fn sanitize_label(label: &str) -> String {
    label
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Erstellt ein Backup: alle Dateien aus `save_dir` werden in ein neues
/// ZIP-Archiv unter `backup_root` gepackt, begleitet von einem Manifest mit
/// Hash und Größe jeder Datei.
pub fn backup(save_dir: &Path, backup_root: &Path, label: Option<&str>) -> Result<BackupEntry> {
    if !save_dir.is_dir() {
        return Err(Error::io(
            save_dir,
            std::io::Error::new(std::io::ErrorKind::NotFound, "Save-Verzeichnis fehlt"),
        ));
    }
    std::fs::create_dir_all(backup_root).map_err(|e| Error::io(backup_root, e))?;

    let now = now_rfc3339();
    let mut base = timestamp_for_filename(&now);
    if let Some(l) = label {
        let clean = sanitize_label(l);
        if !clean.is_empty() {
            base = format!("{base}_{clean}");
        }
    }

    // Der Zeitstempel hat Sekundenauflösung. Zwei Backups in derselben
    // Sekunde dürfen einander nicht überschreiben – `restore` legt
    // unmittelbar vor dem Lesen eines Archivs eine Sicherung an und würde
    // sonst genau das Archiv zerstören, das es gleich einliest.
    let (archive_path, manifest_path) = unique_backup_name(backup_root, &base);

    let files = list_files_recursive(save_dir)?;
    let mut records = BTreeMap::new();

    let file = std::fs::File::create(&archive_path).map_err(|e| Error::io(&archive_path, e))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    for (relative, absolute) in &files {
        let content = std::fs::read(absolute).map_err(|e| Error::io(absolute, e))?;
        zip.start_file(relative, opts).map_err(|e| {
            Error::io(&archive_path, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        zip.write_all(&content).map_err(|e| Error::io(&archive_path, e))?;

        records.insert(
            relative.clone(),
            FileRecord { hash: blake3::hash(&content).to_hex().to_string(), size: content.len() as u64 },
        );
    }
    zip.finish().map_err(|e| {
        Error::io(&archive_path, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    })?;

    let manifest = BackupManifest {
        created_at: now.clone(),
        source: save_dir.display().to_string(),
        label: label.map(str::to_string),
        files: records,
    };
    // Unerreichbar für die aktuellen Feldtypen (String, Option<_>, u64,
    // BTreeMap<String, _> können nicht fehlschlagen); der rohe Fehler wird
    // bewusst verworfen, damit die Meldung rein deutsch bleibt (vgl.
    // `Library::save`).
    let json = serde_json::to_string_pretty(&manifest).map_err(|_| {
        Error::io(
            &manifest_path,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Manifest konnte nicht als JSON serialisiert werden",
            ),
        )
    })?;
    write_atomic(&manifest_path, &format!("{json}\n"))?;

    Ok(BackupEntry {
        archive: archive_path,
        manifest: manifest_path,
        created_at: now,
        label: label.map(str::to_string),
    })
}

fn read_manifest(entry: &BackupEntry) -> Result<BackupManifest> {
    let text = std::fs::read_to_string(&entry.manifest).map_err(|e| Error::io(&entry.manifest, e))?;
    serde_json::from_str(&text).map_err(|e| {
        Error::io(
            &entry.manifest,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("ungültiges Manifest (Zeile {}, Spalte {})", e.line(), e.column()),
            ),
        )
    })
}

/// Prüft jede Datei im Archiv gegen den Hash im Manifest: das Archiv muss
/// sich als ZIP öffnen lassen, jeder Eintrag muss im Manifest stehen, darf
/// dort nur einmal auftauchen (sonst könnte ein doppelter Eintrag eine im
/// Archiv fehlende Datei vortäuschen) und sein Hash muss übereinstimmen;
/// am Ende muss die Anzahl geprüfter Einträge exakt der erwarteten
/// entsprechen.
pub fn verify(entry: &BackupEntry) -> Result<()> {
    let manifest = read_manifest(entry)?;

    let file = std::fs::File::open(&entry.archive).map_err(|e| Error::io(&entry.archive, e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| {
        Error::io(&entry.archive, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    })?;

    let mut seen = std::collections::BTreeSet::new();
    for i in 0..zip.len() {
        let mut zip_entry = zip.by_index(i).map_err(|e| {
            Error::io(&entry.archive, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        if !zip_entry.is_file() {
            continue;
        }
        let name = zip_entry.name().to_string();
        let expected = manifest
            .files
            .get(&name)
            .ok_or_else(|| Error::CorruptBackup(format!("{name} steht nicht im Manifest")))?;

        if !seen.insert(name.clone()) {
            return Err(Error::CorruptBackup(format!("{name} kommt mehrfach im Archiv vor")));
        }

        let mut content = Vec::new();
        std::io::copy(&mut zip_entry, &mut content).map_err(|e| Error::io(&entry.archive, e))?;

        let actual = blake3::hash(&content).to_hex().to_string();
        if actual != expected.hash {
            return Err(Error::CorruptBackup(format!("{name} hat einen abweichenden Hash")));
        }
    }

    if seen.len() != manifest.files.len() {
        return Err(Error::CorruptBackup(format!(
            "Archiv enthält {} von {} erwarteten Dateien",
            seen.len(),
            manifest.files.len()
        )));
    }
    Ok(())
}

/// Alle Backups unter `backup_root`, neueste zuerst. Ein `.zip` ohne
/// begleitendes `.json` (oder umgekehrt) wird stillschweigend übergangen –
/// es ist kein von dieser Funktion angelegtes Backup.
pub fn list_backups(backup_root: &Path) -> Result<Vec<BackupEntry>> {
    if !backup_root.is_dir() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    for dir_entry in std::fs::read_dir(backup_root).map_err(|e| Error::io(backup_root, e))? {
        let path = dir_entry.map_err(|e| Error::io(backup_root, e))?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("zip") {
            continue;
        }
        let manifest_path = path.with_extension("json");
        if !manifest_path.is_file() {
            continue;
        }
        let candidate = BackupEntry {
            archive: path,
            manifest: manifest_path,
            created_at: String::new(),
            label: None,
        };
        if let Ok(m) = read_manifest(&candidate) {
            entries.push(BackupEntry { created_at: m.created_at, label: m.label, ..candidate });
        }
    }

    // Neueste zuerst. Bei gleicher Sekunde entscheidet der Dateiname, damit
    // die Reihenfolge nicht von der Verzeichnisreihenfolge abhängt.
    entries.sort_by(|a, b| b.created_at.cmp(&a.created_at).then_with(|| b.archive.cmp(&a.archive)));
    Ok(entries)
}

/// Stellt ein Backup wieder her. Legt **immer zuerst**, bevor das
/// wiederherzustellende Archiv gelesen wird, ein Sicherungs-Backup des
/// aktuellen Standes an und gibt dieses zurück – so ist ein Überschreiben
/// nie folgenlos.
///
/// `restore` überschreibt nur Dateien, die im Archiv enthalten sind.
/// Dateien, die in `save_dir` liegen, aber nicht im Archiv, bleiben
/// unangetastet – gelöscht wird nie. Ein Rückgängigmachen über die
/// Sicherung wäre bei zusätzlich gelöschten Dateien nicht mehr vollständig
/// möglich; das Risiko eines liegen gebliebenen Fremdlings in einem
/// Proton-Prefix wiegt dagegen gering.
pub fn restore(entry: &BackupEntry, save_dir: &Path, backup_root: &Path) -> Result<BackupEntry> {
    verify(entry)?;

    // Sicherung zuerst: unmittelbar danach wird das Archiv aus `entry`
    // gelesen. Läge die Sicherung zeitlich danach, könnte sie – bei einer
    // Wiederherstellung aus einem soeben selbst erzeugten Backup innerhalb
    // derselben Sekunde – genau dieses Archiv überschreiben, bevor es
    // fertig gelesen ist. `unique_backup_name` sorgt zusätzlich dafür, dass
    // sich zwei Backups nie einen Dateinamen teilen.
    let safety_backup = backup(save_dir, backup_root, Some("vor Wiederherstellung"))?;

    let file = std::fs::File::open(&entry.archive).map_err(|e| Error::io(&entry.archive, e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| {
        Error::io(&entry.archive, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    })?;

    for i in 0..zip.len() {
        let mut zip_entry = zip.by_index(i).map_err(|e| {
            Error::io(&entry.archive, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        if !zip_entry.is_file() {
            continue;
        }

        let name = zip_entry.name().to_string();
        // Nur einzelne Namensteile ohne '.', '..' oder Leerteile werden
        // übernommen (Zip-Slip-Schutz). Ein Name wie "/etc/passwd" zerfällt
        // beim Aufteilen an '/' in ein leeres erstes Element und wird damit
        // abgelehnt; ein Name wie "../../etwas" ebenso über die
        // ".."-Prüfung. Rückwärtsschrägstriche sind auf Unix Teil des
        // Dateinamens und kein Trennzeichen – dieses Crate unterstützt
        // ausschließlich Linux/Proton, ein solcher Name landet also
        // höchstens als (harmloser) buchstäblicher Dateiname in `save_dir`.
        let mut target = save_dir.to_path_buf();
        for component in name.split('/') {
            if component.is_empty() || component == "." || component == ".." {
                return Err(Error::CorruptBackup(format!("unzulässiger Pfad im Archiv: {name}")));
            }
            target.push(component);
        }

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        let mut out_file = std::fs::File::create(&target).map_err(|e| Error::io(&target, e))?;
        std::io::copy(&mut zip_entry, &mut out_file).map_err(|e| Error::io(&target, e))?;
    }

    Ok(safety_backup)
}

/// Prüft, ob der Steam-Client aktuell läuft. Cloud-Synchronisation kann
/// eine Wiederherstellung im Hintergrund überschreiben – das ist der
/// wahrscheinlichste Weg zu Datenverlust bei dieser Funktion.
///
/// Erkannt wird ausschließlich ein Prozess, dessen `/proc/<pid>/comm` exakt
/// `steam` lautet. Hilfsprozesse wie `steamwebhelper` zählen bewusst
/// nicht: sie sind Browser-Unterprozesse ohne eigene
/// Cloud-Synchronisation, und ein Treffer allein auf "enthält steam" würde
/// bei jedem `steamwebhelper` oder `steamerrorreporter` anschlagen und die
/// Warnung wertlos machen. Fehlt `/proc` (z. B. auf einem System ohne
/// procfs), wird `false` zurückgegeben statt eines Fehlers – die Prüfung
/// ist eine Vorsichtsmaßnahme, kein hartes Erfordernis.
pub fn steam_is_running() -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    for entry in entries.filter_map(std::result::Result::ok) {
        let comm = entry.path().join("comm");
        if let Ok(name) = std::fs::read_to_string(&comm) {
            if name.trim() == "steam" {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn save_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let saves = tmp.path().join("Main");
        std::fs::create_dir_all(saves.join("slot1")).unwrap();
        std::fs::write(saves.join("profile.sav"), b"PROFILDATEN").unwrap();
        std::fs::write(saves.join("slot1/campaign.sav"), b"KAMPAGNE").unwrap();
        let backups = tmp.path().join("backups");
        (tmp, saves, backups)
    }

    #[test]
    fn backup_creates_archive_and_manifest() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, Some("vor Chaplain")).unwrap();

        assert!(entry.archive.is_file());
        assert!(entry.manifest.is_file());
        assert_eq!(entry.label.as_deref(), Some("vor Chaplain"));

        let manifest: BackupManifest =
            serde_json::from_str(&std::fs::read_to_string(&entry.manifest).unwrap()).unwrap();
        assert_eq!(manifest.files.len(), 2);
        assert!(manifest.files.contains_key("profile.sav"));
        assert!(manifest.files.contains_key("slot1/campaign.sav"));
    }

    #[test]
    fn verify_accepts_intact_backup() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, None).unwrap();
        verify(&entry).unwrap();
    }

    #[test]
    fn verify_rejects_manipulated_archive() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, None).unwrap();
        std::fs::write(&entry.archive, b"kaputt").unwrap();

        // Ein Archiv, das sich nicht mehr als ZIP öffnen lässt, ist ein
        // E/A-Fehler mit der zip-Bibliotheksmeldung als `source` – kein
        // `CorruptBackup`, denn dieser Fehlertyp trägt nur selbst
        // formulierte deutsche Sätze (siehe Doc-Kommentar von `verify`).
        assert!(matches!(verify(&entry).unwrap_err(), Error::Io { .. }));
    }

    /// Minimaler CRC-32 (bit-reflektiert, Standardpolynom 0xEDB88320) ohne
    /// zusätzliche Abhängigkeit – nur zum Bau eines von Hand
    /// zusammengesetzten ZIP-Archivs in Tests gebraucht.
    fn crc32(data: &[u8]) -> u32 {
        let mut crc: u32 = 0xFFFF_FFFF;
        for &byte in data {
            crc ^= byte as u32;
            for _ in 0..8 {
                let mask = (!(crc & 1)).wrapping_add(1);
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
        !crc
    }

    /// Baut von Hand ein ungültiges, aber lesbares ZIP-Archiv mit zwei
    /// Einträgen desselben Namens (Store, unkomprimiert). Die `zip`-Crate
    /// verweigert das über `ZipWriter` (siehe `InvalidArchive("Duplicate
    /// filename")`) – ein von Hand erzeugtes Archiv (oder eines aus einem
    /// anderen Werkzeug, das diese Prüfung nicht kennt) kann so etwas aber
    /// enthalten, und `ZipArchive::by_index` liest Einträge positionsbasiert,
    /// nicht namensbasiert, also klaglos.
    fn write_zip_with_duplicate_entry(path: &Path, name: &str, content: &[u8]) {
        let mut bytes = Vec::new();
        let mut local_offsets = Vec::new();
        let name_bytes = name.as_bytes();
        let crc = crc32(content);

        for _ in 0..2 {
            local_offsets.push(bytes.len() as u32);
            bytes.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            bytes.extend_from_slice(&20u16.to_le_bytes()); // version needed
            bytes.extend_from_slice(&0u16.to_le_bytes()); // flags
            bytes.extend_from_slice(&0u16.to_le_bytes()); // compression: store
            bytes.extend_from_slice(&0u16.to_le_bytes()); // mod time
            bytes.extend_from_slice(&0u16.to_le_bytes()); // mod date
            bytes.extend_from_slice(&crc.to_le_bytes());
            bytes.extend_from_slice(&(content.len() as u32).to_le_bytes()); // compressed size
            bytes.extend_from_slice(&(content.len() as u32).to_le_bytes()); // uncompressed size
            bytes.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            bytes.extend_from_slice(&0u16.to_le_bytes()); // extra field length
            bytes.extend_from_slice(name_bytes);
            bytes.extend_from_slice(content);
        }

        let central_dir_start = bytes.len() as u32;
        for &offset in &local_offsets {
            bytes.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            bytes.extend_from_slice(&20u16.to_le_bytes()); // version made by
            bytes.extend_from_slice(&20u16.to_le_bytes()); // version needed
            bytes.extend_from_slice(&0u16.to_le_bytes()); // flags
            bytes.extend_from_slice(&0u16.to_le_bytes()); // compression
            bytes.extend_from_slice(&0u16.to_le_bytes()); // mod time
            bytes.extend_from_slice(&0u16.to_le_bytes()); // mod date
            bytes.extend_from_slice(&crc.to_le_bytes());
            bytes.extend_from_slice(&(content.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&(content.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            bytes.extend_from_slice(&0u16.to_le_bytes()); // extra field length
            bytes.extend_from_slice(&0u16.to_le_bytes()); // comment length
            bytes.extend_from_slice(&0u16.to_le_bytes()); // disk number start
            bytes.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
            bytes.extend_from_slice(&0u32.to_le_bytes()); // external attrs
            bytes.extend_from_slice(&offset.to_le_bytes());
            bytes.extend_from_slice(name_bytes);
        }
        let central_dir_size = bytes.len() as u32 - central_dir_start;

        bytes.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes()); // disk number
        bytes.extend_from_slice(&0u16.to_le_bytes()); // disk with central dir
        bytes.extend_from_slice(&(local_offsets.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&(local_offsets.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&central_dir_size.to_le_bytes());
        bytes.extend_from_slice(&central_dir_start.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes()); // comment length

        std::fs::write(path, bytes).unwrap();
    }

    /// Ein Archiv-Eintrag, der doppelt unter demselben Namen auftaucht,
    /// darf eine tatsächlich fehlende Datei nicht verdecken: würde nur die
    /// Anzahl verarbeiteter Einträge gezählt, käme man trotz einer im
    /// Manifest stehenden, im Archiv aber fehlenden Datei auf die richtige
    /// Gesamtzahl.
    #[test]
    fn verify_rejects_duplicate_entry_masking_a_missing_file() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, None).unwrap();

        // Von Hand ein Archiv bauen: "profile.sav" taucht zweimal auf,
        // "slot1/campaign.sav" (im Manifest gefordert) fehlt ganz.
        write_zip_with_duplicate_entry(&entry.archive, "profile.sav", b"PROFILDATEN");

        assert!(
            matches!(verify(&entry).unwrap_err(), Error::CorruptBackup(_)),
            "ein doppelter Eintrag darf eine fehlende Datei nicht verdecken"
        );
    }

    #[test]
    fn restore_restores_content_bit_for_bit() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, None).unwrap();

        std::fs::write(saves.join("profile.sav"), b"KAPUTTGESPIELT").unwrap();
        std::fs::remove_file(saves.join("slot1/campaign.sav")).unwrap();

        restore(&entry, &saves, &backups).unwrap();

        assert_eq!(std::fs::read(saves.join("profile.sav")).unwrap(), b"PROFILDATEN");
        assert_eq!(std::fs::read(saves.join("slot1/campaign.sav")).unwrap(), b"KAMPAGNE");
    }

    /// `restore` darf niemals Dateien löschen, die im Archiv nicht
    /// enthalten sind – nur die im Archiv enthaltenen Dateien werden
    /// überschrieben (siehe Doc-Kommentar von `restore`).
    #[test]
    fn restore_does_not_delete_files_absent_from_the_archive() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, None).unwrap();

        std::fs::write(saves.join("slot1/nicht_gesichert.sav"), b"NEU ANGELEGT").unwrap();

        restore(&entry, &saves, &backups).unwrap();

        assert_eq!(
            std::fs::read(saves.join("slot1/nicht_gesichert.sav")).unwrap(),
            b"NEU ANGELEGT",
            "restore darf nur überschreiben, nicht löschen"
        );
    }

    #[test]
    fn restore_always_backs_up_the_current_state_first() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, None).unwrap();
        std::fs::write(saves.join("profile.sav"), b"NEUER FORTSCHRITT").unwrap();

        let safety_backup = restore(&entry, &saves, &backups).unwrap();

        verify(&safety_backup).unwrap();
        assert_eq!(safety_backup.label.as_deref(), Some("vor Wiederherstellung"));

        // Der überschriebene Fortschritt ist aus der Sicherung wiederholbar.
        restore(&safety_backup, &saves, &backups).unwrap();
        assert_eq!(std::fs::read(saves.join("profile.sav")).unwrap(), b"NEUER FORTSCHRITT");
    }

    #[test]
    fn two_backups_in_the_same_second_do_not_overwrite_each_other() {
        let (_tmp, saves, backups) = save_fixture();

        let first = backup(&saves, &backups, Some("gleich")).unwrap();
        std::fs::write(saves.join("profile.sav"), b"SPAETER").unwrap();
        let second = backup(&saves, &backups, Some("gleich")).unwrap();

        assert_ne!(first.archive, second.archive, "Namenskollision innerhalb einer Sekunde");
        verify(&first).unwrap();
        verify(&second).unwrap();
    }

    #[test]
    fn restoring_twice_does_not_destroy_any_archive() {
        // restore() sichert vor dem Lesen – die Sicherung darf das zu
        // lesende Archiv niemals überschreiben.
        let (_tmp, saves, backups) = save_fixture();
        let original = backup(&saves, &backups, None).unwrap();

        std::fs::write(saves.join("profile.sav"), b"ZWISCHENSTAND").unwrap();
        let safety_backup = restore(&original, &saves, &backups).unwrap();
        assert_eq!(std::fs::read(saves.join("profile.sav")).unwrap(), b"PROFILDATEN");

        restore(&safety_backup, &saves, &backups).unwrap();
        assert_eq!(std::fs::read(saves.join("profile.sav")).unwrap(), b"ZWISCHENSTAND");
    }

    #[test]
    fn list_backups_returns_newest_first() {
        let (_tmp, saves, backups) = save_fixture();
        let first = backup(&saves, &backups, Some("a")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let second = backup(&saves, &backups, Some("b")).unwrap();

        let list = list_backups(&backups).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].archive, second.archive);
        assert_eq!(list[1].archive, first.archive);
    }

    /// Ein `.zip` ohne begleitendes `.json` (z. B. weil das Manifest von
    /// Hand gelöscht wurde) ist kein gültiges Backup und darf die Liste
    /// nicht mit einem kaputten Eintrag füllen.
    #[test]
    fn list_backups_skips_a_zip_without_its_manifest() {
        let (_tmp, saves, backups) = save_fixture();
        backup(&saves, &backups, None).unwrap();
        std::fs::create_dir_all(&backups).unwrap();
        std::fs::write(backups.join("verwaist.zip"), b"egal").unwrap();

        let list = list_backups(&backups).unwrap();
        assert_eq!(list.len(), 1, "das Archiv ohne Manifest darf nicht auftauchen");
    }

    /// Ein Backup eines leeren Save-Verzeichnisses muss ein gültiges, leeres
    /// Archiv samt Manifest erzeugen, und `restore` davon darf nicht
    /// scheitern (siehe Doc-Kommentar: es gibt schlicht nichts zu
    /// überschreiben).
    #[test]
    fn backup_and_restore_handle_an_empty_save_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let saves = tmp.path().join("Main");
        std::fs::create_dir_all(&saves).unwrap();
        let backups = tmp.path().join("backups");

        let entry = backup(&saves, &backups, None).unwrap();
        assert!(entry.archive.is_file());
        let manifest: BackupManifest =
            serde_json::from_str(&std::fs::read_to_string(&entry.manifest).unwrap()).unwrap();
        assert!(manifest.files.is_empty());

        verify(&entry).unwrap();
        restore(&entry, &saves, &backups).unwrap();
    }

    /// Ein Etikett, das nur aus Satzzeichen besteht, darf weder einen
    /// leeren Namensteil noch einen mit '.' beginnenden Dateinamen
    /// erzeugen.
    #[test]
    fn label_of_only_punctuation_yields_a_clean_filename() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, Some("!!!")).unwrap();

        let stem = entry.archive.file_stem().unwrap().to_str().unwrap();
        assert!(!stem.is_empty());
        assert!(!stem.starts_with('.'));
        assert!(!stem.ends_with('_'), "ein leeres Etikett darf keinen toten Trenner hinterlassen");
    }

    #[test]
    fn backup_with_missing_save_dir_reports_an_io_error() {
        let tmp = tempfile::tempdir().unwrap();
        let saves = tmp.path().join("gibt_es_nicht");
        let backups = tmp.path().join("backups");

        assert!(matches!(backup(&saves, &backups, None).unwrap_err(), Error::Io { .. }));
    }

    /// Ein von Hand präpariertes Archiv, dessen Eintragsname aus dem
    /// Save-Verzeichnis hinausführen würde, muss auch dann abgelehnt
    /// werden, wenn das Manifest (das im selben Backup-Ordner liegt und
    /// damit ebenso manipulierbar ist) exakt dazu passt.
    #[test]
    fn restore_rejects_path_traversal_in_archive_entries() {
        let (_tmp, saves, backups) = save_fixture();
        std::fs::create_dir_all(&backups).unwrap();

        let archive_path = backups.join("boese.zip");
        let manifest_path = backups.join("boese.json");

        let file = std::fs::File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        zip.start_file("../entkommen.sav", opts).unwrap();
        zip.write_all(b"BOESARTIG").unwrap();
        zip.finish().unwrap();

        let mut files = BTreeMap::new();
        files.insert(
            "../entkommen.sav".to_string(),
            FileRecord { hash: blake3::hash(b"BOESARTIG").to_hex().to_string(), size: 9 },
        );
        let manifest = BackupManifest {
            created_at: now_rfc3339(),
            source: saves.display().to_string(),
            label: None,
            files,
        };
        std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();

        let entry = BackupEntry {
            archive: archive_path,
            manifest: manifest_path,
            created_at: manifest.created_at.clone(),
            label: None,
        };

        // verify() akzeptiert den Eintrag (Name und Hash stimmen überein) –
        // erst restore() prüft die Pfadkomponenten und muss ablehnen.
        verify(&entry).unwrap();
        let err = restore(&entry, &saves, &backups).unwrap_err();
        assert!(matches!(err, Error::CorruptBackup(_)));
        assert!(!tmp_parent(&saves).join("entkommen.sav").exists());
    }

    fn tmp_parent(saves: &Path) -> PathBuf {
        saves.parent().unwrap().to_path_buf()
    }

    /// Reiner Rauchtest: unabhängig davon, ob `/proc` existiert oder ein
    /// Steam-Prozess läuft, darf der Aufruf nicht abstürzen. Das
    /// tatsächliche Erkennungsverhalten hängt vom laufenden System ab und
    /// lässt sich ohne Prozess-Mocking nicht deterministisch prüfen.
    #[test]
    fn steam_is_running_does_not_panic() {
        let _ = steam_is_running();
    }
}

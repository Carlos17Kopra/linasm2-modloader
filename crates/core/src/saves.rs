//! Sicherung und Wiederherstellung von Savegames.
//!
//! Space-Marine-2-Spielstände liegen in einem Proton-Prefix, den Steam Cloud
//! jederzeit überschreiben kann. Ein Bug hier vernichtet echten Spielstand –
//! deshalb überschreibt `restore` nie, ohne vorher den aktuellen Stand
//! selbst zu sichern und diese Sicherung selbst zu verifizieren, und
//! `verify` prüft jedes Backup Byte für Byte gegen sein Manifest, bevor es
//! benutzt wird. Geschrieben wird durchweg fsync-vor-rename (Archiv wie
//! wiederhergestellte Dateien), Symlinks innerhalb des Save-Verzeichnisses
//! werden nie verfolgt.

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
///
/// Symlinks werden übersprungen statt verfolgt – dieselbe Regel wie beim
/// Einsammeln entpackter Paks in `import.rs`. Ohne sie würde ein
/// Symlink-Zyklus unterhalb von `save_dir` diese Funktion (und damit auch
/// `restore`, das `backup` immer zuerst aufruft) endlos rekursieren lassen;
/// ein Link auf ein fremdes Verzeichnis würde außerdem dessen Inhalt mit
/// ins Backup einsammeln.
///
/// Jeder relative Pfad wird zusätzlich mit `validate_entry_name` geprüft
/// (leere/`.`/`..`-Komponenten, Rückwärtsschrägstriche): ohne diese Prüfung
/// könnte `backup` ein Archiv erzeugen, das `verify` – und damit jede
/// spätere `restore`, die ihre eigene Sicherung verifiziert – als
/// beschädigt zurückweist, etwa wegen einer echten Datei namens
/// `slot1\campaign.sav` (auf Unix ein gewöhnlicher, gültiger Dateiname).
fn list_files_recursive(root: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut collected = Vec::new();
    let mut pending = vec![root.to_path_buf()];

    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|e| Error::io(&dir, e))? {
            let entry = entry.map_err(|e| Error::io(&dir, e))?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|e| Error::io(&path, e))?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| Error::CorruptBackup("Pfad außerhalb des Save-Verzeichnisses".into()))?
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                validate_entry_name(&relative)?;
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

/// Findet ein noch unbelegtes Paar aus Archiv- und Manifestnamen. Kollidiert
/// `base` mit einem vorhandenen Backup, wird `~<Zähler>` angehängt. '~' ist
/// als Trennzeichen sicher, weil weder der Zeitstempel noch `sanitize_label`
/// dieses Zeichen je erzeugen (anders als '-', das aus einem Etikett wie
/// "Kapitel-3" stammen könnte) – `collision_counter` kann den Zähler damit
/// eindeutig zurückgewinnen, ohne ihn mit Bindestrichen aus einem Etikett zu
/// verwechseln.
fn unique_backup_name(backup_root: &Path, base: &str) -> (PathBuf, PathBuf) {
    let mut attempt = 0u32;
    loop {
        let name = if attempt == 0 { base.to_string() } else { format!("{base}~{attempt}") };
        let archive = backup_root.join(format!("{name}.zip"));
        let manifest = backup_root.join(format!("{name}.json"));
        if !archive.exists() && !manifest.exists() {
            return (archive, manifest);
        }
        attempt += 1;
    }
}

/// Liest den von `unique_backup_name` angehängten Kollisionszähler aus
/// einem Dateinamensstamm zurück (0, wenn keiner angehängt wurde). Dient
/// `list_backups` als Tie-Break für Backups mit identischem `created_at`:
/// ein reiner Byte-Vergleich der Dateinamen wäre hier falsch, weil '-'
/// (0x2D) vor '.' (0x2E) sortiert und "basis-1.zip" damit lexikografisch
/// vor "basis.zip" läge, obwohl "basis.zip" zuerst angelegt wurde.
fn collision_counter(stem: &str) -> u32 {
    stem.rsplit_once('~').and_then(|(_, suffix)| suffix.parse().ok()).unwrap_or(0)
}

/// Öffnet ein Verzeichnis nur, um `sync_all` darauf aufzurufen. Auf Unix
/// erzwingt das, dass ein neuer Verzeichniseintrag (hier: die frisch
/// geschriebene Archivdatei) das Blockgerät tatsächlich erreicht hat.
fn sync_dir(dir: &Path) -> Result<()> {
    std::fs::File::open(dir).and_then(|f| f.sync_all()).map_err(|e| Error::io(dir, e))
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
    let archive_file = zip.finish().map_err(|e| {
        Error::io(&archive_path, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    })?;
    // Das Manifest beschreibt genau diese Archivdatei und wird gleich
    // atomar geschrieben (siehe `write_atomic` unten). Ohne diesen fsync
    // (und den des Verzeichnisses) könnte ein Absturz kurz danach ein
    // dauerhaftes Manifest hinterlassen, dessen Archiv seine Bytes nie auf
    // die Platte geschafft hat – dann wäre auch die Sicherung, die
    // `restore` aus genau diesem Aufruf erhält, wertlos.
    archive_file.sync_all().map_err(|e| Error::io(&archive_path, e))?;
    drop(archive_file);
    sync_dir(backup_root)?;

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

/// Prüft einen Eintragsnamen aus Manifest oder Archiv gegen Zip-Slip: leere,
/// `.`- oder `..`-Komponenten sowie Rückwärtsschrägstriche (auf manchen
/// Werkzeugen ein Trennzeichen) sind unzulässig. Ein Name, der mit '/'
/// beginnt (absoluter Pfad), zerfällt beim Aufteilen in eine leere erste
/// Komponente und wird darüber ebenfalls abgelehnt.
fn validate_entry_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::CorruptBackup("unzulässiger (leerer) Pfad im Archiv".into()));
    }
    for component in name.split('/') {
        if component.is_empty() || component == "." || component == ".." || component.contains('\\') {
            return Err(Error::CorruptBackup(format!("unzulässiger Pfad im Archiv: {name}")));
        }
    }
    Ok(())
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

/// Prüft jede Datei im Archiv gegen Hash und Größe im Manifest: das Archiv
/// muss sich als ZIP öffnen lassen, jeder Name (im Manifest wie im Archiv)
/// muss ein zulässiger relativer Pfad sein (siehe `validate_entry_name`),
/// jeder Eintrag muss im Manifest stehen, darf dort nur einmal auftauchen
/// (sonst könnte ein doppelter Eintrag eine im Archiv fehlende Datei
/// vortäuschen), und Größe wie Hash müssen übereinstimmen; am Ende muss die
/// Anzahl geprüfter Einträge exakt der erwarteten entsprechen.
///
/// Die Pfadprüfung läuft hier und nicht erst in `restore`: ein Aufrufer,
/// der `verify` benutzt, um "Backup ist in Ordnung" zu melden, darf ein
/// Archiv mit einem hinausführenden Eintragsnamen nicht grün lackieren.
pub fn verify(entry: &BackupEntry) -> Result<()> {
    let manifest = read_manifest(entry)?;
    for name in manifest.files.keys() {
        validate_entry_name(name)?;
    }

    let file = std::fs::File::open(&entry.archive).map_err(|e| Error::io(&entry.archive, e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| {
        Error::CorruptBackup(format!("Archiv lässt sich nicht als ZIP öffnen: {}", entry.archive.display()))
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
        validate_entry_name(&name)?;
        let expected = manifest
            .files
            .get(&name)
            .ok_or_else(|| Error::CorruptBackup(format!("{name} steht nicht im Manifest")))?;

        if !seen.insert(name.clone()) {
            return Err(Error::CorruptBackup(format!("{name} kommt mehrfach im Archiv vor")));
        }

        let mut content = Vec::new();
        std::io::copy(&mut zip_entry, &mut content).map_err(|e| Error::io(&entry.archive, e))?;

        if content.len() as u64 != expected.size {
            return Err(Error::CorruptBackup(format!("{name} hat eine abweichende Größe")));
        }
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

    // Neueste zuerst. Bei gleicher Sekunde entscheidet zuerst der
    // Kollisionszähler aus dem Dateinamen (siehe `collision_counter`) – ein
    // reiner Byte-Vergleich der Pfade wäre hier falsch (siehe dessen
    // Doc-Kommentar). Zwei Backups mit gleichem `created_at`, aber
    // unterschiedlicher Basis (z. B. verschiedenes Etikett, also Zähler
    // 0 auf beiden Seiten) fielen ohne einen letzten, expliziten Tie-Break
    // sonst auf die (nicht zugesicherte) `read_dir`-Reihenfolge zurück –
    // der abschließende Pfadvergleich macht das Ergebnis in jedem Fall
    // deterministisch.
    entries.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| {
                let stem_of = |entry: &BackupEntry| {
                    entry.archive.file_stem().and_then(|s| s.to_str()).map(collision_counter).unwrap_or(0)
                };
                stem_of(b).cmp(&stem_of(a))
            })
            .then_with(|| a.archive.cmp(&b.archive))
    });
    Ok(entries)
}

/// Löst einen (bereits über `validate_entry_name` geprüften) Eintragsnamen
/// zu einem Zielpfad unter `save_dir` auf.
fn resolve_target_path(save_dir: &Path, name: &str) -> PathBuf {
    let mut target = save_dir.to_path_buf();
    for component in name.split('/') {
        target.push(component);
    }
    target
}

/// Löst einen Archiv-Eintragsnamen zu einem Zielpfad unter `save_dir` auf
/// und lehnt ihn ab, wenn dabei ein bereits vorhandener Pfadanteil
/// (Zwischenverzeichnis oder Zieldatei selbst) ein Symlink ist.
/// `create_dir_all`/das Anlegen einer temporären Datei würden einem solchen
/// Symlink sonst folgen und könnten außerhalb von `save_dir` landen – etwa
/// wenn `slot1` durch einen Link auf `~/.config` ersetzt wurde. Da dieser
/// Pfad Komponente für Komponente mit `symlink_metadata` (statt `metadata`,
/// das folgen würde) geprüft wird, bevor irgendetwas geschrieben wird, kann
/// das nicht mehr passieren.
fn resolve_and_check_target(save_dir: &Path, name: &str) -> Result<PathBuf> {
    validate_entry_name(name)?;
    let target = resolve_target_path(save_dir, name);

    let relative =
        target.strip_prefix(save_dir).expect("target liegt laut resolve_target_path immer unter save_dir");
    let mut probe = save_dir.to_path_buf();
    for component in relative.components() {
        probe.push(component);
        if let Ok(metadata) = std::fs::symlink_metadata(&probe) {
            if metadata.file_type().is_symlink() {
                return Err(Error::UnsafeSaveDir(probe));
            }
        }
    }
    Ok(target)
}

/// Schreibt `content` atomar nach `path`: temporäre Datei im selben
/// Verzeichnis, fsync, dann rename (auf POSIX atomar) – dasselbe Muster wie
/// `atomic::write_atomic`, nur für binäre statt textuelle Inhalte.
/// `write_atomic` selbst bleibt bewusst auf `&str` beschränkt
/// (Konfigurations-/Manifestdateien); diese lokale Variante deckt die aus
/// dem Archiv wiederhergestellten, beliebigen Binärdaten ab, ohne jene
/// Signatur aufzuweiten. Ein Absturz mitten in `write_all` (z. B. volle
/// Platte) hinterlässt so nie eine abgeschnittene Save-Datei – nur die
/// verworfene temporäre Datei.
fn write_atomic_bytes(path: &Path, content: &[u8]) -> Result<()> {
    let dir = path.parent().ok_or_else(|| {
        Error::io(path, std::io::Error::new(std::io::ErrorKind::InvalidInput, "Pfad hat kein Elternverzeichnis"))
    })?;
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;

    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|e| Error::io(dir, e))?;
    tmp.write_all(content).map_err(|e| Error::io(path, e))?;
    tmp.as_file().sync_all().map_err(|e| Error::io(path, e))?;
    tmp.persist(path).map_err(|e| Error::io(path, e.error))?;
    Ok(())
}

/// Der eigentliche Wiederherstellungsvorgang, nachdem die Sicherung des
/// aktuellen Standes bereits angelegt ist. In eigener Funktion, damit
/// `restore` jeden hier auftretenden Fehler mit dem Pfad dieser Sicherung
/// anreichern kann (siehe `Error::RestoreFailedAfterBackup`).
fn restore_after_safety_backup(entry: &BackupEntry, save_dir: &Path, safety_backup: &BackupEntry) -> Result<()> {
    // Die Sicherung wird nicht blind vertraut: erst verifizieren, dann
    // riskieren. Ohne diese Prüfung könnte ein Crash mitten im
    // Überschreiben unten die einzige Rückfallebene als (unbemerkt)
    // beschädigt zurücklassen.
    verify(safety_backup)?;

    let file = std::fs::File::open(&entry.archive).map_err(|e| Error::io(&entry.archive, e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| {
        Error::CorruptBackup(format!("Archiv lässt sich nicht als ZIP öffnen: {}", entry.archive.display()))
    })?;

    // Vollständiger Vorlauf: jeder Zielpfad wird aufgelöst und geprüft
    // (Zip-Slip, Symlinks), bevor auch nur eine Datei geschrieben wird. Ein
    // bösartiger oder beschädigter Eintrag mitten im Archiv darf das
    // Save-Verzeichnis nicht halb überschrieben zurücklassen.
    let mut targets = Vec::with_capacity(zip.len());
    for i in 0..zip.len() {
        let zip_entry = zip.by_index(i).map_err(|e| {
            Error::io(&entry.archive, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        if !zip_entry.is_file() {
            continue;
        }
        let name = zip_entry.name().to_string();
        let target = resolve_and_check_target(save_dir, &name)?;
        targets.push((i, target));
    }

    for (i, target) in targets {
        let mut zip_entry = zip.by_index(i).map_err(|e| {
            Error::io(&entry.archive, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        let mut content = Vec::new();
        std::io::copy(&mut zip_entry, &mut content).map_err(|e| Error::io(&target, e))?;
        write_atomic_bytes(&target, &content)?;
    }

    Ok(())
}

/// Stellt ein Backup wieder her. Legt **immer zuerst**, bevor das
/// wiederherzustellende Archiv gelesen wird, ein Sicherungs-Backup des
/// aktuellen Standes an, verifiziert diese Sicherung selbst und gibt sie
/// zurück – so ist ein Überschreiben nie folgenlos, und die Rückfallebene
/// wird nicht blind vertraut.
///
/// `restore` überschreibt nur Dateien, die im Archiv enthalten sind.
/// Dateien, die in `save_dir` liegen, aber nicht im Archiv, bleiben
/// unangetastet – gelöscht wird nie. Ein Rückgängigmachen über die
/// Sicherung wäre bei zusätzlich gelöschten Dateien nicht mehr vollständig
/// möglich; das Risiko eines liegen gebliebenen Fremdlings in einem
/// Proton-Prefix wiegt dagegen gering.
///
/// Jede Datei wird atomar geschrieben (temporäre Datei + rename) und kein
/// bereits vorhandener Symlink innerhalb von `save_dir` wird verfolgt.
/// Schlägt irgendetwas fehl, nachdem die Sicherung bereits angelegt wurde,
/// nennt der zurückgegebene Fehler (`Error::RestoreFailedAfterBackup`)
/// deren Pfad – in dem Moment, in dem die Nutzerin am dringendsten wissen
/// muss, wohin der vorherige Stand verschwunden ist.
pub fn restore(entry: &BackupEntry, save_dir: &Path, backup_root: &Path) -> Result<BackupEntry> {
    verify(entry)?;

    // Sicherung zuerst: unmittelbar danach wird das Archiv aus `entry`
    // gelesen. Läge die Sicherung zeitlich danach, könnte sie – bei einer
    // Wiederherstellung aus einem soeben selbst erzeugten Backup innerhalb
    // derselben Sekunde – genau dieses Archiv überschreiben, bevor es
    // fertig gelesen ist. `unique_backup_name` sorgt zusätzlich dafür, dass
    // sich zwei Backups nie einen Dateinamen teilen.
    let safety_backup = backup(save_dir, backup_root, Some("vor Wiederherstellung"))?;

    restore_after_safety_backup(entry, save_dir, &safety_backup).map_err(|e| {
        Error::RestoreFailedAfterBackup { safety_backup: safety_backup.archive.clone(), source: Box::new(e) }
    })?;

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

        // Ein Archiv, das sich nicht mehr als ZIP öffnen lässt, bekommt
        // einen eigenen, selbst formulierten deutschen Satz ohne
        // eingebettete Bibliotheksmeldung – `CorruptBackup`, damit ein
        // Aufrufer diesen Fall gezielt von einem gewöhnlichen E/A-Fehler
        // unterscheiden und zu einem anderen Backup raten kann.
        assert!(matches!(verify(&entry).unwrap_err(), Error::CorruptBackup(_)));
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

    /// Reproduziert genau den Fehler, den ein reiner Byte-Vergleich der
    /// Dateinamen als Tie-Break verursacht hätte: mit drei Kollisionen
    /// innerhalb derselben Sekunde (identisches Etikett, also identische
    /// Basis) müssen die drei Backups in Erzeugungsreihenfolge – neuestes
    /// zuerst – erscheinen, nicht in der Reihenfolge "Basis, Basis~2,
    /// Basis~1", die "-" vor "." sortieren ergäbe. Kein Sleep nötig: der
    /// Kollisionszähler macht die Reihenfolge unabhängig von der Uhr.
    #[test]
    fn list_backups_orders_same_second_collisions_by_creation_order() {
        let (_tmp, saves, backups) = save_fixture();
        let first = backup(&saves, &backups, Some("gleich")).unwrap();
        let second = backup(&saves, &backups, Some("gleich")).unwrap();
        let third = backup(&saves, &backups, Some("gleich")).unwrap();

        let list = list_backups(&backups).unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].archive, third.archive, "zuletzt erzeugtes Kollisions-Backup muss zuerst stehen");
        assert_eq!(list[1].archive, second.archive);
        assert_eq!(list[2].archive, first.archive);
    }

    /// Legt von Hand ein leeres, aber gültiges Backup (Archiv + Manifest)
    /// mit fest vorgegebenem `created_at` und Dateinamensbasis an – dient
    /// Tests, die eine Tie-Break-Situation ohne jede Abhängigkeit von der
    /// Uhr reproduzieren wollen.
    fn write_backup_pair(backup_root: &Path, base: &str, created_at: &str) {
        std::fs::create_dir_all(backup_root).unwrap();
        let archive_path = backup_root.join(format!("{base}.zip"));
        let manifest_path = backup_root.join(format!("{base}.json"));

        let file = std::fs::File::create(&archive_path).unwrap();
        zip::ZipWriter::new(file).finish().unwrap();

        let manifest = BackupManifest {
            created_at: created_at.to_string(),
            source: "irrelevant".to_string(),
            label: None,
            files: BTreeMap::new(),
        };
        std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
    }

    /// Zwei Backups mit identischem `created_at`, aber unterschiedlicher
    /// Basis (z. B. verschiedenes Etikett) kollidieren nicht – der
    /// Kollisionszähler ist für beide 0 und entscheidet nichts. Ohne einen
    /// abschließenden, expliziten Tie-Break auf den Archivpfad würde die
    /// Reihenfolge dann von der (nicht zugesicherten) `read_dir`-Reihenfolge
    /// abhängen, statt deterministisch zu sein.
    #[test]
    fn list_backups_breaks_ties_deterministically_when_bases_differ() {
        let (_tmp, _saves, backups) = save_fixture();
        let same_timestamp = "2026-01-01T00:00:00Z";
        write_backup_pair(&backups, "zzz_alpha", same_timestamp);
        write_backup_pair(&backups, "aaa_beta", same_timestamp);

        let list = list_backups(&backups).unwrap();
        assert_eq!(list.len(), 2);
        let stems: Vec<&str> = list.iter().map(|e| e.archive.file_stem().unwrap().to_str().unwrap()).collect();
        assert_eq!(
            stems,
            vec!["aaa_beta", "zzz_alpha"],
            "gleicher Zeitstempel, unterschiedliche Basis: Reihenfolge muss deterministisch (aufsteigend nach Pfad) sein"
        );
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

    /// Baut von Hand ein Backup (Archiv + passendes Manifest) mit genau
    /// einem Eintrag `name`/`content` – Hash und Größe im Manifest stimmen
    /// bewusst überein, damit ein Test gezielt nur die Pfadprüfung trifft,
    /// nicht Hash- oder Größenvergleich.
    fn write_backup_with_single_entry(backup_root: &Path, save_dir: &Path, name: &str, content: &[u8]) -> BackupEntry {
        std::fs::create_dir_all(backup_root).unwrap();
        let archive_path = backup_root.join("boese.zip");
        let manifest_path = backup_root.join("boese.json");

        let file = std::fs::File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        zip.start_file(name, opts).unwrap();
        zip.write_all(content).unwrap();
        zip.finish().unwrap();

        let mut files = BTreeMap::new();
        files.insert(
            name.to_string(),
            FileRecord { hash: blake3::hash(content).to_hex().to_string(), size: content.len() as u64 },
        );
        let manifest = BackupManifest {
            created_at: now_rfc3339(),
            source: save_dir.display().to_string(),
            label: None,
            files,
        };
        std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();

        BackupEntry {
            archive: archive_path,
            manifest: manifest_path,
            created_at: manifest.created_at,
            label: None,
        }
    }

    fn tmp_parent(saves: &Path) -> PathBuf {
        saves.parent().unwrap().to_path_buf()
    }

    /// `verify` muss einen hinausführenden Pfad selbst erkennen – nicht
    /// erst `restore`. Hash und Anzahl stimmen hier absichtlich exakt mit
    /// dem Manifest überein: ein Aufrufer, der sich allein auf `verify`
    /// verlässt, um "Backup ist in Ordnung" zu melden, darf so ein Archiv
    /// nicht grün lackieren.
    #[test]
    fn verify_rejects_a_hostile_path_even_when_hash_and_count_match() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = write_backup_with_single_entry(&backups, &saves, "../entkommen.sav", b"BOESARTIG");

        assert!(matches!(verify(&entry).unwrap_err(), Error::CorruptBackup(_)));
    }

    #[test]
    fn verify_rejects_absolute_paths_in_entry_names() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = write_backup_with_single_entry(&backups, &saves, "/etc/passwd", b"BOESARTIG");

        assert!(matches!(verify(&entry).unwrap_err(), Error::CorruptBackup(_)));
    }

    /// Rückwärtsschrägstriche sind auf Unix zwar nur ein gewöhnliches
    /// Zeichen im Dateinamen, kein Trennzeichen – `verify` lehnt sie
    /// trotzdem ab, weil ein Archiv nicht nur von diesem Crate gelesen
    /// werden muss und andere Werkzeuge sie als Trennzeichen verstehen
    /// könnten.
    #[test]
    fn verify_rejects_backslash_components_in_entry_names() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = write_backup_with_single_entry(&backups, &saves, "slot1\\campaign.sav", b"BOESARTIG");

        assert!(matches!(verify(&entry).unwrap_err(), Error::CorruptBackup(_)));
    }

    /// Ein von Hand präpariertes Archiv, dessen Eintragsname aus dem
    /// Save-Verzeichnis hinausführen würde, wird schon von `verify` (dem
    /// allerersten Schritt von `restore`) abgelehnt – vor jeder Sicherung.
    /// Das ist strenger, als der ursprüngliche Plan vorsah (der die Prüfung
    /// erst mitten in der Extraktionsschleife von `restore` ansiedelte).
    #[test]
    fn restore_rejects_path_traversal_before_taking_any_safety_backup() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = write_backup_with_single_entry(&backups, &saves, "../entkommen.sav", b"BOESARTIG");

        let err = restore(&entry, &saves, &backups).unwrap_err();
        assert!(matches!(err, Error::CorruptBackup(_)), "verify() lehnt vor jeder Sicherung ab: {err:?}");
        assert_eq!(
            list_backups(&backups).unwrap().len(),
            1,
            "es darf keine zusätzliche Sicherung entstanden sein, wenn schon verify() ablehnt"
        );
        assert!(!tmp_parent(&saves).join("entkommen.sav").exists());
    }

    /// `verify` prüft jetzt auch die Größe, nicht mehr nur den Hash: ein
    /// von Hand auf eine falsche Größe gesetztes Manifest muss auch dann
    /// auffallen, wenn der Hash-Vergleich (der bei unverändertem Inhalt
    /// weiterhin passt) allein grünes Licht gäbe.
    #[test]
    fn verify_rejects_a_manifest_with_a_tampered_size_even_when_hash_matches() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, None).unwrap();

        let mut manifest: BackupManifest =
            serde_json::from_str(&std::fs::read_to_string(&entry.manifest).unwrap()).unwrap();
        manifest.files.get_mut("profile.sav").unwrap().size = 999;
        std::fs::write(&entry.manifest, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();

        assert!(matches!(verify(&entry).unwrap_err(), Error::CorruptBackup(_)));
    }

    #[test]
    fn unique_backup_name_appends_a_collision_counter() {
        let tmp = tempfile::tempdir().unwrap();
        let backup_root = tmp.path().join("backups");
        std::fs::create_dir_all(&backup_root).unwrap();
        std::fs::write(backup_root.join("basis.zip"), b"x").unwrap();
        std::fs::write(backup_root.join("basis.json"), b"x").unwrap();

        let (archive, manifest) = unique_backup_name(&backup_root, "basis");

        assert_eq!(archive, backup_root.join("basis~1.zip"));
        assert_eq!(manifest, backup_root.join("basis~1.json"));
        assert_eq!(collision_counter("basis~1"), 1);
        assert_eq!(collision_counter("basis"), 0, "ohne Kollision gibt es keinen Zähler");
    }

    /// Eine Sicherung wird nie blind vertraut: ist die soeben angelegte
    /// Sicherung (aus welchem Grund auch immer) selbst beschädigt, muss der
    /// eigentliche Wiederherstellungsvorgang das erkennen und abbrechen,
    /// statt auf ihr aufzubauen.
    #[test]
    fn restore_after_safety_backup_refuses_to_proceed_if_the_safety_copy_is_corrupt() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, None).unwrap();
        let safety = backup(&saves, &backups, Some("vor Wiederherstellung")).unwrap();
        std::fs::write(&safety.archive, b"kaputt").unwrap();

        let err = restore_after_safety_backup(&entry, &saves, &safety).unwrap_err();
        assert!(
            matches!(err, Error::CorruptBackup(_)),
            "eine beschädigte Sicherung darf niemals als Rückfallebene gelten: {err:?}"
        );
    }

    /// Ersetzt `slot1` durch einen Symlink auf ein fremdes Verzeichnis,
    /// bevor wiederhergestellt wird. `profile.sav` steht laut sortiertem
    /// relativem Pfad im Archiv VOR `slot1/campaign.sav` – trotzdem darf es
    /// nicht überschrieben worden sein: der vollständige Vorlauf muss den
    /// unsicheren zweiten Eintrag erkennen, bevor der erste geschrieben
    /// wird, und es darf nirgends durch den Symlink hindurch geschrieben
    /// werden.
    #[test]
    fn restore_does_not_write_any_file_when_a_later_entry_is_unsafe() {
        let (tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, None).unwrap();

        std::fs::write(saves.join("profile.sav"), b"UNVERAENDERT LASSEN").unwrap();
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::remove_dir_all(saves.join("slot1")).unwrap();
        std::os::unix::fs::symlink(&outside, saves.join("slot1")).unwrap();

        let err = restore(&entry, &saves, &backups).unwrap_err();
        let Error::RestoreFailedAfterBackup { safety_backup, .. } = &err else {
            panic!("erwartet: Error::RestoreFailedAfterBackup, war: {err:?}");
        };
        assert!(
            err.to_string().contains(safety_backup.to_str().unwrap()),
            "die Fehlermeldung muss den Pfad der Sicherung nennen: {err}"
        );
        assert!(safety_backup.is_file(), "die genannte Sicherung muss tatsächlich angelegt worden sein");

        assert_eq!(
            std::fs::read(saves.join("profile.sav")).unwrap(),
            b"UNVERAENDERT LASSEN",
            "der Vorlauf muss den unsicheren zweiten Eintrag erkennen, bevor der erste geschrieben wird"
        );
        assert!(!outside.join("campaign.sav").exists(), "darf nicht durch den Symlink hindurch geschrieben haben");
    }

    /// Ein Symlink innerhalb von `save_dir`, der auf `save_dir` selbst
    /// zeigt, darf `backup` nicht in eine Endlosrekursion schicken – und da
    /// `restore` `backup` immer zuerst aufruft, bliebe sonst auch jede
    /// Wiederherstellung hängen.
    #[test]
    fn backup_does_not_follow_a_symlink_cycle() {
        let (_tmp, saves, backups) = save_fixture();
        std::os::unix::fs::symlink(&saves, saves.join("zyklus")).unwrap();

        let entry = backup(&saves, &backups, None).unwrap();

        let manifest: BackupManifest =
            serde_json::from_str(&std::fs::read_to_string(&entry.manifest).unwrap()).unwrap();
        assert_eq!(manifest.files.len(), 2, "der Symlink selbst darf nicht als Datei eingesammelt werden");
    }

    /// Ein Rückwärtsschrägstrich ist auf Unix ein gewöhnliches, gültiges
    /// Zeichen in einem Dateinamen – `backup` darf ein solches Save trotzdem
    /// nicht klaglos einpacken: `verify` (und damit jede `restore`, die
    /// ihre eigene Sicherung verifiziert) würde das erzeugte Archiv sofort
    /// wieder als beschädigt zurückweisen. `backup` muss also selbst schon
    /// ablehnen, statt ein Archiv zu erzeugen, das nie eine eigene Prüfung
    /// besteht.
    #[test]
    fn backup_rejects_a_save_file_whose_name_contains_a_backslash() {
        let (_tmp, saves, backups) = save_fixture();
        std::fs::write(saves.join("slot1\\campaign.sav"), b"X").unwrap();

        let err = backup(&saves, &backups, None).unwrap_err();
        assert!(matches!(err, Error::CorruptBackup(_)), "{err:?}");
        assert!(list_backups(&backups).unwrap().is_empty(), "es darf kein halbes Archiv zurückbleiben");
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

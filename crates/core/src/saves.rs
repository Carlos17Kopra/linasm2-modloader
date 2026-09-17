//! Backing up and restoring savegames.
//!
//! Space Marine 2 saves live in a Proton prefix that Steam Cloud can
//! overwrite at any moment. A bug here destroys real save data — so
//! `restore` never overwrites anything without first backing up the current
//! state itself and verifying that backup, and `verify` checks every backup
//! byte for byte against its manifest before it is used. Everything is
//! written fsync-before-rename (the archive as well as restored files), and
//! symlinks inside the save directory are never followed.

use crate::atomic::write_atomic;
use crate::error::{ArchiveDefect, BackupDefect, Error, Result};
use crate::import::now_rfc3339;
use crate::platform::{Current, Platform};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Hash and size of a single backed-up file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileRecord {
    pub hash: String,
    pub size: u64,
}

/// Accompanies every backup and makes corruption detectable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackupManifest {
    pub created_at: String,
    pub source: String,
    #[serde(default)]
    pub label: Option<String>,
    /// The key is the path relative to the save directory, separated by '/'.
    pub files: BTreeMap<String, FileRecord>,
}

/// A reference to a backup: the archive and manifest files plus the
/// metadata that `list_backups` and the display need.
#[derive(Debug, Clone)]
pub struct BackupEntry {
    pub archive: PathBuf,
    pub manifest: PathBuf,
    pub created_at: String,
    pub label: Option<String>,
}

/// Lists every file below `root` recursively, as pairs of the path relative
/// to `root` ('/'-separated, platform-independent for the manifest) and the
/// absolute path. The result is sorted by the relative path so that a
/// backup is deterministic regardless of the (unspecified) `read_dir`
/// order.
///
/// Symlinks are skipped rather than followed — the same rule as when
/// collecting unpacked paks in `import.rs`. Without it a symlink cycle
/// below `save_dir` would make this function recurse forever (and with it
/// `restore`, which always calls `backup` first); a link to a foreign
/// directory would also pull that directory's contents into the backup.
///
/// Every relative path is additionally checked with `validate_entry_name`
/// (empty/`.`/`..` components, backslashes): without that check `backup`
/// could produce an archive that `verify` — and therefore every later
/// `restore`, which verifies its own safety backup — rejects as corrupt,
/// for instance because of a real file named `slot1\campaign.sav` (an
/// ordinary, valid file name on Unix).
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
                    .map_err(|_| Error::CorruptBackup(BackupDefect::OutsideSaveDir))?
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

/// A timestamp suitable for use in a file name: 2026-09-12_180000
fn timestamp_for_filename(rfc: &str) -> String {
    rfc.trim_end_matches('Z').replace(':', "").replace('T', "_")
}

/// Finds a still-unused pair of archive and manifest names. If `base`
/// collides with an existing backup, `~<counter>` is appended. '~' is safe
/// as a separator because neither the timestamp nor `sanitize_label` ever
/// produces that character (unlike '-', which could come from a label such
/// as "Kapitel-3") — this lets `collision_counter` recover the counter
/// unambiguously, without confusing it with hyphens from a label.
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

/// Reads back the collision counter that `unique_backup_name` appended to a
/// file name stem (0 if none was appended). Serves `list_backups` as a
/// tie-break for backups with an identical `created_at`: a plain byte
/// comparison of the file names would be wrong here, because '-' (0x2D)
/// sorts before '.' (0x2E) and "basis-1.zip" would therefore come
/// lexicographically before "basis.zip", even though "basis.zip" was
/// created first.
fn collision_counter(stem: &str) -> u32 {
    stem.rsplit_once('~').and_then(|(_, suffix)| suffix.parse().ok()).unwrap_or(0)
}

/// Opens a directory only to call `sync_all` on it. On Unix this forces a
/// new directory entry (here: the freshly written archive file) to have
/// actually reached the block device.
#[cfg(unix)]
fn sync_dir(dir: &Path) -> Result<()> {
    std::fs::File::open(dir).and_then(|f| f.sync_all()).map_err(|e| Error::io(dir, e))
}

/// Windows has no counterpart, and this is deliberately a no-op rather
/// than an error.
///
/// `File::open` on a directory fails there outright with
/// ERROR_ACCESS_DENIED — a directory handle needs
/// `FILE_FLAG_BACKUP_SEMANTICS`, which `std` does not set — and flushing
/// one is not an operation Windows offers the way Unix does. Returning the
/// error instead would make every backup and every restore fail on a
/// perfectly healthy system, which is what it did until CI first ran these
/// tests on Windows.
///
/// The honest consequence: the promise "the new directory entry has
/// reached the device" is weaker on Windows than on Linux, and rests on
/// NTFS's own metadata journalling instead. The rest of the ordering is
/// untouched — the archive is written to a temporary file, flushed, and
/// only then renamed over its target — so a crash still cannot leave a
/// half-written archive behind on either system.
#[cfg(windows)]
fn sync_dir(_dir: &Path) -> Result<()> {
    Ok(())
}

/// Makes a label file-name-safe: only alphanumeric characters and hyphens
/// survive, everything else becomes '-', and leading and trailing hyphens
/// are dropped. If the label consists only of punctuation the result is
/// empty — the caller then does not append it to the file name at all (see
/// `backup`), so no empty name part and no name starting with '.' can ever
/// arise.
fn sanitize_label(label: &str) -> String {
    label
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// A backup's file name stem: the timestamp, followed by '_' and the
/// file-name-safe label when the backup has one. Creating (`backup`) and
/// renaming (`rename`) must follow the same rule — otherwise a renamed
/// backup would end up with a name `backup` would never have assigned, and
/// `rename` could no longer tell afterwards that the name is already
/// correct.
fn backup_base_name(created_at: &str, label: Option<&str>) -> String {
    let base = timestamp_for_filename(created_at);
    match label.map(sanitize_label) {
        Some(clean) if !clean.is_empty() => format!("{base}_{clean}"),
        _ => base,
    }
}

/// Writes a manifest atomically.
///
/// The serialization error is unreachable for the current field types
/// (String, `Option<_>`, u64, `BTreeMap<String, _>`); the raw error is
/// discarded on purpose so that the message stays a catalogued,
/// translatable one instead of the library's raw (English) text (cf.
/// `Library::save`).
fn write_manifest(path: &Path, manifest: &BackupManifest) -> Result<()> {
    let json = serde_json::to_string_pretty(manifest).map_err(|_| {
        Error::io(
            path,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                crate::t!("error.manifest_serialize_failed"),
            ),
        )
    })?;
    write_atomic(path, &format!("{json}\n"))
}

/// Creates a backup: every file from `save_dir` is packed into a new ZIP
/// archive under `backup_root`, accompanied by a manifest holding the hash
/// and size of each file.
pub fn backup(save_dir: &Path, backup_root: &Path, label: Option<&str>) -> Result<BackupEntry> {
    backup_from(save_dir, backup_root, label, save_dir)
}

/// The body of `backup`, with the manifest's `source` field as a separate
/// parameter. `import_archive` unpacks a foreign archive into a temporary
/// directory and then goes through here, so that an imported backup is
/// written by exactly the same (crash-safe) sequence as any other — while
/// the manifest still names the ZIP it came from and not the temporary
/// directory, which is gone by the time anyone reads it.
fn backup_from(
    save_dir: &Path,
    backup_root: &Path,
    label: Option<&str>,
    source: &Path,
) -> Result<BackupEntry> {
    if !save_dir.is_dir() {
        return Err(Error::io(
            save_dir,
            std::io::Error::new(std::io::ErrorKind::NotFound, crate::t!("error.missing_save_dir")),
        ));
    }
    std::fs::create_dir_all(backup_root).map_err(|e| Error::io(backup_root, e))?;

    let now = now_rfc3339();
    let base = backup_base_name(&now, label);

    // The timestamp has one-second resolution. Two backups within the same
    // second must not overwrite each other — `restore` takes a safety
    // backup immediately before reading an archive and would otherwise
    // destroy the very archive it is about to read.
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
    // The manifest describes exactly this archive file and is written
    // atomically in a moment (see `write_atomic` below). Without this fsync
    // (and the one on the directory) a crash shortly afterwards could leave
    // behind a durable manifest whose archive never got its bytes onto the
    // disk — and then the safety backup `restore` receives from this very
    // call would be worthless too.
    archive_file.sync_all().map_err(|e| Error::io(&archive_path, e))?;
    drop(archive_file);
    sync_dir(backup_root)?;

    let manifest = BackupManifest {
        created_at: now.clone(),
        source: source.display().to_string(),
        label: label.map(str::to_string),
        files: records,
    };
    write_manifest(&manifest_path, &manifest)?;

    Ok(BackupEntry {
        archive: archive_path,
        manifest: manifest_path,
        created_at: now,
        label: label.map(str::to_string),
    })
}

/// Checks an entry name from the manifest or the archive against Zip-Slip:
/// empty, `.` or `..` components and backslashes (a separator in some
/// tools) are not allowed. A name starting with '/' (an absolute path)
/// splits into an empty first component and is rejected through that same
/// rule.
fn validate_entry_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::CorruptBackup(BackupDefect::EmptyPath));
    }
    for component in name.split('/') {
        if component.is_empty() || component == "." || component == ".." || component.contains('\\') {
            return Err(Error::CorruptBackup(BackupDefect::InvalidPath { name: name.to_string() }));
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
                crate::t!("error.invalid_manifest", line = e.line(), column = e.column()),
            ),
        )
    })
}

/// Checks every file in the archive against the hash and size in the
/// manifest: the archive must open as a ZIP, every name (in the manifest as
/// well as in the archive) must be a valid relative path (see
/// `validate_entry_name`), every entry must appear in the manifest and may
/// appear there only once (otherwise a duplicate entry could mask a file
/// missing from the archive), and size and hash must match; finally the
/// number of verified entries must match the expected number exactly.
///
/// The path check runs here and not only in `restore`: a caller that uses
/// `verify` to report "backup is fine" must not paint an archive with an
/// escaping entry name green.
pub fn verify(entry: &BackupEntry) -> Result<()> {
    let manifest = read_manifest(entry)?;
    for name in manifest.files.keys() {
        validate_entry_name(name)?;
    }

    let file = std::fs::File::open(&entry.archive).map_err(|e| Error::io(&entry.archive, e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| {
        Error::CorruptBackup(BackupDefect::NotAZip { path: entry.archive.clone() })
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
            .ok_or_else(|| Error::CorruptBackup(BackupDefect::UnknownEntry { name: name.clone() }))?;

        if !seen.insert(name.clone()) {
            return Err(Error::CorruptBackup(BackupDefect::DuplicateEntry { name: name.clone() }));
        }

        let mut content = Vec::new();
        std::io::copy(&mut zip_entry, &mut content).map_err(|e| Error::io(&entry.archive, e))?;

        if content.len() as u64 != expected.size {
            return Err(Error::CorruptBackup(BackupDefect::SizeMismatch { name: name.clone() }));
        }
        let actual = blake3::hash(&content).to_hex().to_string();
        if actual != expected.hash {
            return Err(Error::CorruptBackup(BackupDefect::HashMismatch { name: name.clone() }));
        }
    }

    if seen.len() != manifest.files.len() {
        return Err(Error::CorruptBackup(BackupDefect::CountMismatch {
            found: seen.len(),
            expected: manifest.files.len(),
        }));
    }
    Ok(())
}

/// All backups under `backup_root`, newest first. A `.zip` without an
/// accompanying `.json` (or the other way round) is silently skipped — it
/// is not a backup created by this function.
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

    // Newest first. Within the same second the collision counter from the
    // file name decides first (see `collision_counter`) — a plain byte
    // comparison of the paths would be wrong here (see its doc comment).
    // Two backups with the same `created_at` but a different base (e.g. a
    // different label, so counter 0 on both sides) would otherwise fall
    // back to the (unspecified) `read_dir` order for lack of a final,
    // explicit tie-break — the closing path comparison makes the result
    // deterministic in every case.
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

/// Changes a backup's label. The timestamp — a backup's identity, by which
/// the UI recognizes it again — stays untouched. Archive and manifest move
/// to the file name `backup` would have assigned for this label, so that a
/// backup is called the same thing in the file manager as in the UI.
///
/// The order of the three steps is the actual protection: first write the
/// new manifest, then move the archive, then delete the old manifest. At
/// every crash point in between exactly one complete `.zip`/`.json` pair
/// exists, and the archive — the only irreplaceable part — is never lost;
/// whichever half is left over is an orphan that `list_backups` silently
/// skips. The reverse order (moving first) would make the backup disappear
/// from the list entirely after a crash between the steps.
pub fn rename(entry: &BackupEntry, backup_root: &Path, label: Option<&str>) -> Result<BackupEntry> {
    // The manifest, not `entry`, is the source for `created_at`:
    // `list_backups` does build its entries from it, but a hand-assembled
    // `BackupEntry` need not have filled that field.
    let mut manifest = read_manifest(entry)?;
    manifest.label = label.map(str::to_string);

    let base = backup_base_name(&manifest.created_at, label);
    if entry.archive.file_stem().and_then(|s| s.to_str()) == Some(base.as_str()) {
        // "Kapitel-3" → "Kapitel 3": the same file name, only a different
        // label. If we moved anyway, `unique_backup_name` would consider
        // the backup's own, currently occupied name taken and append a
        // pointless `~1`.
        write_manifest(&entry.manifest, &manifest)?;
        return Ok(BackupEntry {
            archive: entry.archive.clone(),
            manifest: entry.manifest.clone(),
            created_at: manifest.created_at,
            label: manifest.label,
        });
    }

    let (new_archive, new_manifest) = unique_backup_name(backup_root, &base);
    write_manifest(&new_manifest, &manifest)?;
    if let Err(e) = std::fs::rename(&entry.archive, &new_archive) {
        // Without this cleanup a manifest orphan would be left behind,
        // which `unique_backup_name` would read as a taken name forever —
        // a later attempt with the same label would then get a `~1`.
        let _ = std::fs::remove_file(&new_manifest);
        return Err(Error::io(&entry.archive, e));
    }
    std::fs::remove_file(&entry.manifest).map_err(|e| Error::io(&entry.manifest, e))?;
    sync_dir(backup_root)?;

    Ok(BackupEntry {
        archive: new_archive,
        manifest: new_manifest,
        created_at: manifest.created_at,
        label: manifest.label,
    })
}

/// Deletes a backup for good, archive as well as manifest. A file that has
/// already vanished is not an error — the goal is then already reached, and
/// a second click on "Delete" should not produce an error message.
pub fn delete(entry: &BackupEntry) -> Result<()> {
    remove_if_present(&entry.archive)?;
    remove_if_present(&entry.manifest)?;
    if let Some(dir) = entry.manifest.parent() {
        sync_dir(dir)?;
    }
    Ok(())
}

fn remove_if_present(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(path, e)),
    }
}

/// Resolves an entry name (already checked via `validate_entry_name`) to a
/// target path under `save_dir`.
fn resolve_target_path(save_dir: &Path, name: &str) -> PathBuf {
    let mut target = save_dir.to_path_buf();
    for component in name.split('/') {
        target.push(component);
    }
    target
}

/// Resolves an archive entry name to a target path under `save_dir` and
/// rejects it if any already existing part of that path (an intermediate
/// directory or the target file itself) is a symlink. Otherwise
/// `create_dir_all` and the creation of the temporary file would follow
/// such a symlink and could end up outside `save_dir` — for instance if
/// `slot1` had been replaced by a link to `~/.config`. Because this path is
/// checked component by component with `symlink_metadata` (instead of
/// `metadata`, which would follow) before anything is written, that can no
/// longer happen.
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

/// Writes `content` atomically to `path`: a temporary file in the same
/// directory, fsync, then rename (atomic on POSIX) — the same pattern as
/// `atomic::write_atomic`, only for binary instead of textual content.
/// `write_atomic` itself deliberately stays restricted to `&str`
/// (configuration and manifest files); this local variant covers the
/// arbitrary binary data restored from the archive without widening that
/// signature. A crash in the middle of `write_all` (e.g. a full disk) can
/// therefore never leave a truncated save file behind — only the discarded
/// temporary file.
fn write_atomic_bytes(path: &Path, content: &[u8]) -> Result<()> {
    let dir = path.parent().ok_or_else(|| {
        Error::io(
            path,
            std::io::Error::new(std::io::ErrorKind::InvalidInput, crate::t!("error.path_without_parent")),
        )
    })?;
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;

    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|e| Error::io(dir, e))?;
    tmp.write_all(content).map_err(|e| Error::io(path, e))?;
    tmp.as_file().sync_all().map_err(|e| Error::io(path, e))?;
    tmp.persist(path).map_err(|e| Error::io(path, e.error))?;
    Ok(())
}

/// The actual restore operation, after the safety backup of the current
/// state has already been taken. It lives in its own function so that
/// `restore` can enrich every error occurring here with the path of that
/// safety backup (see `Error::RestoreFailedAfterBackup`).
fn restore_after_safety_backup(entry: &BackupEntry, save_dir: &Path, safety_backup: &BackupEntry) -> Result<()> {
    // The safety backup is not trusted blindly: verify first, then take
    // the risk. Without this check a crash in the middle of the
    // overwriting below could leave the only fallback behind as (silently)
    // corrupt.
    verify(safety_backup)?;

    let file = std::fs::File::open(&entry.archive).map_err(|e| Error::io(&entry.archive, e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| {
        Error::CorruptBackup(BackupDefect::NotAZip { path: entry.archive.clone() })
    })?;

    // A complete dry run first: every target path is resolved and checked
    // (Zip-Slip, symlinks) before even one file is written. A malicious or
    // damaged entry in the middle of the archive must not leave the save
    // directory half overwritten.
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

/// Restores a backup. **Always first**, before the archive to be restored
/// is read, it takes a safety backup of the current state, verifies that
/// backup itself and returns it — so an overwrite can always be undone, and
/// the fallback is not trusted blindly.
///
/// `restore` only overwrites files contained in the archive. Files that
/// live in `save_dir` but are not in the archive stay untouched — nothing
/// is ever deleted. Undoing a restore via the safety backup would no longer
/// be complete if files had additionally been deleted; the risk of a stray
/// leftover in a Proton prefix weighs far less.
///
/// Every file is written atomically (temporary file + rename) and no
/// already existing symlink inside `save_dir` is followed. If anything
/// fails after the safety backup has been taken, the returned error
/// (`Error::RestoreFailedAfterBackup`) names its path — at the very moment
/// the user most urgently needs to know where the previous state went.
pub fn restore(entry: &BackupEntry, save_dir: &Path, backup_root: &Path) -> Result<BackupEntry> {
    verify(entry)?;

    // Safety backup first: immediately afterwards the archive from `entry`
    // is read. If the safety backup came later, it could — when restoring
    // from a backup just created within the same second — overwrite that
    // very archive before it has been read completely.
    // `unique_backup_name` additionally makes sure two backups never share
    // a file name.
    //
    // The label stays German by decision, like
    // `vanilla::VANILLA_SNAPSHOT_PREFIX`: it is matched by prefix and
    // already written into existing backup names on users' disks, so
    // translating it would rename data that is already there. It is shown
    // even in an English interface — a known limitation, not an
    // oversight — and splitting stored from displayed form is an open
    // question left for later.
    let safety_backup = backup(save_dir, backup_root, Some("vor Wiederherstellung"))?;

    restore_after_safety_backup(entry, save_dir, &safety_backup).map_err(|e| {
        Error::RestoreFailedAfterBackup { safety_backup: safety_backup.archive.clone(), source: Box::new(e) }
    })?;

    Ok(safety_backup)
}

/// Checks whether the Steam client is currently running. Cloud sync can
/// overwrite a restore in the background — that is the most likely route to
/// data loss around this functionality.
///
/// A public entry point that forwards to `Platform::steam_is_running`:
/// process detection itself (e.g. `/proc` on Linux) is platform-dependent
/// and therefore belongs behind the `Platform` trait (see
/// `platform::unix::Unix::steam_is_running` for the details of the
/// detection), not hard-wired here.
pub fn steam_is_running() -> bool {
    Current::steam_is_running()
}

/// Upper bound for the uncompressed total size of an archive being
/// imported. Savegames are a few megabytes; anything far beyond that is
/// either not a savegame archive or a zip bomb, and the check happens
/// before the first entry is unpacked so that neither can fill the disk.
const MAX_IMPORT_BYTES: u64 = 512 * 1024 * 1024;

/// The file extensions a Space Marine 2 savegame uses. An archive without
/// any of them is rejected — the most likely mistake in the file dialog is
/// picking a mod archive.
const SAVE_EXTENSIONS: [&str; 2] = ["cfg", "sav"];

/// Imports a backup produced by another launcher: a plain ZIP whose entries
/// are the savegame files. The archive is checked, unpacked into a
/// temporary directory and written back out through `backup_from`, so what
/// ends up under `backup_root` is an ordinary backup of this program —
/// archive plus manifest, verifiable, restorable.
///
/// Without a `label` the archive's file name becomes the label, so that an
/// imported backup can still be told apart from the locally created ones in
/// the list.
pub fn import_archive(archive: &Path, backup_root: &Path, label: Option<&str>) -> Result<BackupEntry> {
    import_archive_limited(archive, backup_root, label, MAX_IMPORT_BYTES)
}

/// `import_archive` with the size limit as a parameter, so that the limit
/// can be tested without building a 512 MiB archive.
fn import_archive_limited(
    archive: &Path,
    backup_root: &Path,
    label: Option<&str>,
    max_bytes: u64,
) -> Result<BackupEntry> {
    let file = std::fs::File::open(archive).map_err(|e| Error::io(archive, e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| {
        // The raw (English) message of the zip crate stays out of this: the
        // user is told, in whichever language is active, that the file is
        // not a readable ZIP.
        Error::UnusableArchive(ArchiveDefect::NotAZip { path: archive.to_path_buf() })
    })?;

    let names = collect_import_entries(&mut zip, archive, max_bytes)?;
    if !names.iter().any(|(_, name)| has_save_extension(name)) {
        return Err(Error::NoSaveInArchive(archive.to_path_buf()));
    }
    let prefix = common_directory_prefix(&names);

    // Everything is unpacked into a temporary directory first and only then
    // packed into a backup. A rejected archive therefore leaves nothing
    // behind under `backup_root`, and `backup_from` computes the hashes
    // over the same bytes that were actually written.
    let staging = tempfile::tempdir().map_err(|e| Error::io(archive, e))?;
    let mut budget = max_bytes;
    for (index, name) in &names {
        let mut entry = zip.by_index(*index).map_err(|e| {
            Error::io(archive, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        let relative = strip_leading_components(name, prefix);
        let target = resolve_target_path(staging.path(), &relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }

        // Read one byte beyond the remaining budget: the size in the
        // archive's header is only a claim, and a lying header must not be
        // able to slip past the check made before unpacking.
        let mut content = Vec::new();
        std::io::copy(&mut entry.by_ref().take(budget.saturating_add(1)), &mut content)
            .map_err(|e| Error::io(archive, e))?;
        if content.len() as u64 > budget {
            return Err(oversized(max_bytes));
        }
        budget -= content.len() as u64;

        std::fs::write(&target, &content).map_err(|e| Error::io(&target, e))?;
    }

    let fallback = archive.file_stem().map(|stem| stem.to_string_lossy().into_owned());
    let label = label.map(str::to_string).or(fallback).filter(|text| !text.trim().is_empty());
    backup_from(staging.path(), backup_root, label.as_deref(), archive)
}

/// Checks every entry of the archive and returns the ones to import, as
/// pairs of index in the archive and normalized name.
///
/// Rejected are symlink entries (restored, a link would let a later write
/// land outside the save directory — the archive counterpart of the rule
/// `backup` and `restore` follow), names that escape the save directory,
/// names that appear twice (read by position, the second would silently
/// overwrite the first while unpacking) and a declared total size beyond
/// `max_bytes`.
fn collect_import_entries(
    zip: &mut zip::ZipArchive<std::fs::File>,
    archive: &Path,
    max_bytes: u64,
) -> Result<Vec<(usize, String)>> {
    let mut names: Vec<(usize, String)> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut declared = 0u64;

    for index in 0..zip.len() {
        let entry = zip.by_index(index).map_err(|e| {
            Error::io(archive, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        if entry.unix_mode().is_some_and(|mode| mode & 0o170_000 == 0o120_000) {
            return Err(Error::UnusableArchive(ArchiveDefect::Symlink { name: entry.name().to_string() }));
        }
        if !entry.is_file() {
            continue;
        }

        let name = normalize_entry_name(entry.name());
        validate_entry_name(&name).map_err(|_| {
            Error::UnusableArchive(ArchiveDefect::InvalidPath { name: entry.name().to_string() })
        })?;
        if !seen.insert(name.clone()) {
            return Err(Error::UnusableArchive(ArchiveDefect::DuplicateName { name: name.clone() }));
        }

        declared = declared.saturating_add(entry.size());
        if declared > max_bytes {
            return Err(oversized(max_bytes));
        }
        names.push((index, name));
    }
    Ok(names)
}

fn oversized(max_bytes: u64) -> Error {
    Error::UnusableArchive(ArchiveDefect::TooLarge { limit: max_bytes })
}

/// Brings a foreign entry name into the form the rest of this module
/// expects: '/' as the separator (some Windows packers write '\\', which
/// `validate_entry_name` rejects as a component of a file name) and no
/// leading "./". A leading '/' is deliberately *not* removed — an absolute
/// path is rejected, not silently made relative.
fn normalize_entry_name(name: &str) -> String {
    let converted = name.replace('\\', "/");
    let mut rest = converted.as_str();
    while let Some(stripped) = rest.strip_prefix("./") {
        rest = stripped;
    }
    rest.to_string()
}

fn has_save_extension(name: &str) -> bool {
    match name.rsplit_once('.') {
        Some((_, extension)) => SAVE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()),
        None => false,
    }
}

/// The number of leading path components every entry shares. Other
/// launchers pack their saves below a directory of their own ("Main/",
/// "Backup/Main/"); kept as is, `restore` would create that directory
/// inside the save directory instead of replacing the files in it.
///
/// Only a directory *all* entries lie in counts — a single file next to
/// that directory (a readme, say) would otherwise move the whole rest of
/// the archive one level up.
fn common_directory_prefix(names: &[(usize, String)]) -> usize {
    let mut shared: Option<Vec<&str>> = None;
    for (_, name) in names {
        let mut directories: Vec<&str> = name.split('/').collect();
        directories.pop();
        shared = Some(match shared {
            None => directories,
            Some(previous) => previous
                .into_iter()
                .zip(directories)
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| a)
                .collect(),
        });
        if shared.as_ref().is_some_and(Vec::is_empty) {
            return 0;
        }
    }
    shared.map_or(0, |prefix| prefix.len())
}

/// Drops the first `count` components of an entry name. `count` always
/// comes from `common_directory_prefix` and therefore never covers the file
/// name itself, so the result is never empty.
fn strip_leading_components(name: &str, count: usize) -> String {
    name.split('/').skip(count).collect::<Vec<_>>().join("/")
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

        // An archive that can no longer be opened as a ZIP gets its own,
        // self-written German sentence with no embedded library message —
        // `CorruptBackup`, so that a caller can tell this case apart from
        // an ordinary I/O error and suggest a different backup.
        assert!(matches!(verify(&entry).unwrap_err(), Error::CorruptBackup(_)));
    }

    /// A minimal CRC-32 (bit-reflected, standard polynomial 0xEDB88320)
    /// without an extra dependency — needed only to build a hand-assembled
    /// ZIP archive in tests.
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

    /// Builds, by hand, an invalid but readable ZIP archive with two
    /// entries of the same name (stored, uncompressed). The `zip` crate
    /// refuses this via `ZipWriter` (see `InvalidArchive("Duplicate
    /// filename")`) — but an archive built by hand (or one from another
    /// tool that does not know this check) can contain exactly that, and
    /// `ZipArchive::by_index` reads entries by position, not by name, so it
    /// reads them without complaint.
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

    /// An archive entry that appears twice under the same name must not
    /// mask a file that is genuinely missing: if only the number of
    /// processed entries were counted, the total would still come out right
    /// despite a file that the manifest lists but the archive lacks.
    #[test]
    fn verify_rejects_duplicate_entry_masking_a_missing_file() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, None).unwrap();

        // Build an archive by hand: "profile.sav" appears twice,
        // "slot1/campaign.sav" (required by the manifest) is missing
        // entirely.
        write_zip_with_duplicate_entry(&entry.archive, "profile.sav", b"PROFILDATEN");

        assert!(
            matches!(verify(&entry).unwrap_err(), Error::CorruptBackup(_)),
            "a duplicate entry must not mask a missing file"
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

    /// `restore` must never delete files that the archive does not contain
    /// — only the files contained in the archive are overwritten (see the
    /// doc comment on `restore`).
    #[test]
    fn restore_does_not_delete_files_absent_from_the_archive() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, None).unwrap();

        std::fs::write(saves.join("slot1/nicht_gesichert.sav"), b"NEU ANGELEGT").unwrap();

        restore(&entry, &saves, &backups).unwrap();

        assert_eq!(
            std::fs::read(saves.join("slot1/nicht_gesichert.sav")).unwrap(),
            b"NEU ANGELEGT",
            "restore may only overwrite, never delete"
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

        // The overwritten progress can be recovered from the safety backup.
        restore(&safety_backup, &saves, &backups).unwrap();
        assert_eq!(std::fs::read(saves.join("profile.sav")).unwrap(), b"NEUER FORTSCHRITT");
    }

    #[test]
    fn two_backups_in_the_same_second_do_not_overwrite_each_other() {
        let (_tmp, saves, backups) = save_fixture();

        let first = backup(&saves, &backups, Some("gleich")).unwrap();
        std::fs::write(saves.join("profile.sav"), b"SPAETER").unwrap();
        let second = backup(&saves, &backups, Some("gleich")).unwrap();

        assert_ne!(first.archive, second.archive, "name collision within the same second");
        verify(&first).unwrap();
        verify(&second).unwrap();
    }

    #[test]
    fn restoring_twice_does_not_destroy_any_archive() {
        // restore() takes a backup before reading — that backup must never
        // overwrite the archive being read.
        let (_tmp, saves, backups) = save_fixture();
        let original = backup(&saves, &backups, None).unwrap();

        std::fs::write(saves.join("profile.sav"), b"ZWISCHENSTAND").unwrap();
        let safety_backup = restore(&original, &saves, &backups).unwrap();
        assert_eq!(std::fs::read(saves.join("profile.sav")).unwrap(), b"PROFILDATEN");

        restore(&safety_backup, &saves, &backups).unwrap();
        assert_eq!(std::fs::read(saves.join("profile.sav")).unwrap(), b"ZWISCHENSTAND");
    }

    /// Reproduces exactly the bug a plain byte comparison of the file names
    /// as a tie-break would have caused: with three collisions within the
    /// same second (identical label, hence identical base) the three
    /// backups must appear in creation order — newest first — not in the
    /// order "base, base~2, base~1" that sorting "-" before "." would
    /// yield. No sleep needed: the collision counter makes the order
    /// independent of the clock.
    #[test]
    fn list_backups_orders_same_second_collisions_by_creation_order() {
        let (_tmp, saves, backups) = save_fixture();
        let first = backup(&saves, &backups, Some("gleich")).unwrap();
        let second = backup(&saves, &backups, Some("gleich")).unwrap();
        let third = backup(&saves, &backups, Some("gleich")).unwrap();

        let list = list_backups(&backups).unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].archive, third.archive, "the most recently created collision backup must come first");
        assert_eq!(list[1].archive, second.archive);
        assert_eq!(list[2].archive, first.archive);
    }

    /// Creates, by hand, an empty but valid backup (archive + manifest)
    /// with a fixed `created_at` and file name base — for tests that want
    /// to reproduce a tie-break situation without any dependency on the
    /// clock.
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

    /// Two backups with an identical `created_at` but a different base
    /// (e.g. a different label) do not collide — the collision counter is 0
    /// for both and decides nothing. Without a final, explicit tie-break on
    /// the archive path the order would then depend on the (unspecified)
    /// `read_dir` order instead of being deterministic.
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
            "same timestamp, different base: the order must be deterministic (ascending by path)"
        );
    }

    /// A `.zip` without an accompanying `.json` (e.g. because the manifest
    /// was deleted by hand) is not a valid backup and must not fill the
    /// list with a broken entry.
    #[test]
    fn list_backups_skips_a_zip_without_its_manifest() {
        let (_tmp, saves, backups) = save_fixture();
        backup(&saves, &backups, None).unwrap();
        std::fs::create_dir_all(&backups).unwrap();
        std::fs::write(backups.join("verwaist.zip"), b"egal").unwrap();

        let list = list_backups(&backups).unwrap();
        assert_eq!(list.len(), 1, "the archive without a manifest must not show up");
    }

    /// A backup of an empty save directory must produce a valid, empty
    /// archive together with a manifest, and restoring from it must not
    /// fail (see the doc comment: there is simply nothing to overwrite).
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

    /// A label consisting only of punctuation must produce neither an empty
    /// name part nor a file name starting with '.'.
    #[test]
    fn label_of_only_punctuation_yields_a_clean_filename() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, Some("!!!")).unwrap();

        let stem = entry.archive.file_stem().unwrap().to_str().unwrap();
        assert!(!stem.is_empty());
        assert!(!stem.starts_with('.'));
        assert!(!stem.ends_with('_'), "an empty label must not leave a dangling separator behind");
    }

    #[test]
    fn backup_with_missing_save_dir_reports_an_io_error() {
        let tmp = tempfile::tempdir().unwrap();
        let saves = tmp.path().join("gibt_es_nicht");
        let backups = tmp.path().join("backups");

        assert!(matches!(backup(&saves, &backups, None).unwrap_err(), Error::Io { .. }));
    }

    /// Builds, by hand, a backup (archive + matching manifest) with exactly
    /// one entry `name`/`content` — hash and size in the manifest match on
    /// purpose, so that a test hits only the path check and neither the
    /// hash nor the size comparison.
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

    /// `verify` must detect an escaping path itself — not only `restore`.
    /// Hash and count deliberately match the manifest exactly here: a
    /// caller that relies on `verify` alone to report "backup is fine" must
    /// not paint such an archive green.
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

    /// On Unix a backslash is merely an ordinary character in a file name,
    /// not a separator — `verify` rejects it anyway, because an archive may
    /// be read by more than this crate and other tools could understand it
    /// as a separator.
    #[test]
    fn verify_rejects_backslash_components_in_entry_names() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = write_backup_with_single_entry(&backups, &saves, "slot1\\campaign.sav", b"BOESARTIG");

        assert!(matches!(verify(&entry).unwrap_err(), Error::CorruptBackup(_)));
    }

    /// A hand-crafted archive whose entry name would lead out of the save
    /// directory is rejected by `verify` already (the very first step of
    /// `restore`) — before any safety backup is taken. That is stricter
    /// than the original design called for, which placed the check in the
    /// middle of `restore`'s extraction loop.
    #[test]
    fn restore_rejects_path_traversal_before_taking_any_safety_backup() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = write_backup_with_single_entry(&backups, &saves, "../entkommen.sav", b"BOESARTIG");

        let err = restore(&entry, &saves, &backups).unwrap_err();
        assert!(matches!(err, Error::CorruptBackup(_)), "verify() rejects before any safety backup is taken: {err:?}");
        assert_eq!(
            list_backups(&backups).unwrap().len(),
            1,
            "no additional safety backup may have been created when verify() already rejects"
        );
        assert!(!tmp_parent(&saves).join("entkommen.sav").exists());
    }

    /// `verify` now checks the size too, not only the hash: a manifest
    /// whose size was set to a wrong value by hand must be caught even when
    /// the hash comparison alone (which still matches for unchanged
    /// content) would give the green light.
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
        assert_eq!(collision_counter("basis"), 0, "without a collision there is no counter");
    }

    /// A safety backup is never trusted blindly: if the backup just taken
    /// is itself corrupt (for whatever reason), the actual restore
    /// operation must detect that and abort instead of building on it.
    #[test]
    fn restore_after_safety_backup_refuses_to_proceed_if_the_safety_copy_is_corrupt() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, None).unwrap();
        let safety = backup(&saves, &backups, Some("vor Wiederherstellung")).unwrap();
        std::fs::write(&safety.archive, b"kaputt").unwrap();

        let err = restore_after_safety_backup(&entry, &saves, &safety).unwrap_err();
        assert!(
            matches!(err, Error::CorruptBackup(_)),
            "a corrupt safety backup must never count as a fallback: {err:?}"
        );
    }

    /// Replaces `slot1` with a symlink to a foreign directory before
    /// restoring. By sorted relative path, `profile.sav` comes BEFORE
    /// `slot1/campaign.sav` in the archive — it must still not have been
    /// overwritten: the complete dry run has to spot the unsafe second
    /// entry before the first one is written, and nothing may be written
    /// through the symlink anywhere.
    // Unix only for the fixture, not for the behaviour: creating a
    // symlink needs Developer Mode or elevation on Windows. The guard
    // under test is platform-neutral — it rests on `symlink_metadata`,
    // which reports a link as a link on both systems.
    #[cfg(unix)]
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
            panic!("expected Error::RestoreFailedAfterBackup, was: {err:?}");
        };
        assert!(
            err.to_string().contains(safety_backup.to_str().unwrap()),
            "the error message must name the path of the safety backup: {err}"
        );
        assert!(safety_backup.is_file(), "the named safety backup must actually have been created");

        assert_eq!(
            std::fs::read(saves.join("profile.sav")).unwrap(),
            b"UNVERAENDERT LASSEN",
            "the dry run must spot the unsafe second entry before the first one is written"
        );
        assert!(!outside.join("campaign.sav").exists(), "must not have written through the symlink");
    }

    /// A symlink inside `save_dir` pointing at `save_dir` itself must not
    /// send `backup` into endless recursion — and since `restore` always
    /// calls `backup` first, every restore would otherwise hang as well.
    // Unix only for the fixture, not for the behaviour: creating a
    // symlink needs Developer Mode or elevation on Windows. The guard
    // under test is platform-neutral — it rests on `symlink_metadata`,
    // which reports a link as a link on both systems.
    #[cfg(unix)]
    #[test]
    fn backup_does_not_follow_a_symlink_cycle() {
        let (_tmp, saves, backups) = save_fixture();
        std::os::unix::fs::symlink(&saves, saves.join("zyklus")).unwrap();

        let entry = backup(&saves, &backups, None).unwrap();

        let manifest: BackupManifest =
            serde_json::from_str(&std::fs::read_to_string(&entry.manifest).unwrap()).unwrap();
        assert_eq!(manifest.files.len(), 2, "the symlink itself must not be collected as a file");
    }

    /// On Unix a backslash is an ordinary, valid character in a file name —
    /// `backup` must still not pack such a save without complaint: `verify`
    /// (and with it every `restore`, which verifies its own safety backup)
    /// would immediately reject the resulting archive as corrupt. So
    /// `backup` has to refuse by itself, instead of producing an archive
    /// that never passes its own verification.
    // Unix only, and the doc comment above says why without meaning to: a
    // backslash is an ordinary character in a file name *there*. On Windows
    // it separates path components, so `slot1\campaign.sav` does not create
    // the hostile file this test needs — it creates a directory `slot1`
    // holding an ordinary save, which `backup` is right to accept. The
    // guard stays relevant on both systems for archives written elsewhere;
    // `verify_rejects_backslash_components_in_entry_names` covers that side
    // and runs everywhere.
    #[cfg(unix)]
    #[test]
    fn backup_rejects_a_save_file_whose_name_contains_a_backslash() {
        let (_tmp, saves, backups) = save_fixture();
        std::fs::write(saves.join("slot1\\campaign.sav"), b"X").unwrap();

        let err = backup(&saves, &backups, None).unwrap_err();
        assert!(matches!(err, Error::CorruptBackup(_)), "{err:?}");
        assert!(list_backups(&backups).unwrap().is_empty(), "no half-written archive may be left behind");
    }

    /// Pure smoke test: regardless of whether `/proc` exists or a Steam
    /// process is running, the call must not crash. The actual detection
    /// behaviour depends on the running system and cannot be checked
    /// deterministically without mocking processes.
    #[test]
    fn steam_is_running_does_not_panic() {
        let _ = steam_is_running();
    }

    #[test]
    fn rename_changes_the_label_in_the_listing() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, Some("Kapitel 3")).unwrap();

        rename(&entry, &backups, Some("Vor dem Bossfight")).unwrap();

        let list = list_backups(&backups).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].label.as_deref(), Some("Vor dem Bossfight"));
    }

    /// The label is part of the file name too — whoever looks for the
    /// backup in the file manager should see the same name there as in the
    /// UI.
    #[test]
    fn rename_moves_archive_and_manifest_to_the_new_name() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, Some("Kapitel 3")).unwrap();
        let old_archive = entry.archive.clone();
        let old_manifest = entry.manifest.clone();

        let renamed = rename(&entry, &backups, Some("Vor dem Bossfight")).unwrap();

        assert!(!old_archive.exists(), "the old archive must disappear");
        assert!(!old_manifest.exists(), "the old manifest must disappear");
        assert!(renamed.archive.is_file());
        assert!(renamed.manifest.is_file());
        let stem = renamed.archive.file_stem().unwrap().to_str().unwrap();
        assert!(stem.ends_with("_Vor-dem-Bossfight"), "{stem}");
    }

    /// The timestamp is a backup's identity (the UI uses it to remember
    /// which backups were verified during this session). Renaming is pure
    /// labelling and must not shift it — and the archive must still pass
    /// verification afterwards.
    #[test]
    fn rename_keeps_created_at_and_a_verifiable_archive() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, Some("Kapitel 3")).unwrap();

        let renamed = rename(&entry, &backups, Some("Anderes")).unwrap();

        assert_eq!(renamed.created_at, entry.created_at);
        verify(&renamed).unwrap();
    }

    /// "Kapitel-3" and "Kapitel 3" yield the same file name stem
    /// (`sanitize_label` turns both into `Kapitel-3`). Then no renaming may
    /// happen at all — otherwise `unique_backup_name` would append a
    /// pointless `~1` to the backup, because its own name is already taken.
    #[test]
    fn rename_to_a_label_with_the_same_file_stem_keeps_the_file_names() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, Some("Kapitel-3")).unwrap();

        let renamed = rename(&entry, &backups, Some("Kapitel 3")).unwrap();

        assert_eq!(renamed.archive, entry.archive);
        assert_eq!(renamed.manifest, entry.manifest);
        assert_eq!(renamed.label.as_deref(), Some("Kapitel 3"), "the label itself changes all the same");
    }

    /// An empty label removes the labelling instead of creating a backup
    /// named "".
    #[test]
    fn rename_with_an_empty_label_removes_the_label() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, Some("Kapitel 3")).unwrap();

        let renamed = rename(&entry, &backups, None).unwrap();

        assert_eq!(renamed.label, None);
        let stem = renamed.archive.file_stem().unwrap().to_str().unwrap();
        assert_eq!(stem, timestamp_for_filename(&entry.created_at), "only the timestamp may remain");
    }

    /// Two backups from the same second with the same target label must not
    /// overwrite each other — the same rule as when creating them.
    #[test]
    fn rename_onto_an_occupied_name_appends_a_collision_counter() {
        let (_tmp, saves, backups) = save_fixture();
        let occupant = backup(&saves, &backups, Some("Ziel")).unwrap();
        let entry = BackupEntry {
            archive: backups.join("2026-09-16_180000_Anders.zip"),
            manifest: backups.join("2026-09-16_180000_Anders.json"),
            created_at: String::from("2026-09-16T18:00:00Z"),
            label: Some(String::from("Anders")),
        };
        std::fs::copy(&occupant.archive, &entry.archive).unwrap();
        let mut manifest = read_manifest(&occupant).unwrap();
        manifest.created_at = entry.created_at.clone();
        manifest.label = entry.label.clone();
        std::fs::write(&entry.manifest, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();

        // Both backups now aim at the same stem as soon as this one is
        // supposed to be called "Ziel" — but only if the timestamp matches
        // too.
        let taken = backups.join(format!(
            "{}_Ziel.zip",
            timestamp_for_filename(&entry.created_at)
        ));
        std::fs::write(&taken, b"belegt").unwrap();

        let renamed = rename(&entry, &backups, Some("Ziel")).unwrap();

        assert_ne!(renamed.archive, taken, "the occupied file must not be overwritten");
        assert_eq!(std::fs::read(&taken).unwrap(), b"belegt");
        let stem = renamed.archive.file_stem().unwrap().to_str().unwrap();
        assert!(stem.ends_with("_Ziel~1"), "{stem}");
    }

    /// `rename` writes the new manifest before it moves the archive (so
    /// that no state without a complete pair can arise). If the move then
    /// fails — for instance because the archive was deleted by hand in the
    /// meantime — this new manifest has to disappear again. Left behind, it
    /// would make `unique_backup_name` consider the name permanently taken
    /// and append a `~1` to every future rename onto this label.
    #[test]
    fn rename_removes_the_new_manifest_when_the_archive_cannot_be_moved() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, Some("Kapitel 3")).unwrap();
        std::fs::remove_file(&entry.archive).unwrap();

        let err = rename(&entry, &backups, Some("Blockiert")).unwrap_err();

        assert!(matches!(err, Error::Io { .. }), "{err:?}");
        let new_stem = format!("{}_Blockiert", timestamp_for_filename(&entry.created_at));
        assert!(
            !backups.join(format!("{new_stem}.json")).exists(),
            "the new manifest must be removed again"
        );
        assert!(entry.manifest.is_file(), "the old manifest stays untouched");
    }

    #[test]
    fn delete_removes_archive_and_manifest() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, Some("weg damit")).unwrap();

        delete(&entry).unwrap();

        assert!(!entry.archive.exists());
        assert!(!entry.manifest.exists());
        assert!(list_backups(&backups).unwrap().is_empty());
    }

    /// Anyone who hits "Delete" twice — or removed the backup by hand in
    /// the meantime — should not see an error: the goal has been reached.
    #[test]
    fn delete_is_idempotent() {
        let (_tmp, saves, backups) = save_fixture();
        let entry = backup(&saves, &backups, None).unwrap();

        delete(&entry).unwrap();
        delete(&entry).unwrap();
    }

    /// Builds a ZIP the way a foreign launcher would leave one behind:
    /// entry names exactly as given, no manifest next to it.
    fn foreign_zip(path: &Path, files: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        for (name, content) in files {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap();
    }

    fn manifest_of(entry: &BackupEntry) -> BackupManifest {
        serde_json::from_str(&std::fs::read_to_string(&entry.manifest).unwrap()).unwrap()
    }

    /// An imported archive must be indistinguishable from one this program
    /// created itself — including a manifest that `verify` accepts.
    /// Otherwise the imported backup would be a second class of backup that
    /// `restore` (which verifies) could never use.
    #[test]
    fn import_archive_creates_a_verified_backup() {
        let (tmp, _saves, backups) = save_fixture();
        let archive = tmp.path().join("fremd.zip");
        foreign_zip(&archive, &[("profile.cfg", b"PROFIL"), ("slot1/campaign.cfg", b"KAMPAGNE")]);

        let entry = import_archive(&archive, &backups, Some("von Nexus")).unwrap();

        verify(&entry).unwrap();
        let manifest = manifest_of(&entry);
        assert_eq!(manifest.files.len(), 2);
        assert!(manifest.files.contains_key("profile.cfg"));
        assert!(manifest.files.contains_key("slot1/campaign.cfg"));
        assert_eq!(manifest.source, archive.display().to_string());
    }

    #[test]
    fn imported_backup_appears_in_the_list() {
        let (tmp, _saves, backups) = save_fixture();
        let archive = tmp.path().join("fremd.zip");
        foreign_zip(&archive, &[("profile.cfg", b"PROFIL")]);

        let entry = import_archive(&archive, &backups, None).unwrap();

        let listed = list_backups(&backups).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].archive, entry.archive);
    }

    /// Foreign launchers pack their saves below some directory of their
    /// own. Kept as is, `restore` would create that directory inside the
    /// save directory instead of replacing the files in it.
    #[test]
    fn import_archive_strips_the_common_directory_prefix() {
        let (tmp, _saves, backups) = save_fixture();
        let archive = tmp.path().join("fremd.zip");
        foreign_zip(
            &archive,
            &[("Backup/Main/profile.cfg", b"PROFIL"), ("Backup/Main/slot1/campaign.cfg", b"KAMPAGNE")],
        );

        let entry = import_archive(&archive, &backups, None).unwrap();

        let manifest = manifest_of(&entry);
        assert!(manifest.files.contains_key("profile.cfg"), "{:?}", manifest.files.keys());
        assert!(manifest.files.contains_key("slot1/campaign.cfg"), "{:?}", manifest.files.keys());
    }

    /// Only a directory *all* entries share may be stripped. A single file
    /// next to the save directory would otherwise silently move the rest of
    /// the archive one level up.
    #[test]
    fn import_archive_keeps_paths_when_entries_share_no_directory() {
        let (tmp, _saves, backups) = save_fixture();
        let archive = tmp.path().join("fremd.zip");
        foreign_zip(&archive, &[("Main/profile.cfg", b"PROFIL"), ("liesmich.txt", b"HINWEIS")]);

        let entry = import_archive(&archive, &backups, None).unwrap();

        let manifest = manifest_of(&entry);
        assert!(manifest.files.contains_key("Main/profile.cfg"), "{:?}", manifest.files.keys());
        assert!(manifest.files.contains_key("liesmich.txt"), "{:?}", manifest.files.keys());
    }

    /// Some Windows packers write '\' as the separator. Taken literally,
    /// `validate_entry_name` would reject the archive (and a restore would
    /// create one file with a backslash in its name).
    #[test]
    fn import_archive_normalizes_backslash_separators() {
        let (tmp, _saves, backups) = save_fixture();
        let archive = tmp.path().join("fremd.zip");
        foreign_zip(&archive, &[("profile.cfg", b"PROFIL"), ("slot1\\campaign.cfg", b"KAMPAGNE")]);

        let entry = import_archive(&archive, &backups, None).unwrap();

        let manifest = manifest_of(&entry);
        assert!(manifest.files.contains_key("slot1/campaign.cfg"), "{:?}", manifest.files.keys());
    }

    #[test]
    fn import_archive_recognizes_uppercase_savegame_extensions() {
        let (tmp, _saves, backups) = save_fixture();
        let archive = tmp.path().join("fremd.zip");
        foreign_zip(&archive, &[("PROFILE.SAV", b"PROFIL")]);

        import_archive(&archive, &backups, None).unwrap();
    }

    /// The most likely mistake is picking a mod archive in the file dialog.
    #[test]
    fn import_archive_rejects_an_archive_without_savegame_files() {
        let (tmp, _saves, backups) = save_fixture();
        let archive = tmp.path().join("mod.zip");
        foreign_zip(&archive, &[("cool_mod.pak", b"PAKDATEN"), ("liesmich.txt", b"HINWEIS")]);

        let err = import_archive(&archive, &backups, None).unwrap_err();

        assert!(matches!(err, Error::NoSaveInArchive(_)), "{err:?}");
    }

    #[test]
    fn import_archive_rejects_an_escaping_entry_name() {
        let (tmp, _saves, backups) = save_fixture();
        let archive = tmp.path().join("boese.zip");
        foreign_zip(&archive, &[("profile.cfg", b"PROFIL"), ("../entkommen.cfg", b"BOESE")]);

        let err = import_archive(&archive, &backups, None).unwrap_err();

        assert!(matches!(err, Error::UnusableArchive(_)), "{err:?}");
    }

    /// A symlink entry is the archive counterpart of the symlink rule that
    /// `backup` and `restore` follow: a link stored in the archive would,
    /// once restored, let a later write land outside the save directory.
    #[test]
    fn import_archive_rejects_a_symlink_entry() {
        let (tmp, _saves, backups) = save_fixture();
        let archive = tmp.path().join("boese.zip");
        let file = std::fs::File::create(&archive).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        zip.start_file("profile.cfg", opts).unwrap();
        zip.write_all(b"PROFIL").unwrap();
        zip.add_symlink("slot1.cfg", "/etc/passwd", opts).unwrap();
        zip.finish().unwrap();

        let err = import_archive(&archive, &backups, None).unwrap_err();

        assert!(matches!(err, Error::UnusableArchive(_)), "{err:?}");
    }

    /// Two entries that differ only in the separator collapse into one name
    /// during normalization. Unpacked in order, the second would silently
    /// overwrite the first and the backup would claim a state that never
    /// existed that way — so the archive is rejected instead.
    ///
    /// (Two *identical* names cannot reach this point: `ZipArchive` keeps
    /// its entries in a map keyed by name and already collapses them while
    /// opening the file.)
    #[test]
    fn import_archive_rejects_names_that_collide_after_normalization() {
        let (tmp, _saves, backups) = save_fixture();
        let archive = tmp.path().join("doppelt.zip");
        foreign_zip(&archive, &[("slot1/campaign.cfg", b"ERSTER"), ("slot1\\campaign.cfg", b"ZWEITER")]);

        let err = import_archive(&archive, &backups, None).unwrap_err();

        assert!(matches!(err, Error::UnusableArchive(_)), "{err:?}");
    }

    #[test]
    fn import_archive_reports_an_unreadable_archive_clearly() {
        let (tmp, _saves, backups) = save_fixture();
        let archive = tmp.path().join("kaputt.zip");
        std::fs::write(&archive, b"das ist kein ZIP").unwrap();

        let err = import_archive(&archive, &backups, None).unwrap_err();

        let message = err.to_string();
        assert!(matches!(err, Error::UnusableArchive(_)), "{err:?}");
        assert!(
            !message.contains("invalid") && !message.contains("Invalid"),
            "the message must be composed from the catalogue, not carry the raw zip message: {message}"
        );
    }

    /// Without a label an imported backup would only be a timestamp in the
    /// list, indistinguishable from the ones created here.
    #[test]
    fn import_archive_labels_the_backup_with_the_file_name() {
        let (tmp, _saves, backups) = save_fixture();
        let archive = tmp.path().join("GameSaveManager 2026.zip");
        foreign_zip(&archive, &[("profile.cfg", b"PROFIL")]);

        let entry = import_archive(&archive, &backups, None).unwrap();

        assert_eq!(entry.label.as_deref(), Some("GameSaveManager 2026"));
        let stem = entry.archive.file_stem().unwrap().to_str().unwrap();
        assert!(stem.ends_with("_GameSaveManager-2026"), "{stem}");
    }

    #[test]
    fn import_archive_prefers_the_given_label() {
        let (tmp, _saves, backups) = save_fixture();
        let archive = tmp.path().join("GameSaveManager 2026.zip");
        foreign_zip(&archive, &[("profile.cfg", b"PROFIL")]);

        let entry = import_archive(&archive, &backups, Some("Kapitel 3")).unwrap();

        assert_eq!(entry.label.as_deref(), Some("Kapitel 3"));
    }

    /// The declared uncompressed size is checked before a single entry is
    /// unpacked: a zip bomb must not be able to fill the disk first and be
    /// noticed afterwards.
    #[test]
    fn import_archive_rejects_an_oversized_archive_before_unpacking() {
        let (tmp, _saves, backups) = save_fixture();
        let archive = tmp.path().join("gross.zip");
        foreign_zip(&archive, &[("profile.cfg", &[b'X'; 4096])]);

        let err = import_archive_limited(&archive, &backups, None, 1024).unwrap_err();

        assert!(matches!(err, Error::UnusableArchive(_)), "{err:?}");
        // A limit below one MiB has to be named in bytes; "0 MiB" would
        // leave the user with no idea what the limit actually is. The
        // wording around the number depends on the active language, only
        // the number itself does not.
        assert!(err.to_string().contains("1024"), "{err}");
    }

    /// A rejected archive must not leave a half-finished backup behind:
    /// the list is what the user restores from.
    #[test]
    fn import_archive_leaves_nothing_behind_when_it_rejects_the_archive() {
        let (tmp, _saves, backups) = save_fixture();
        let archive = tmp.path().join("boese.zip");
        foreign_zip(&archive, &[("profile.cfg", b"PROFIL"), ("../entkommen.cfg", b"BOESE")]);

        import_archive(&archive, &backups, None).unwrap_err();

        assert!(list_backups(&backups).unwrap().is_empty());
        let stray = backups.is_dir()
            && std::fs::read_dir(&backups).unwrap().next().is_some();
        assert!(!stray, "no file may remain under the backup directory");
    }
}

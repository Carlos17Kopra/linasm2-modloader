//! Import of mods from archives or from single `.pak` files.
//!
//! Design: an imported pak is never moved again; `pak_config.yaml` stays the
//! single source of truth for order and activation state. An import appends a
//! new entry, disabled, at the end, so it never changes a running setup.

use crate::error::{Error, Result};
use crate::library::{hash_file, Library, ModInfo};
use crate::pak_config::{PakConfig, PakEntry};
use crate::paths::GamePaths;
use crate::platform::{Current, Platform};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Result of an import.
#[derive(Debug, PartialEq)]
pub struct ImportOutcome {
    pub pak: String,
    /// Set when a content-identical mod (according to the library) was
    /// already present.
    pub duplicate_of: Option<String>,
}

/// Extracts all `.pak` files from an archive into `into`.
///
/// Supported are `.zip`, `.7z`, `.rar` and a single `.pak` file (which is
/// then returned unchanged, without being copied into `into`). The archive's
/// directory structure is flattened — mod archives like to wrap paks in
/// folders, but only the file itself matters to the engine.
///
/// If two entries carry the same file name in different folders (for example
/// `a/mod.pak` and `b/mod.pak` — typical for archives with several
/// installation variants), both are kept: the second one gets a sequential
/// number before the extension (`mod.pak`, `mod_2.pak`, `mod_3.pak`, ...)
/// instead of silently displacing the first.
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

/// Recognizes `.pak` files regardless of the extension's case.
fn is_pak_file(name: &str) -> bool {
    name.to_lowercase().ends_with(".pak")
}

/// Strips a `.pak` extension, case-insensitively.
///
/// Uses `str::get` instead of direct byte indexing: for a name that does not
/// end in `.pak`, the computed cut point could otherwise fall in the middle
/// of a multi-byte UTF-8 character (`"a€€"`, for example) — `get` then
/// returns `None` instead of panicking.
///
/// Public so that a caller outside this module (for example
/// `AppState::register_unknown_pak` in the app layer, for a pak copied in by
/// hand without a call to `import_pak`) can derive the same display name as
/// `import_pak` does itself (see its `display_name`), instead of implementing
/// that derivation a second time — and potentially differently, see how
/// `str::trim_end_matches` strips repeatedly on `a.pak.pak`.
pub fn strip_pak_suffix(name: &str) -> &str {
    let cut = name.len().saturating_sub(4);
    match name.get(cut..) {
        Some(suffix) if suffix.eq_ignore_ascii_case(".pak") => &name[..cut],
        _ => name,
    }
}

/// Finds a still-free name in `into` for a file that is about to be placed
/// there. If `desired` is free, it is used unchanged; otherwise — as in
/// `unique_pak_name` — a sequential number is appended before the extension
/// (`mod.pak` -> `mod_2.pak`, `mod_3.pak`, ...). Unlike `unique_pak_name`,
/// this variant only looks at the contents of `into` itself (not at the mods
/// directory or the configuration) — it serves the extraction step, not the
/// import proper.
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
        // Only the file name from the archive entry is used — that also
        // defends against zip slip, since no directory component from the
        // archive reaches the target path. An entry without a file component
        // (a pure directory entry, for example) is skipped.
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

/// `.rar` via an external tool — the `unrar` crate bundles non-free source
/// code and is not viable for a public release.
fn extract_rar(archive: &Path, into: &Path) -> Result<Vec<PathBuf>> {
    let raw = into.join("__rar");
    std::fs::create_dir_all(&raw).map_err(|e| Error::io(&raw, e))?;

    let tools: [(&str, &[&str]); 3] = [
        ("unar", &["-quiet", "-force-overwrite", "-output-directory"]),
        ("7z", &["x", "-y"]),
        ("7zz", &["x", "-y"]),
    ];

    // If a tool was actually found and run but failed, the problem is the
    // archive — not a missing tool. `NoRarTool`s "please install unar/7zip"
    // would then be a misleading recommendation.
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
        // The tool does not talk to the user — only our own result
        // (success or failure) counts.
        cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        let status = cmd.status().map_err(|e| Error::io(archive, e))?;
        if status.success() {
            return collect_paks_recursively(&raw, into);
        }
    }

    if tool_ran {
        Err(Error::io(
            archive,
            std::io::Error::new(std::io::ErrorKind::InvalidData, crate::t!("error.archive_unpack_failed")),
        ))
    } else {
        Err(Error::NoRarTool)
    }
}

/// Searches an extracted tree for `.pak` files and puts them flat into
/// `into`. If two files from different subfolders end up with the same
/// target file name (for example `Option A/mod.pak` and `Option B/mod.pak`),
/// both are kept: the second one gets a sequential number (`mod_2.pak`,
/// `mod_3.pak`, ...) instead of displacing the first.
///
/// Every directory is sorted by path before it is processed, so the result is
/// deterministic regardless of the (unspecified) order `read_dir` returns.
/// Symlinks are skipped: `unar`/`7z` can restore links from an archive, and a
/// link pointing at `/` or at the game directory would otherwise cause
/// foreign files from outside the extraction folder to be collected.
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

/// Finds a pak name that is still free both in the mods directory and in the
/// configuration. If `desired` collides with an already existing file or an
/// already registered pak, a sequential number is appended before the
/// extension (`mod.pak` -> `mod_2.pak`, `mod_3.pak`, ...). That way an import
/// never overwrites an existing file or an existing entry — not even when its
/// content is unknown to the library (placed there by hand, for example, and
/// therefore not covered by the hash duplicate check).
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

/// Places a pak in the game directory and appends it, disabled, at the end of
/// the configuration.
///
/// If the content is already known to the library (by hash), nothing is
/// copied or registered — `duplicate_of` names the existing pak. Otherwise,
/// if the target file name collides with an existing file or an existing
/// entry, an alternative name is assigned (see `unique_pak_name`) instead of
/// overwriting anything.
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
                std::io::Error::new(std::io::ErrorKind::InvalidInput, crate::t!("error.invalid_pak_filename")),
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

    // rename fails across device boundaries — copy in that case.
    if std::fs::rename(pak, &target).is_err() {
        if let Err(e) = std::fs::copy(pak, &target) {
            // A failed copy may have left an incomplete file behind — it
            // must not stay there, or `unique_pak_name` would consider
            // that name taken forever.
            let _ = std::fs::remove_file(&target);
            return Err(Error::io(&target, e));
        }
        let _ = std::fs::remove_file(pak);
    }

    let display_name = strip_pak_suffix(&final_name).replace(['_', '-'], " ");

    // Stat freshly after placing the file instead of reusing the metadata
    // read before the move or copy: `fs::copy` gives no guarantee that the
    // modification time is preserved, and `detect_altered`s cheap pre-filter
    // needs to know the actual state of the file as it now lies in the mods
    // directory. If the stat call fails for once, `mtime` stays `None` — the
    // next comparison then hashes once instead of blindly trusting the
    // missing information.
    let mtime = std::fs::metadata(&target)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());

    // The position the entry will get in `cfg.entries` just below — noted
    // here already so that `last_known_position` is right from the start and
    // does not have to wait for the next `persist()` (see
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
            // An import always appends disabled at the end (see the doc
            // comment above) — that is at the same time the first known
            // state for `PakConfig::reconcile`s restoration.
            last_known_disabled: true,
            last_known_position: Some(position),
            mtime,
            known_altered: false,
        },
    );

    cfg.entries.push(PakEntry { pak: final_name.clone(), disabled: true });

    Ok(ImportOutcome { pak: final_name, duplicate_of: None })
}

/// RFC 3339 timestamp for "now" in UTC, without a date crate.
pub fn now_rfc3339() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    format_utc(seconds)
}

/// Formats a Unix time (seconds since the epoch, UTC) as RFC 3339.
///
/// Calendar arithmetic follows Howard Hinnant's "civil_from_days" algorithm.
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

    /// Builds a game directory that `GamePaths::from_game_dir` accepts.
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
        assert!(paks[0].ends_with("tief.pak"), "the directory structure is flattened");
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
        // An archive must never write outside the target directory.
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("boese.zip");
        zip_with(&[("../../entkommen.pak", b"DATEN")], &archive);

        let target = tmp.path().join("out");
        std::fs::create_dir_all(&target).unwrap();
        let paks = extract_paks(&archive, &target).unwrap();

        for p in &paks {
            assert!(p.starts_with(&target), "the file landed outside: {}", p.display());
        }
        assert!(!tmp.path().join("entkommen.pak").exists());
    }

    /// Two entries with the same file name from different folders (two
    /// installation variants, for example) must both survive — the second
    /// one gets a sequential number (see the doc comment on `extract_paks`)
    /// instead of silently displacing the first.
    #[test]
    fn same_basename_from_different_folders_gets_a_numbered_variant() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("mod.zip");
        zip_with(&[("a/mod.pak", b"ERSTER"), ("b/mod.pak", b"ZWEITER")], &archive);

        let target = tmp.path().join("out");
        std::fs::create_dir_all(&target).unwrap();
        let paks = extract_paks(&archive, &target).unwrap();

        assert_eq!(paks.len(), 2, "both variants have to survive");
        assert!(paks[0].ends_with("mod.pak"));
        assert!(paks[1].ends_with("mod_2.pak"), "the second hit gets a sequential number");
        assert_eq!(std::fs::read(&paks[0]).unwrap(), b"ERSTER");
        assert_eq!(std::fs::read(&paks[1]).unwrap(), b"ZWEITER");
    }

    /// `collect_paks_recursively` (used for `.7z`/`.rar`) must apply the same
    /// numbering as the zip path, and must produce a deterministic result
    /// regardless of the (unspecified) `read_dir` order.
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
            "order and naming have to be deterministic"
        );
        assert_eq!(std::fs::read(into.join("mod.pak")).unwrap(), b"ERSTER");
        assert_eq!(std::fs::read(into.join("mod_2.pak")).unwrap(), b"ZWEITER");
        assert_eq!(std::fs::read(into.join("y.pak")).unwrap(), b"TIEF");
    }

    /// `unar`/`7z` can restore symlinks from an archive. A link to a foreign
    /// directory must not be followed while collecting the paks, or a
    /// malicious archive could use a link to `/` or to the game directory to
    /// collect foreign files.
    // Unix only for the fixture, not for the behaviour: creating a
    // symlink needs Developer Mode or elevation on Windows. The guard
    // under test is platform-neutral — it rests on `symlink_metadata`,
    // which reports a link as a link on both systems.
    #[cfg(unix)]
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

        assert!(result.is_empty(), "a symlink to a foreign directory must not be followed");
        assert!(!into.join("fremd.pak").exists());
    }

    /// A name that does not end in `.pak` and whose last bytes would fall in
    /// the middle of a multi-byte UTF-8 character must not crash
    /// `strip_pak_suffix`.
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

    /// No real tool can successfully extract a `.rar` file with invalid
    /// content. The expected result depends on whether the test environment
    /// provides a tool at all: without `unar`/`7z`/`7zz` in PATH the right
    /// message is `NoRarTool` ("please install"); if one is present but fails
    /// on the invalid archive, that same message would be misleading — there
    /// an `Error::Io` pointing at the broken archive has to come back.
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
                "a tool that was found but failed has to be reported as an I/O error, not as NoRarTool: {error:?}"
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
        assert!(cfg.entries.last().unwrap().disabled, "an import must not enable anything");
        assert_eq!(cfg.entries[0].pak, "alt.pak", "existing entries stay in front");
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
        assert_eq!(cfg.entries.len(), 1, "a duplicate is not entered a second time");
    }

    /// Two paks with different content but the same file name: the second
    /// import must not overwrite the file that is already there, even when
    /// its content is unknown to the library (placed by hand, for example,
    /// rather than imported).
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

        assert_ne!(outcome.pak, "mod.pak", "the name has to give way so that nothing is overwritten");
        assert_eq!(
            std::fs::read(paths.mods_dir().join("mod.pak")).unwrap(),
            b"BEREITS DA",
            "the existing file must not be changed"
        );
        assert_eq!(std::fs::read(paths.mods_dir().join(&outcome.pak)).unwrap(), b"NEUER INHALT");
        assert_eq!(cfg.entries.last().unwrap().pak, outcome.pak);
    }

    #[test]
    fn formats_unix_time_as_rfc3339() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(1_757_700_000), "2025-09-12T18:00:00Z");
        // Leap year.
        assert_eq!(format_utc(1_709_164_800), "2024-02-29T00:00:00Z");
        // Year boundary, checked independently with
        // `date -u -d @<epoch> +%FT%TZ`.
        assert_eq!(format_utc(1_767_225_599), "2025-12-31T23:59:59Z");
        assert_eq!(format_utc(1_767_225_600), "2026-01-01T00:00:00Z");
    }
}

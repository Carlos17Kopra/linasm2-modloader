use crate::atomic::write_atomic;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Metadata about an imported mod. The pak file itself lives in the game
/// directory; all that is recorded here is what we know about it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModInfo {
    pub pak: String,
    pub name: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub nexus_id: Option<u32>,
    #[serde(default)]
    pub notes: Option<String>,
    /// blake3 of the pak contents — detects duplicates and outside changes.
    pub hash: String,
    pub size: u64,
    /// RFC 3339 timestamp.
    pub imported_at: String,
    /// Path of the archive or file the mod was imported from.
    #[serde(default)]
    pub source: Option<String>,

    /// The activation state `pak_config.yaml` carried for this pak at the
    /// last successful `persist()`. It lets `PakConfig::reconcile` restore a
    /// pak that vanished and later reappeared (Spec §9 R3, e.g. after a
    /// Steam update) to its previous state, instead of appending it enabled
    /// at the end like a pak never seen before. `#[serde(default)]`, so that
    /// older `library.json` files without this field still load.
    #[serde(default)]
    pub last_known_disabled: bool,
    /// The position in `pak_config.yaml` this pak held at the last
    /// successful `persist()` — see `last_known_disabled`.
    ///
    /// `Option`, not `usize`: a `library.json` written before this field
    /// existed (or a pak that was never part of the configuration) has to
    /// arrive as "no position history known", not as "position 0". With a
    /// plain `usize` and `#[serde(default)]`, every legacy entry would jump
    /// to the top of the configuration on the first `reconcile` after
    /// reappearing (and several such entries in a row would even end up in
    /// reverse order) — a silent change to the load order that nobody asked
    /// for.
    #[serde(default)]
    pub last_known_position: Option<usize>,

    /// Size and modification time of the pak file as of the last
    /// verification by `detect_altered`. Serves as a cheap prefilter (one
    /// `stat` call instead of a full hash over a file that may be several
    /// gigabytes): if the size or modification time on disk differs from
    /// these values, an actual hash comparison is worth it; if both match, a
    /// hash comparison is unnecessary. When the content last checked is
    /// known to be altered (see `known_altered`), these values deliberately
    /// reflect the *altered* state, not the originally imported one — only
    /// that keeps the prefilter effective for a permanently altered pak.
    /// `#[serde(default)]`, so that older `library.json` files without this
    /// field still load (the first run after that hashes once, rather than
    /// blindly trusting the mismatch).
    #[serde(default)]
    pub mtime: Option<u64>,

    /// `true` when `detect_altered` last confirmed that the content differs
    /// from the original `hash` ("altered outside the loader", Spec §6.3).
    /// `hash` itself stays untouched — it remains the fingerprint of the
    /// originally imported version for `find_by_hash`'s duplicate detection;
    /// otherwise a later re-import of exactly that original file would no
    /// longer be recognized as a duplicate. Together with `size`/`mtime`
    /// refreshed to the *altered* state, this flag makes it possible to
    /// repeat the warning from the cache while the content stays as it is
    /// (but still differs), without re-hashing the file on every run.
    /// `#[serde(default)]`, so that older `library.json` files without this
    /// field still load.
    #[serde(default)]
    pub known_altered: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Library {
    /// The key is the pak file name.
    #[serde(default)]
    pub mods: BTreeMap<String, ModInfo>,
}

impl Library {
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| {
                Error::io(
                    path,
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("ungültiges JSON (Zeile {}, Spalte {})", e.line(), e.column()),
                    ),
                )
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        // Unreachable for the current field types (String, Option<_>, u64,
        // u32, BTreeMap<String, _> cannot fail); the raw error is discarded
        // on purpose so that the message stays purely German.
        let json = serde_json::to_string_pretty(self).map_err(|_| {
            Error::io(
                path,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Bibliothek konnte nicht als JSON serialisiert werden",
                ),
            )
        })?;
        write_atomic(path, &format!("{json}\n"))
    }

    /// Returns the first entry with this hash. When two paks share a hash
    /// (e.g. the same file imported under two names), `BTreeMap`'s key order
    /// makes the result deterministically the alphabetically first pak file
    /// name.
    pub fn find_by_hash(&self, hash: &str) -> Option<&ModInfo> {
        self.mods.values().find(|m| m.hash == hash)
    }

    /// Detects paks whose content was altered outside the loader (Spec §6.3,
    /// third reconciliation case: the hash differs from `library.json`).
    /// Only paks already known to the library are checked — for an unknown
    /// pak there is nothing to compare against (that is already covered by
    /// `PakConfig::reconcile`'s `added` case; a pak copied in by hand only
    /// gets a history of its own through `AppState::persist`, see that
    /// function's doc comment).
    ///
    /// Hashing every file in full — and they can run to several gigabytes —
    /// on every single call is not defensible. So a cheap prefilter comes
    /// first: compare size and modification time against the last confirmed
    /// values (`ModInfo::size`/`ModInfo::mtime`) — one `stat` call instead
    /// of a full read. Only when one of the two differs is the file actually
    /// hashed.
    ///
    /// Three outcomes after an actual hash:
    /// - The hash confirms the originally imported content (e.g. after a
    ///   `touch` that changed nothing, or because a `library.json` predates
    ///   `ModInfo::mtime` and the field is therefore `None`): `size`/`mtime`
    ///   are refreshed and `known_altered` is cleared if it was set.
    /// - The hash differs and the pak was not considered altered until now:
    ///   reported as "altered outside the loader", AND `size`/`mtime` are
    ///   refreshed to the *altered* state and `known_altered` is set.
    /// - A pak already known to be altered (`known_altered`) whose size and
    ///   modification time have not changed since the last verification (so
    ///   the prefilter never fires at all): still reported, without hashing
    ///   again — see the dedicated check for that right after the prefilter.
    ///
    /// In all three cases `hash` itself stays untouched — it remains the
    /// fingerprint of the originally imported version for duplicate
    /// detection on import (`find_by_hash`). Without refreshing
    /// `size`/`mtime` in the altered case too (the very reason
    /// `known_altered` exists), a pak that permanently differs from the
    /// original would be hashed in full on *every* call, forever — failing
    /// to recognize exactly that case was the original bug.
    ///
    /// `cache_refreshed` in the result reports whether anything about the
    /// stored state (the prefilter values or `known_altered`) changed: the
    /// caller (`AppState::open`) then rewrites `library.json` right away,
    /// instead of keeping the change in memory only and hashing again on
    /// every further call — read-only ones included.
    ///
    /// A file that `present` says should exist but that can no longer be
    /// read is skipped silently — that is the case `PakConfig::reconcile`'s
    /// `removed` already reports. Any other I/O error during verification
    /// (missing read permission, a failed hash) does not abort the whole
    /// verification: Spec §6.3 describes this third case explicitly as a
    /// marker, not as a hard requirement — a single unreadable pak must not
    /// bring down even purely read-only commands such as `paths`. Such cases
    /// end up as a German message in `warnings` instead.
    pub fn detect_altered(&mut self, mods_dir: &Path, present: &[String]) -> Result<AlteredReport> {
        let mut altered = Vec::new();
        let mut warnings = Vec::new();
        let mut cache_refreshed = false;

        for pak in present {
            let Some(info) = self.mods.get(pak) else { continue };

            let path = mods_dir.join(pak);
            let metadata = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    warnings.push(format!(
                        "{pak}: Prüfung auf Veränderung übersprungen (nicht lesbar: {e})"
                    ));
                    continue;
                }
            };

            let size = metadata.len();
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs());

            if size == info.size && mtime == info.mtime {
                // The prefilter reports no change since the last
                // verification. If the content was already known to be
                // altered back then, it stays altered — without hashing
                // again. That is the real purpose of `known_altered`: the
                // warning keeps appearing on every run (it is the signal
                // to the user), but the expensive hash runs only once per
                // actual change.
                if info.known_altered {
                    altered.push(pak.clone());
                }
                continue;
            }

            let hash = match hash_file(&path) {
                Ok(h) => h,
                Err(e) => {
                    warnings.push(format!("{pak}: Prüfung auf Veränderung übersprungen ({e})"));
                    continue;
                }
            };
            if hash == info.hash {
                if let Some(entry) = self.mods.get_mut(pak) {
                    entry.size = size;
                    entry.mtime = mtime;
                    if entry.known_altered {
                        entry.known_altered = false;
                    }
                    cache_refreshed = true;
                }
            } else {
                altered.push(pak.clone());
                if let Some(entry) = self.mods.get_mut(pak) {
                    // `hash` stays the fingerprint of the originally
                    // imported version (duplicate detection) — only size
                    // and modification time are refreshed to the
                    // *altered* state, so that the prefilter works again
                    // on the next run (see the doc comment above).
                    entry.size = size;
                    entry.mtime = mtime;
                    entry.known_altered = true;
                    cache_refreshed = true;
                }
            }
        }

        Ok(AlteredReport { altered, cache_refreshed, warnings })
    }
}

/// Result of `Library::detect_altered`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AlteredReport {
    /// Paks whose hash differs from the last known state ("altered outside
    /// the loader", Spec §6.3).
    pub altered: Vec<String>,
    /// `true` when, for at least one pak, size and modification time no
    /// longer matched in the cheap prefilter but the hash confirmed the
    /// content — the caller should then rewrite `library.json` so that
    /// future runs can make use of the prefilter again.
    pub cache_refreshed: bool,
    /// German messages about paks that could not be verified at all
    /// (missing read permission or similar) — informational, not a failure
    /// of the whole verification.
    pub warnings: Vec<String>,
}

/// blake3 hash of a file, read as a stream — paks can be gigabytes in size.
pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path).map_err(|e| Error::io(path, e))?;
    let mut hasher = blake3::Hasher::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| Error::io(path, e))?;
    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(pak: &str, hash: &str) -> ModInfo {
        ModInfo {
            pak: pak.to_string(),
            name: pak.trim_end_matches(".pak").to_string(),
            author: None,
            version: None,
            nexus_id: None,
            notes: None,
            hash: hash.to_string(),
            size: 42,
            imported_at: "2026-09-12T18:00:00Z".to_string(),
            source: None,
            last_known_disabled: false,
            last_known_position: Some(0),
            mtime: None,
            known_altered: false,
        }
    }

    #[test]
    fn hash_is_stable_and_distinguishes_content() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.bin");
        let b = dir.path().join("b.bin");
        std::fs::write(&a, b"identisch").unwrap();
        std::fs::write(&b, b"identisch").unwrap();

        assert_eq!(hash_file(&a).unwrap(), hash_file(&b).unwrap());

        std::fs::write(&b, b"anders").unwrap();
        assert_ne!(hash_file(&a).unwrap(), hash_file(&b).unwrap());
    }

    #[test]
    fn load_with_missing_file_yields_empty_library() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::load(&dir.path().join("gibt_es_nicht.json")).unwrap();
        assert!(lib.mods.is_empty());
    }

    #[test]
    fn save_and_load_are_inverses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        let mut lib = Library::default();
        lib.mods.insert("a.pak".into(), info("a.pak", "hash-a"));

        lib.save(&path).unwrap();

        assert_eq!(Library::load(&path).unwrap().mods, lib.mods);
    }

    #[test]
    fn finds_duplicate_by_hash() {
        let mut lib = Library::default();
        lib.mods.insert("a.pak".into(), info("a.pak", "hash-a"));

        assert_eq!(lib.find_by_hash("hash-a").map(|m| m.pak.as_str()), Some("a.pak"));
        assert!(lib.find_by_hash("unbekannt").is_none());
    }

    /// BTreeMap iterates in key order; when two paks share a hash (e.g. the
    /// same mod imported under two file names), the result is therefore
    /// deterministically the alphabetically first pak file name rather than
    /// an arbitrary pick.
    #[test]
    fn finds_first_pak_by_key_order_on_shared_hash() {
        let mut lib = Library::default();
        lib.mods.insert("z.pak".into(), info("z.pak", "gemeinsamer-hash"));
        lib.mods.insert("a.pak".into(), info("a.pak", "gemeinsamer-hash"));

        assert_eq!(
            lib.find_by_hash("gemeinsamer-hash").map(|m| m.pak.as_str()),
            Some("a.pak"),
            "on an identical hash the alphabetically first pak name must win"
        );
    }

    #[test]
    fn corrupt_json_is_reported_as_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        std::fs::write(&path, "{kein json").unwrap();

        let err = Library::load(&path).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains(path.to_str().unwrap()),
            "the error message must contain the path: {message}"
        );
        assert!(
            !message.contains("expected") && !message.contains("invalid"),
            "the error message should be in German, not carry the raw serde_json message: {message}"
        );
    }

    // --- detect_altered ---------------------------------------------------

    fn write_pak(mods_dir: &Path, name: &str, content: &[u8]) -> std::fs::Metadata {
        std::fs::create_dir_all(mods_dir).unwrap();
        let path = mods_dir.join(name);
        std::fs::write(&path, content).unwrap();
        std::fs::metadata(&path).unwrap()
    }

    fn info_matching(pak: &str, metadata: &std::fs::Metadata, hash: &str) -> ModInfo {
        let mut m = info(pak, hash);
        m.size = metadata.len();
        m.mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        m
    }

    /// Proves not just that nothing is reported as "altered", but that the
    /// cheap prefilter really does keep the file from being hashed at all:
    /// without read permission an actual hash attempt would have to fail and
    /// leave a warning behind (see
    /// `detect_altered_warns_instead_of_failing_on_an_unreadable_pak`
    /// below) — so if `warnings` stays empty, `hash_file` was never called.
    /// An assertion that only checks `altered.is_empty()` would stay green
    /// even if the prefilter were accidentally removed and every file hashed
    /// on every call — and that is precisely the property which, by this
    /// function's own doc comment, makes it usable automatically at all.
    #[test]
    fn detect_altered_ignores_an_unchanged_pak_without_hashing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.pak");
        let metadata = write_pak(dir.path(), "a.pak", b"INHALT");
        let hash = hash_file(&path).unwrap();

        let mut lib = Library::default();
        lib.mods.insert("a.pak".into(), info_matching("a.pak", &metadata, &hash));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
            if std::fs::read(&path).is_ok() {
                // When the test runs as root, the kernel bypasses the read
                // protection entirely — this method then cannot make the
                // behaviour observable.
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
                eprintln!("skipped: this process can apparently bypass read permissions (root?)");
                return;
            }
        }

        let report = lib.detect_altered(dir.path(), &["a.pak".into()]).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        }

        assert!(report.altered.is_empty());
        assert!(
            report.warnings.is_empty(),
            "an actual hash attempt would have had to fail on the revoked read permission \
             and would have become visible as a warning: {:?}",
            report.warnings
        );
    }

    /// The actual purpose of `detect_altered`: a pak whose content was
    /// replaced outside the loader (a different hash, with differing size
    /// and modification time) must be reported as altered (Spec §6.3, third
    /// reconciliation case).
    #[test]
    fn detect_altered_reports_a_pak_whose_content_was_replaced_outside_the_loader() {
        let dir = tempfile::tempdir().unwrap();
        let metadata = write_pak(dir.path(), "a.pak", b"URSPRUENGLICH");
        let hash = hash_file(&dir.path().join("a.pak")).unwrap();

        let mut lib = Library::default();
        lib.mods.insert("a.pak".into(), info_matching("a.pak", &metadata, &hash));

        // Replaced by hand, without the loader — size and content change.
        std::fs::write(dir.path().join("a.pak"), b"ERSETZT MIT ANDEREM INHALT").unwrap();

        let report = lib.detect_altered(dir.path(), &["a.pak".into()]).unwrap();

        assert_eq!(report.altered, vec!["a.pak"]);
        assert!(report.cache_refreshed, "the detection itself is a state change that has to be saved");
        assert!(report.warnings.is_empty());
        assert_eq!(
            lib.mods["a.pak"].hash, hash,
            "the stored hash stays that of the last imported version, otherwise a later \
             re-import of the same original file would no longer be recognized as a \
             duplicate"
        );
        assert!(lib.mods["a.pak"].known_altered, "the altered state has to be recorded");
        assert_eq!(
            lib.mods["a.pak"].size,
            std::fs::metadata(dir.path().join("a.pak")).unwrap().len(),
            "size/mtime are refreshed to the ALTERED state, otherwise every future run \
             would hash again (see detect_altered_repeats_the_advisory_...)"
        );
    }

    /// The actual fix for review item 2 (third instance): a pak already
    /// recognized as altered must repeat the warning on every further run —
    /// it is the signal to the user — but must not be hashed again for it,
    /// as long as size and modification time do not change. Checks both
    /// halves: that the message persists, AND that the second run really
    /// stops reading (demonstrated via revoked read permission — an actual
    /// second hash attempt would fail on that and become visible as a
    /// warning, see
    /// `detect_altered_warns_instead_of_failing_on_an_unreadable_pak`).
    #[test]
    fn detect_altered_repeats_the_advisory_without_hashing_again_once_confirmed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.pak");
        let metadata = write_pak(dir.path(), "a.pak", b"URSPRUENGLICH");
        let original_hash = hash_file(&path).unwrap();

        let mut lib = Library::default();
        lib.mods.insert("a.pak".into(), info_matching("a.pak", &metadata, &original_hash));

        std::fs::write(&path, b"ERSETZT MIT ANDEREM INHALT").unwrap();

        // First run: actually hashes and detects the mismatch.
        let first = lib.detect_altered(dir.path(), &["a.pak".into()]).unwrap();
        assert_eq!(first.altered, vec!["a.pak"]);
        assert!(first.cache_refreshed);
        assert!(lib.mods["a.pak"].known_altered);
        assert_eq!(lib.mods["a.pak"].hash, original_hash, "the baseline hash is kept for duplicate detection");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
            if std::fs::read(&path).is_ok() {
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
                eprintln!("skipped: this process can apparently bypass read permissions (root?)");
                return;
            }
        }

        let second = lib.detect_altered(dir.path(), &["a.pak".into()]).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        }

        assert_eq!(second.altered, vec!["a.pak"], "the warning has to keep appearing on every run");
        assert!(!second.cache_refreshed, "without a new finding there is nothing to refresh again");
        assert!(
            second.warnings.is_empty(),
            "an actual second hash attempt would have had to fail on the revoked read \
             permission: {:?}",
            second.warnings
        );
    }

    /// When the content returns to the originally imported state (the hash
    /// matches again), `known_altered` has to be cleared — otherwise the
    /// warning would wrongly keep running even though nothing differs any
    /// more.
    #[test]
    fn detect_altered_clears_known_altered_once_content_matches_the_original_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.pak");
        let metadata = write_pak(dir.path(), "a.pak", b"URSPRUENGLICH");
        let original_hash = hash_file(&path).unwrap();

        let mut lib = Library::default();
        lib.mods.insert("a.pak".into(), info_matching("a.pak", &metadata, &original_hash));

        std::fs::write(&path, b"ZWISCHENZEITLICH ANDERS").unwrap();
        let first = lib.detect_altered(dir.path(), &["a.pak".into()]).unwrap();
        assert_eq!(first.altered, vec!["a.pak"]);
        assert!(lib.mods["a.pak"].known_altered);

        // The original content is restored — and, importantly for the
        // prefilter, the original size along with it.
        std::fs::write(&path, b"URSPRUENGLICH").unwrap();
        let second = lib.detect_altered(dir.path(), &["a.pak".into()]).unwrap();

        assert!(second.altered.is_empty(), "the original content is back – no difference any more");
        assert!(!lib.mods["a.pak"].known_altered, "the marker has to be cleared");
    }

    /// A pak unknown to the library (never imported, e.g. copied in by
    /// hand) has nothing to compare against — that is the `added` case of
    /// `PakConfig::reconcile`, not this one.
    #[test]
    fn detect_altered_skips_a_pak_unknown_to_the_library() {
        let dir = tempfile::tempdir().unwrap();
        write_pak(dir.path(), "fremd.pak", b"X");

        let mut lib = Library::default();
        let report = lib.detect_altered(dir.path(), &["fremd.pak".into()]).unwrap();

        assert!(report.altered.is_empty());
    }

    /// A changed modification time without changed content (e.g. from a
    /// `touch`, or because a copy replaced the original with identical
    /// content but a new timestamp) is not "altered outside the loader" —
    /// the hash confirms the unchanged content. The prefilter cache is
    /// refreshed anyway, so that future runs do not hash again.
    #[test]
    fn detect_altered_refreshes_the_cache_on_a_false_positive_from_the_cheap_prefilter() {
        let dir = tempfile::tempdir().unwrap();
        let metadata = write_pak(dir.path(), "a.pak", b"UNVERAENDERT");
        let hash = hash_file(&dir.path().join("a.pak")).unwrap();

        let mut lib = Library::default();
        let mut stale = info_matching("a.pak", &metadata, &hash);
        // Deliberately stale modification time, of the kind a `touch` or
        // another copy with unchanged content could leave behind.
        stale.mtime = stale.mtime.map(|t| t.saturating_sub(3600));
        lib.mods.insert("a.pak".into(), stale);

        let report = lib.detect_altered(dir.path(), &["a.pak".into()]).unwrap();

        assert!(report.altered.is_empty(), "unchanged content must not count as altered");
        assert!(report.cache_refreshed, "a prefilter refresh has to be reported (2)");
        assert_eq!(
            lib.mods["a.pak"].mtime, metadata.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()),
            "the prefilter cache has to be refreshed after the confirmation"
        );
    }

    /// Simulates exactly the scenario from review item 2: a `library.json`
    /// that predates `ModInfo::mtime` deserializes that field as `None` (see
    /// `#[serde(default)]`), while the real file does have an actual
    /// `mtime`. Without `cache_refreshed` that would lead to a full hash on
    /// every single call — read-only commands such as `list`/`paths`
    /// included — without limit, because those commands never rewrite
    /// `library.json`.
    #[test]
    fn detect_altered_reports_cache_refresh_for_a_pre_mtime_library_entry() {
        let dir = tempfile::tempdir().unwrap();
        let metadata = write_pak(dir.path(), "a.pak", b"INHALT");
        let hash = hash_file(&dir.path().join("a.pak")).unwrap();

        let mut lib = Library::default();
        let mut legacy = info("a.pak", &hash);
        legacy.size = metadata.len();
        legacy.mtime = None; // like a legacy entry without this field
        lib.mods.insert("a.pak".into(), legacy);

        let first_run = lib.detect_altered(dir.path(), &["a.pak".into()]).unwrap();
        assert!(first_run.altered.is_empty());
        assert!(first_run.cache_refreshed, "a missing mtime has to count as a prefilter mismatch");

        // After the (simulated) rewrite of library.json the prefilter now
        // works again: no second hash attempt needed.
        let second_run = lib.detect_altered(dir.path(), &["a.pak".into()]).unwrap();
        assert!(second_run.altered.is_empty());
        assert!(!second_run.cache_refreshed, "the prefilter has to take effect on the second run already");
    }

    /// Item 3: a single unreadable pak must not make the whole verification
    /// fail (that would even hit `sm2 paths`, which never reads pak
    /// contents) — Spec §6.3 describes this case as a marker, not as a hard
    /// requirement.
    #[cfg(unix)]
    #[test]
    fn detect_altered_warns_instead_of_failing_on_an_unreadable_pak() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.pak");
        write_pak(dir.path(), "a.pak", b"INHALT");

        let mut lib = Library::default();
        // Size and hash deliberately disagree with the cheap prefilter, so
        // that the code path really reaches the (then failing) hash attempt
        // instead of skipping out before it.
        let mut mismatched = info("a.pak", "irrelevant");
        mismatched.size = 0;
        mismatched.mtime = None;
        lib.mods.insert("a.pak".into(), mismatched);

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read(&path).is_ok() {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            eprintln!("skipped: this process can apparently bypass read permissions (root?)");
            return;
        }

        let result = lib.detect_altered(dir.path(), &["a.pak".into()]);

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let report = result.expect("an unreadable pak must not make the verification fail");
        assert!(report.altered.is_empty(), "without readable content nothing can count as altered");
        assert_eq!(report.warnings.len(), 1, "the unreadability has to become visible as a warning");
        assert!(report.warnings[0].contains("a.pak"));
    }

    /// A file the caller says should be there but that can no longer be read
    /// (e.g. in a rare race between `list_paks` and this call) must not make
    /// `detect_altered` fail — that case belongs to
    /// `PakConfig::reconcile`'s `removed`.
    #[test]
    fn detect_altered_skips_a_pak_that_disappeared_since_being_listed() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::default();
        lib.mods.insert("weg.pak".into(), info("weg.pak", "irrelevant"));

        let report = lib.detect_altered(dir.path(), &["weg.pak".into()]).unwrap();

        assert!(report.altered.is_empty());
    }

    // --- last_known_position: Option instead of usize (review item 4) ----

    /// A `library.json` from before `last_known_position` omits the field
    /// entirely — it does not merely carry the value 0. If it deserialized
    /// to `Some(0)` instead of `None`, such a legacy entry would wrongly
    /// jump to the first position on the next `reconcile` after reappearing.
    #[test]
    fn legacy_json_without_last_known_position_yields_none_not_zero() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        std::fs::write(
            &path,
            r#"{"mods":{"a.pak":{"pak":"a.pak","name":"a","hash":"h","size":1,"imported_at":"2026-01-01T00:00:00Z"}}}"#,
        )
        .unwrap();

        let lib = Library::load(&path).unwrap();

        assert_eq!(
            lib.mods["a.pak"].last_known_position, None,
            "a missing history must not show up as position 0"
        );
        assert!(!lib.mods["a.pak"].last_known_disabled);
    }
}

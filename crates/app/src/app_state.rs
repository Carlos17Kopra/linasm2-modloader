//! Shared state for all CLI commands: paths, settings, library and pak
//! configuration in one structure, loaded once at startup and reconciled
//! with the directory contents.

use anyhow::{Context, Result};
use sm2_core::library::Library;
use sm2_core::pak_config::{KnownState, PakConfig};
use sm2_core::paths::{app_dirs, AppDirs, GamePaths};
use sm2_core::settings::Settings;
use sm2_core::Error;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Severity of a notice from the reconciliation — it determines color and
/// icon in the GUI, and the prefix on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    /// Something happened that the user should know about, but nothing is
    /// missing.
    Info,
    /// Something deviates from the expected state and may need a decision.
    Warning,
    /// Something expected is missing.
    Error,
}

/// A notice from the reconciliation between configuration and directory.
///
/// A struct rather than `eprintln!`: the GUI shows the same notices in a
/// notice bar above the mod list, and text already written to stderr could
/// no longer be colored, grouped or dismissed there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub kind: NoticeKind,
    pub text: String,
}

impl Notice {
    fn info(text: impl Into<String>) -> Self {
        Self { kind: NoticeKind::Info, text: text.into() }
    }

    fn warning(text: impl Into<String>) -> Self {
        Self { kind: NoticeKind::Warning, text: text.into() }
    }

    fn error(text: impl Into<String>) -> Self {
        Self { kind: NoticeKind::Error, text: text.into() }
    }
}

pub struct AppState {
    pub paths: GamePaths,
    pub settings: Settings,
    pub dirs: AppDirs,
    pub library: Library,
    pub config: PakConfig,
    /// Notices accumulated since they were last taken — see `Notice`.
    pub notices: Vec<Notice>,
}

impl AppState {
    /// Detects the game, loads all state and reconciles the configuration
    /// with the directory contents. Reports deviations on stderr.
    ///
    /// The reconciliation is deliberately NOT written to disk right away:
    /// `reconcile` is deterministic — every call reproduces the same result
    /// from the same directory contents — and writing immediately would make
    /// read-only commands like `list` or `paths` fail needlessly on a
    /// write-protected mods directory. The reconciled configuration only
    /// reaches the disk once a modifying command calls `persist()` anyway.
    pub fn open() -> Result<Self> {
        let (dirs, settings) = load_dirs_and_settings()?;
        Self::open_with(dirs, settings)
    }

    /// Like `open()`, but with base directories and settings already loaded.
    ///
    /// A separate entry point for the GUI: if game detection fails, it shows
    /// the first-run screen and must be able to write the directory the user
    /// picks into those same settings — which therefore have to survive the
    /// failed attempt.
    pub fn open_with(dirs: AppDirs, settings: Settings) -> Result<Self> {
        let paths = match &settings.game_dir {
            Some(dir) => {
                // For a manual path, derive the library from it.
                let library = dir
                    .ancestors()
                    .find(|a| a.join("steamapps/common").is_dir())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| dir.clone());
                GamePaths::from_game_dir(dir, &library)?
            }
            None => GamePaths::discover().context(
                "Space Marine 2 nicht gefunden – Pfad in settings.toml unter game_dir eintragen",
            )?,
        };

        let library_path = dirs.data.join("library.json");
        let mut library = Library::load(&library_path)?;
        let mut config = PakConfig::load(&paths.pak_config_path())?;
        let (cache_refreshed, mut notices) = reconcile_and_collect(&mut library, &mut config, &paths)?;
        if cache_refreshed {
            save_library_cache_best_effort(&library, &library_path, &mut notices);
        }

        Ok(Self { paths, settings, dirs, library, config, notices })
    }

    /// Takes all notices accumulated since the last call and empties the
    /// buffer — so the same notice never appears twice.
    pub fn take_notices(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.notices)
    }

    /// Is the mods directory writable? The GUI asks this once while loading
    /// so it can disable enabling, sorting and import, rather than letting
    /// the user fail only when saving.
    pub fn mods_dir_is_writable(&self) -> bool {
        check_write_permission(&self.paths.mods_dir()).is_ok()
    }

    /// Writes library and configuration to disk.
    ///
    /// The order is deliberate: `library.json` (our own records) first,
    /// `pak_config.yaml` (the file the game engine sees) last. If the second
    /// step fails, the engine has not seen the new state yet — only our own
    /// records, which have not taken effect, are ahead. In the reverse order
    /// the same failure would leave behind a change that is already in
    /// effect for the engine, together with stale records of our own.
    ///
    /// Checks up front whether the mods directory is writable at all — here
    /// and not in a plain `open()`, so that read-only commands do not fail
    /// on a write-protected directory.
    ///
    /// Also updates `ModInfo::last_known_disabled`/`last_known_position` in
    /// the library for every pak that is currently part of the
    /// configuration (see their doc comments): this is the only place these
    /// values are written, and the only source from which
    /// `PakConfig::reconcile` can put a pak that reappears later back in its
    /// old place. A pak that is currently missing is deliberately left
    /// untouched here — that is exactly why its last known state survives.
    ///
    /// A pak in `config.entries` without a `ModInfo` (copied into `mods/` by
    /// hand instead of imported via `import_pak` — see review point 1) is
    /// given a minimal `ModInfo` entry here instead of staying without any
    /// history forever: otherwise such a pak would never return to its
    /// previous position after disappearing (a Steam update, say), even
    /// though `reconcile`'s append rule is meant for exactly this
    /// population. The hashing cost only hits `persist()` (a deliberately
    /// writing call), not `AppState::open()` — so a read-only command like
    /// `list` does not hash a newly discovered, hand-copied pak (see review
    /// point 2, the same principle).
    ///
    /// If the hashing fails, the entry is skipped (not `persist()` as a
    /// whole) and reported as a German-language warning, consistent with how
    /// `detect_altered` handles read errors itself: if the file simply
    /// disappears again, the next `reconcile` reports it under `removed`
    /// anyway, but a permanent permission error on a file that is still
    /// there would otherwise stay silent and unnoticed forever, instead of
    /// telling the user why this pak never gets a history.
    pub fn persist(&mut self) -> Result<()> {
        check_write_permission(&self.paths.mods_dir())?;
        let mods_dir = self.paths.mods_dir();
        let mut warnings = Vec::new();
        for (position, entry) in self.config.entries.iter().enumerate() {
            match self.library.mods.get_mut(&entry.pak) {
                Some(mod_info) => {
                    mod_info.last_known_disabled = entry.disabled;
                    mod_info.last_known_position = Some(position);
                }
                None => match register_unknown_pak(&mods_dir, &entry.pak, entry.disabled, position) {
                    Ok(info) => {
                        self.library.mods.insert(entry.pak.clone(), info);
                    }
                    Err(e) => warnings.push(Notice::warning(format!(
                        "{} konnte nicht für die Positions-/Aktivierungshistorie \
                         registriert werden – {e}",
                        entry.pak
                    ))),
                },
            }
        }
        self.notices.append(&mut warnings);
        self.library.save(&self.dirs.data.join("library.json"))?;
        self.config.save(&self.paths.pak_config_path())?;
        Ok(())
    }

    /// Determines the save directory, honoring a value set in
    /// `settings.steam_user` (see `GamePaths::save_dir`'s doc comment for
    /// the exact resolution order). The single call site in the CLI, so that
    /// `steam_user` does not have to be wired up again at every place that
    /// needs the save directory.
    pub fn save_dir(&self) -> Result<PathBuf> {
        self.paths.save_dir(self.settings.steam_user.as_deref()).map_err(Into::into)
    }

    pub fn profiles_dir(&self) -> PathBuf {
        self.dirs.data.join("profiles")
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.dirs.data.join("backups/saves")
    }
}

/// Reconciles `config` with the actual directory contents (spec §6.3) and
/// reports every deviation on stderr. A separate function instead of inline
/// in `open()`, so the logic can be exercised in tests without a real game
/// installation (which `open()` requires via `discover()`/`settings.toml`).
///
/// Builds the known state from `library` for `PakConfig::reconcile` (see
/// `KnownState`'s doc comment): `pak_config.rs` deliberately does not know
/// the library itself, so the module layering is not inverted — the app
/// layer builds this map explicitly and passes it in as a parameter. A
/// `ModInfo` without `last_known_position` (never part of the configuration,
/// or a `library.json` from before that field existed) is excluded rather
/// than included with a guessed position — see review point 4 and
/// `ModInfo::last_known_position`'s doc comment.
///
/// Then checks the third reconciliation case from spec §6.3 ("altered
/// outside") via `Library::detect_altered` and returns whether its cheap
/// pre-filter cache was refreshed — the caller (`open()`) then rewrites
/// `library.json` immediately (see review point 2), so that a stale or
/// missing `mtime` does not lead to a full hash again on every further
/// call, including read-only ones.
fn reconcile_and_collect(
    library: &mut Library,
    config: &mut PakConfig,
    paths: &GamePaths,
) -> Result<(bool, Vec<Notice>)> {
    let mut notices = Vec::new();
    let present = paths.list_paks()?;

    let known_state: HashMap<String, KnownState> = library
        .mods
        .values()
        .filter_map(|m| {
            m.last_known_position
                .map(|position| (m.pak.clone(), KnownState { disabled: m.last_known_disabled, position }))
        })
        .collect();

    let reconciliation = config.reconcile(&present, &known_state);
    let restored: std::collections::HashSet<&str> =
        reconciliation.restored.iter().map(String::as_str).collect();
    for pak in &reconciliation.added {
        if restored.contains(pak.as_str()) {
            notices.push(Notice::info(format!(
                "{pak} war zwischenzeitlich nicht vorhanden und wurde mit vorherigem \
                 Aktivierungszustand und vorheriger Position wiederhergestellt."
            )));
        } else {
            notices.push(Notice::warning(format!(
                "{pak} stand nicht in pak_config.yaml und wurde aktiv übernommen – eine Datei \
                 im Mods-Verzeichnis lädt ohnehin, ungesteuert und zuerst."
            )));
        }
    }
    for pak in &reconciliation.removed {
        notices.push(Notice::error(format!(
            "{pak} steht in pak_config.yaml, die Datei fehlt aber – Eintrag entfernt."
        )));
    }

    // Only size and modification time are checked here by default (see
    // `Library::detect_altered`'s doc comment) — a full hash only runs when
    // that cheap pre-filter indicates a difference.
    let report = library.detect_altered(&paths.mods_dir(), &present)?;
    for pak in &report.altered {
        notices.push(Notice::warning(format!(
            "{pak} wurde außerhalb des Loaders verändert (Hash weicht ab)."
        )));
    }
    for warning in &report.warnings {
        notices.push(Notice::warning(warning.clone()));
    }

    Ok((report.cache_refreshed, notices))
}

/// Writes `library.json` after a pure cache refresh (see
/// `Library::detect_altered`'s `cache_refreshed`) — on a best-effort basis
/// only: if the write fails (because the application data directory exists
/// but is not writable, say), it only warns, `open()` itself does NOT fail.
/// Loading the library and the configuration has already succeeded by this
/// point; a hard `?` here would reintroduce exactly the class of failure
/// ("every command, even `paths`/`list`, fails") that introducing
/// `cache_refreshed` had just removed — this write is pure cache
/// maintenance, not a result the user intended with their call. A separate
/// function so this behavior (warn instead of fail) is testable without a
/// real game installation.
fn save_library_cache_best_effort(library: &Library, library_path: &Path, notices: &mut Vec<Notice>) {
    if let Err(e) = library.save(library_path) {
        notices.push(Notice::warning(format!(
            "Cache-Auffrischung in library.json konnte nicht gespeichert werden – {e}"
        )));
    }
}

/// Builds a minimal entry for a pak that is listed in `config.entries` but
/// has no `ModInfo` entry yet (because it was dropped into `mods/` by hand
/// instead of imported via `import_pak`) — see `persist()`'s doc comment
/// (review point 1).
fn register_unknown_pak(
    mods_dir: &Path,
    pak: &str,
    disabled: bool,
    position: usize,
) -> sm2_core::Result<sm2_core::library::ModInfo> {
    use sm2_core::import::{now_rfc3339, strip_pak_suffix};
    use sm2_core::library::{hash_file, ModInfo};

    let path = mods_dir.join(pak);
    let metadata = std::fs::metadata(&path).map_err(|e| sm2_core::Error::io(&path, e))?;
    let hash = hash_file(&path)?;
    let mtime = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());

    // The same derivation as `import_pak`'s `display_name` (a
    // case-insensitive, non-repeated `.pak` suffix via `strip_pak_suffix`,
    // then separators replaced by spaces) — not a separate
    // `trim_end_matches(".pak")`, which would not match `MOD.PAK` at all and
    // would strip `a.pak.pak` more than once. Otherwise two identically
    // placed paks could end up with different display names depending on
    // whether they were imported or copied by hand.
    let name = strip_pak_suffix(pak).replace(['_', '-'], " ");

    Ok(ModInfo {
        pak: pak.to_string(),
        name,
        author: None,
        version: None,
        nexus_id: None,
        notes: None,
        hash,
        size: metadata.len(),
        imported_at: now_rfc3339(),
        source: None,
        last_known_disabled: disabled,
        last_known_position: Some(position),
        mtime,
        known_altered: false,
    })
}

/// Determines whether we can write in `dir` — before a modifying command
/// makes changes that would then fail only when saving.
/// Loads base directories and settings — the part of `open()` that succeeds
/// even without a detected game directory. Split out so the GUI still knows
/// where to write a directory chosen by the user after game detection has
/// failed.
pub fn load_dirs_and_settings() -> Result<(AppDirs, Settings)> {
    let dirs = app_dirs().context("Basisverzeichnisse nicht ermittelbar")?;
    std::fs::create_dir_all(&dirs.config)
        .with_context(|| format!("{} konnte nicht angelegt werden", dirs.config.display()))?;
    std::fs::create_dir_all(&dirs.data)
        .with_context(|| format!("{} konnte nicht angelegt werden", dirs.data.display()))?;
    let settings = Settings::load(&dirs.config.join("settings.toml"))?;
    Ok((dirs, settings))
}

fn check_write_permission(dir: &Path) -> Result<()> {
    let probe = dir.join(".sm2-modloader-writetest");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(_) => Err(Error::NotWritable(dir.to_path_buf()).into()),
    }
}

/// Builds an `AppState` directly from the public fields, without `open()`
/// (which requires a real game installation via `discover()` or
/// `settings.toml`). A minimal, valid game directory is enough for tests.
/// `pub(crate)` so the tests in `cli.rs` can use this fixture too instead of
/// duplicating it.
#[cfg(test)]
pub(crate) fn test_fixture(base: &Path) -> AppState {
    let game = base.join("game");
    std::fs::create_dir_all(game.join("client_pc/root/mods")).unwrap();
    let paths = GamePaths::from_game_dir(&game, base).unwrap();
    AppState {
        paths,
        settings: Settings::default(),
        dirs: AppDirs {
            config: base.join("config"),
            data: base.join("data"),
            state: base.join("state"),
        },
        library: Library::default(),
        config: PakConfig::default(),
        notices: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sm2_core::library::ModInfo;
    use sm2_core::pak_config::PakEntry;

    fn minimal_mod_info(pak: &str) -> ModInfo {
        ModInfo {
            pak: pak.to_string(),
            name: pak.to_string(),
            author: None,
            version: None,
            nexus_id: None,
            notes: None,
            hash: "irrelevant".to_string(),
            size: 1,
            imported_at: "2026-09-12T18:00:00Z".to_string(),
            source: None,
            last_known_disabled: false,
            last_known_position: Some(0),
            mtime: None,
            known_altered: false,
        }
    }

    /// The core case from the review (1a): a pak disappears (through a Steam
    /// update, say, spec §9 R3), `persist()` writes the now shortened
    /// configuration, the pak reappears — and must then come back with its
    /// previous enabled state AND its previous position, not enabled and
    /// alphabetically at the end. Checks the actual wiring (persist() ->
    /// library.json -> reconcile), not just `PakConfig::reconcile` in
    /// isolation (`pak_config.rs`'s own test already covers that).
    #[test]
    fn a_pak_that_disappears_and_reappears_keeps_its_previous_state_across_persist() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        let mods_dir = state.paths.mods_dir();

        for name in ["a.pak", "b.pak", "c.pak"] {
            std::fs::write(mods_dir.join(name), b"INHALT").unwrap();
            state.library.mods.insert(name.to_string(), minimal_mod_info(name));
        }
        state.config.entries = vec![
            PakEntry { pak: "a.pak".into(), disabled: false },
            PakEntry { pak: "b.pak".into(), disabled: true },
            PakEntry { pak: "c.pak".into(), disabled: false },
        ];

        // Writes last_known_disabled/last_known_position for all three.
        state.persist().unwrap();

        // b.pak disappears (a Steam update, say) and is reconciled.
        std::fs::remove_file(mods_dir.join("b.pak")).unwrap();
        reconcile_and_collect(&mut state.library, &mut state.config, &state.paths).unwrap();
        assert_eq!(
            state.config.entries.iter().map(|e| e.pak.as_str()).collect::<Vec<_>>(),
            vec!["a.pak", "c.pak"],
            "b.pak must be removed by the reconcile"
        );
        // persist() writes the shortened configuration; b.pak stays
        // untouched in the library (it is not part of config.entries).
        state.persist().unwrap();

        // b.pak reappears.
        std::fs::write(mods_dir.join("b.pak"), b"INHALT").unwrap();
        reconcile_and_collect(&mut state.library, &mut state.config, &state.paths).unwrap();

        let names: Vec<&str> = state.config.entries.iter().map(|e| e.pak.as_str()).collect();
        assert_eq!(names, vec!["a.pak", "b.pak", "c.pak"], "b.pak must return to its previous position");
        assert!(state.config.entries[1].disabled, "b.pak was disabled and must be disabled again");
    }

    /// Review point 1: the same guarantee as above, but for a pak that was
    /// never imported via `import_pak` — copied into `mods/` by hand,
    /// without any `ModInfo` entry at all. This is exactly the population
    /// that used to be excluded from any history (`KnownState` came solely
    /// from `library.mods`, which only `import_pak` ever writes to), and
    /// after disappearing and reappearing it always came back enabled and in
    /// alphabetical order instead of at its previous position.
    #[test]
    fn a_hand_copied_pak_without_any_prior_modinfo_also_gets_its_history_back() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        let mods_dir = state.paths.mods_dir();

        // Copied in by hand: the file is in the mods directory, but (unlike
        // with an import) there is no library.mods entry for it.
        std::fs::write(mods_dir.join("hand.pak"), b"VON HAND KOPIERT").unwrap();
        assert!(state.library.mods.is_empty(), "starting point: unknown to the library");

        reconcile_and_collect(&mut state.library, &mut state.config, &state.paths).unwrap();
        assert_eq!(
            state.config.entries.iter().map(|e| e.pak.as_str()).collect::<Vec<_>>(),
            vec!["hand.pak"],
            "an unknown pak is first appended as enabled, as usual"
        );

        // The user disables it explicitly (`sm2 disable hand.pak`, say) and
        // a modifying command writes the configuration.
        state.config.entries[0].disabled = true;
        state.persist().unwrap();
        assert!(
            state.library.mods.contains_key("hand.pak"),
            "persist() must now give the previously unknown pak a ModInfo entry"
        );

        // A Steam update wipes the mods folder.
        std::fs::remove_file(mods_dir.join("hand.pak")).unwrap();
        reconcile_and_collect(&mut state.library, &mut state.config, &state.paths).unwrap();
        assert!(state.config.entries.is_empty());
        state.persist().unwrap();

        // The user installs the exact same file by hand again.
        std::fs::write(mods_dir.join("hand.pak"), b"VON HAND KOPIERT").unwrap();
        reconcile_and_collect(&mut state.library, &mut state.config, &state.paths).unwrap();

        assert_eq!(state.config.entries.len(), 1);
        assert_eq!(state.config.entries[0].pak, "hand.pak");
        assert!(
            state.config.entries[0].disabled,
            "the previous disabled state must come back, not enabled-alphabetical"
        );
    }

    /// Review point 4 (third round): the display name of a hand-copied pak
    /// must follow exactly the same derivation as `import_pak`'s
    /// `display_name` (`strip_pak_suffix` + separators replaced by spaces) —
    /// not `trim_end_matches(".pak")`, which would not match `MOD.PAK` at
    /// all and would strip `a.pak.pak` more than once.
    #[test]
    fn register_unknown_pak_derives_the_display_name_like_import_pak_does() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        let mods_dir = state.paths.mods_dir();

        for name in ["mein_mod.pak", "MOD.PAK", "a.pak.pak"] {
            std::fs::write(mods_dir.join(name), b"x").unwrap();
        }
        reconcile_and_collect(&mut state.library, &mut state.config, &state.paths).unwrap();
        state.persist().unwrap();

        assert_eq!(state.library.mods["mein_mod.pak"].name, "mein mod");
        assert_eq!(
            state.library.mods["MOD.PAK"].name, "MOD",
            "the .pak suffix must be recognised regardless of letter case"
        );
        assert_eq!(
            state.library.mods["a.pak.pak"].name, "a.pak",
            "only the last .pak suffix may be stripped, not repeatedly"
        );
    }

    /// Review point 3: if `register_unknown_pak` fails (here: the file
    /// disappears again between `reconcile` and `persist`, but equally on
    /// missing read permissions with the file still present), that must not
    /// happen silently — the user has to learn why this pak never gets a
    /// history.
    #[test]
    fn persist_survives_a_pak_that_cannot_be_registered_without_inventing_history() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        // In the configuration, but without a ModInfo AND without a file in
        // the mods directory — reproduces register_unknown_pak's error path
        // (the stat fails) without depending on file permissions.
        state.config.entries = vec![PakEntry { pak: "weg.pak".into(), disabled: false }];

        // No panic, `persist()` itself still succeeds (the absence of this
        // one entry is not a hard requirement).
        state.persist().unwrap();

        assert!(
            !state.library.mods.contains_key("weg.pak"),
            "without a readable file no ModInfo can be created"
        );
    }

    /// Review point 1: if writing `library.json` after a pure cache refresh
    /// fails (the application data directory exists but is not writable,
    /// say), that must not fail the whole call as it used to (`?`) — up to
    /// that point both loading the library and loading the configuration had
    /// already succeeded.
    #[cfg(unix)]
    #[test]
    fn save_library_cache_best_effort_warns_instead_of_failing_on_a_read_only_data_dir() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let library_path = data_dir.join("library.json");

        let mut perms = std::fs::metadata(&data_dir).unwrap().permissions();
        perms.set_mode(0o555);
        std::fs::set_permissions(&data_dir, perms.clone()).unwrap();

        let probe = data_dir.join(".probe");
        let bypassed = std::fs::write(&probe, b"").is_ok();
        let _ = std::fs::remove_file(&probe);

        if bypassed {
            perms.set_mode(0o755);
            std::fs::set_permissions(&data_dir, perms).unwrap();
            eprintln!("skipped: this process can apparently bypass write protection (root?)");
            return;
        }

        // Must not crash — the function has no return value through which a
        // caller could turn the failure into an abort (see its doc comment);
        // that is deliberately part of the contract here. The failure does
        // not vanish, though: it lands in the buffer as a notice.
        let mut notices = Vec::new();
        save_library_cache_best_effort(&Library::default(), &library_path, &mut notices);

        perms.set_mode(0o755);
        std::fs::set_permissions(&data_dir, perms).unwrap();

        assert!(!library_path.exists(), "the write must actually have failed");
        assert_eq!(notices.len(), 1, "the failure must appear as exactly one notice");
        assert_eq!(notices[0].kind, NoticeKind::Warning, "cache maintenance is not an error");
        assert!(
            notices[0].text.contains("library.json"),
            "the notice must name the affected file: {}",
            notices[0].text
        );
    }

    /// Review point 4: a `library.json` from before `last_known_position`
    /// deserializes that field as `None` (see `ModInfo`'s
    /// `#[serde(default)]`). Several such legacy entries must not all
    /// collide at position 0 when they reappear (ending up in reverse order
    /// relative to each other) — they have to be treated like paks never
    /// seen before: alphabetically at the end.
    #[test]
    fn legacy_entries_without_a_known_position_do_not_collide_at_the_front() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        let mods_dir = state.paths.mods_dir();

        for name in ["b.pak", "a.pak"] {
            std::fs::write(mods_dir.join(name), b"x").unwrap();
            let mut info = minimal_mod_info(name);
            info.last_known_position = None; // like a legacy entry without it
            state.library.mods.insert(name.to_string(), info);
        }

        reconcile_and_collect(&mut state.library, &mut state.config, &state.paths).unwrap();

        let names: Vec<&str> = state.config.entries.iter().map(|e| e.pak.as_str()).collect();
        assert_eq!(
            names,
            vec!["a.pak", "b.pak"],
            "without a known position it must be appended alphabetically, not collide at position 0"
        );
    }

    /// Review point 2: a `library.json` from before `ModInfo::mtime` yields
    /// `None` while the real file has an actual `mtime` — so the cheap
    /// pre-filter misses on the first run and hashes once. Without
    /// `reconcile_and_report`'s caller rewriting `library.json` right away,
    /// that would happen again on every further call, including read-only
    /// ones.
    #[test]
    fn reconcile_and_report_reports_cache_refresh_so_open_can_persist_it_once() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        let mods_dir = state.paths.mods_dir();
        std::fs::write(mods_dir.join("a.pak"), b"INHALT").unwrap();
        let hash = sm2_core::library::hash_file(&mods_dir.join("a.pak")).unwrap();

        let mut legacy = minimal_mod_info("a.pak");
        legacy.hash = hash;
        legacy.size = std::fs::metadata(mods_dir.join("a.pak")).unwrap().len();
        legacy.mtime = None; // like a legacy entry without this field
        state.library.mods.insert("a.pak".to_string(), legacy);
        state.config.entries = vec![PakEntry { pak: "a.pak".into(), disabled: false }];

        let (first_run, _) =
            reconcile_and_collect(&mut state.library, &mut state.config, &state.paths).unwrap();
        assert!(first_run, "a missing mtime must be reported as a refresh on the first run");

        let (second_run, _) =
            reconcile_and_collect(&mut state.library, &mut state.config, &state.paths).unwrap();
        assert!(!second_run, "the refreshed cache must already take effect on the second run");
    }

    /// Creates two Proton save user directories under `base` (the same root
    /// that `test_fixture` uses as `library_dir`), the way they appear when
    /// several Steam profiles share the same prefix.
    fn write_two_save_users(base: &Path) -> (String, String) {
        let user_root = base
            .join("steamapps/compatdata/2183900/pfx/drive_c/users/steamuser")
            .join("AppData/Local/Saber/Space Marine 2/storage/steam/user");
        let a = "76561198000000001".to_string();
        let b = "76561198000000002".to_string();
        std::fs::create_dir_all(user_root.join(&a).join("Main")).unwrap();
        std::fs::create_dir_all(user_root.join(&b).join("Main")).unwrap();
        (a, b)
    }

    /// 2b: `settings.steam_user` is no longer a decorative setting — it is
    /// actually read and resolves the otherwise fatal ambiguity of several
    /// save user profiles.
    #[test]
    fn save_dir_uses_the_configured_steam_user_to_resolve_ambiguity() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        let (a, _b) = write_two_save_users(tmp.path());
        state.settings.steam_user = Some(a.clone());

        let saves = state.save_dir().unwrap();

        assert!(saves.ends_with(format!("{a}/Main")));
    }

    /// A `steam_user` that matches none of the profiles found (a typo in
    /// `settings.toml`, say) must produce a clear error that names the
    /// profiles that do exist.
    #[test]
    fn save_dir_reports_a_clear_error_when_the_configured_steam_user_matches_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_fixture(tmp.path());
        write_two_save_users(tmp.path());
        state.settings.steam_user = Some("00000000000000000".to_string());

        let err = state.save_dir().unwrap_err();

        assert!(
            err.to_string().contains("00000000000000000"),
            "the error must name the (not found) setting: {err}"
        );
    }

    #[test]
    fn profiles_dir_and_backups_dir_are_under_the_data_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_fixture(tmp.path());

        assert_eq!(state.profiles_dir(), state.dirs.data.join("profiles"));
        assert_eq!(state.backups_dir(), state.dirs.data.join("backups/saves"));
    }

    #[test]
    fn check_write_permission_succeeds_for_a_writable_dir_and_leaves_no_probe_file() {
        let tmp = tempfile::tempdir().unwrap();
        check_write_permission(tmp.path()).unwrap();
        assert!(!tmp.path().join(".sm2-modloader-writetest").exists());
    }

    #[test]
    fn check_write_permission_fails_for_a_missing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("gibt_es_nicht");
        assert!(check_write_permission(&missing).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn check_write_permission_fails_for_a_read_only_dir() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("readonly");
        std::fs::create_dir_all(&dir).unwrap();
        let mut perms = std::fs::metadata(&dir).unwrap().permissions();
        perms.set_mode(0o555);
        std::fs::set_permissions(&dir, perms.clone()).unwrap();

        let result = check_write_permission(&dir);

        // Clean up so tempfile can delete the directory again.
        perms.set_mode(0o755);
        std::fs::set_permissions(&dir, perms).unwrap();

        if result.is_ok() {
            // If the test runs as root, the kernel bypasses the write
            // protection mode entirely — not a failure of this test.
            eprintln!("skipped: this process can apparently bypass write protection (root?)");
            return;
        }
        assert!(result.is_err());
    }
}

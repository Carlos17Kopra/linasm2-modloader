use crate::error::{Error, Result};
use crate::platform::{Current, Platform};
use crate::{APP_ID, APP_SLUG, LEGACY_APP_SLUG};
use std::path::{Path, PathBuf};

/// All paths around a game installation.
#[derive(Debug, Clone)]
pub struct GamePaths {
    pub game_dir: PathBuf,
    /// The Steam library the game lives in. The Proton prefix hangs off
    /// that, not off the game directory.
    pub library_dir: PathBuf,
}

impl GamePaths {
    /// Verifies that `game_dir` really is a Space Marine 2 installation.
    pub fn from_game_dir(game_dir: &Path, library_dir: &Path) -> Result<Self> {
        if !game_dir.join("client_pc/root/mods").is_dir() {
            return Err(Error::NotAGameDir(game_dir.to_path_buf()));
        }
        Ok(Self {
            game_dir: game_dir.to_path_buf(),
            library_dir: library_dir.to_path_buf(),
        })
    }

    /// Finds the game through the Steam libraries.
    ///
    /// `steamlocate` covers the normal case (default and custom libraries
    /// alike via `libraryfolders.vdf`, Flatpak Steam included). If it finds
    /// no Steam installation at all, `Platform::steam_roots` steps in as a
    /// fallback (see `find_in_roots`) — declared so far but never actually
    /// taken, since `steamlocate` itself already covers the same set of
    /// roots internally. The fallback only checks the default library
    /// directly under each root, not that root's own `libraryfolders.vdf`.
    /// `steamlocate` already covers that when it succeeds, and if even that
    /// more robust approach fails, an unusual custom library is only
    /// reachable by hand through `settings.toml`'s `game_dir` anyway.
    pub fn discover() -> Result<Self> {
        match steamlocate::SteamDir::locate() {
            Ok(steam) => {
                let (app, library) = steam
                    .find_app(APP_ID)
                    .map_err(|_| Error::GameNotFound(APP_ID))?
                    .ok_or(Error::GameNotFound(APP_ID))?;

                let game_dir = library
                    .path()
                    .join("steamapps/common")
                    .join(&app.install_dir);

                Self::from_game_dir(&game_dir, library.path())
            }
            Err(_) => Self::find_in_roots(&Current::steam_roots()),
        }
    }

    /// Checks each of the given Steam roots directly for a Space Marine 2
    /// installation under `steamapps/common/Space Marine 2`. A function of
    /// its own (rather than inline in `discover`) so it can be tested with
    /// synthetic roots from a `tempfile` fixture, without the detour through
    /// `$HOME` that `Platform::steam_roots` depends on.
    fn find_in_roots(roots: &[PathBuf]) -> Result<Self> {
        for root in roots {
            let game_dir = root.join("steamapps/common/Space Marine 2");
            if let Ok(paths) = Self::from_game_dir(&game_dir, root) {
                return Ok(paths);
            }
        }
        Err(Error::SteamNotFound)
    }

    pub fn mods_dir(&self) -> PathBuf {
        self.game_dir.join("client_pc/root/mods")
    }

    pub fn pak_config_path(&self) -> PathBuf {
        self.mods_dir().join("pak_config.yaml")
    }

    pub fn executable(&self) -> PathBuf {
        self.game_dir
            .join("client_pc/root/bin/pc")
            .join("Warhammer 40000 Space Marine 2 - Retail.exe")
    }

    /// The savegame directory inside the Proton prefix.
    ///
    /// Below `AppData/Local` the path is identical to Windows — only the
    /// root comes from the platform.
    ///
    /// `steam_user`, when given (`settings.toml`'s `steam_user`), overrides
    /// the automatic choice: with exactly one user directory present the
    /// automatic choice would pick it anyway; with several present
    /// `Error::AmbiguousSaveUser` would otherwise decide the matter — a
    /// preset instead selects exactly that directory, provided it is among
    /// the ones found. A `steam_user` that matches none of the directories
    /// found yields an error of its own naming the profiles actually
    /// present. That way a typo in `settings.toml` is obvious at once
    /// instead of disappearing into a silent
    /// `Error::NoSaveUser`/`AmbiguousSaveUser`.
    pub fn save_dir(&self, steam_user: Option<&str>) -> Result<PathBuf> {
        let prefix_root = Current::user_profile_root(APP_ID, &self.library_dir);
        if !prefix_root.is_dir() {
            return Err(Error::PrefixMissing(APP_ID));
        }

        let user_root =
            prefix_root.join("AppData/Local/Saber/Space Marine 2/storage/steam/user");

        // A failed read_dir here does not necessarily mean the folder never
        // existed: on a fresh installation Proton creates the prefix on the
        // first launch, but the Saber folder only appears once the game
        // actually writes a save. Both cases look the same to the user —
        // there is no profile (yet).
        let mut user_ids: Vec<String> = std::fs::read_dir(&user_root)
            .map_err(|_| Error::NoSaveUser(user_root.clone()))?
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        user_ids.sort();

        if let Some(requested) = steam_user {
            return if user_ids.iter().any(|id| id == requested) {
                Ok(user_root.join(requested).join("Main"))
            } else {
                Err(Error::UnknownSaveUser { requested: requested.to_string(), available: user_ids })
            };
        }

        match user_ids.len() {
            0 => Err(Error::NoSaveUser(user_root)),
            1 => Ok(user_root.join(&user_ids[0]).join("Main")),
            _ => Err(Error::AmbiguousSaveUser(user_ids)),
        }
    }

    /// All `.pak` files in the mods directory, alphabetically.
    ///
    /// `Path::is_file` follows symlinks, so a symlink to a `.pak` file is
    /// correctly counted; a dead symlink, or one pointing at a directory, is
    /// correctly excluded.
    pub fn list_paks(&self) -> Result<Vec<String>> {
        let dir = self.mods_dir();
        let mut paks: Vec<String> = std::fs::read_dir(&dir)
            .map_err(|e| Error::io(&dir, e))?
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().is_file())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.to_lowercase().ends_with(".pak"))
            .collect();
        paks.sort();
        Ok(paks)
    }
}

/// The application's XDG directories.
#[derive(Debug, Clone)]
pub struct AppDirs {
    pub config: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
}

pub fn app_dirs() -> Result<AppDirs> {
    let project_dirs = directories::ProjectDirs::from("", "", APP_SLUG).ok_or_else(|| {
        Error::PlainIo(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            crate::t!("error.app_dirs_not_found"),
        ))
    })?;

    let dirs = AppDirs {
        config: project_dirs.config_dir().to_path_buf(),
        data: project_dirs.data_dir().to_path_buf(),
        // On Linux with XDG_STATE_HOME, state_dir() practically always
        // returns Some(..); the fallback only applies on platforms without a
        // state directory of their own, and then deliberately uses the same
        // place as the rest of the persistent data.
        state: project_dirs
            .state_dir()
            .unwrap_or_else(|| project_dirs.data_dir())
            .to_path_buf(),
    };

    // Up to and including 0.1.0 the program was called `sm2-modloader` and
    // kept all three directories under that name. The move belongs here and
    // nowhere later: `load_dirs_and_settings` creates the config and data
    // directories before it reads anything, and an empty directory created
    // under the new name is exactly the thing that stops the move below —
    // the stored language, a hand-entered game directory and every profile
    // would stay behind, invisible, under the old name.
    //
    // The three moves are independent of one another on purpose. A crash
    // between two of them leaves each directory either fully moved or
    // untouched, which is a state the program reads without trouble; the
    // next start finishes what is left.
    if let Some(legacy) = directories::ProjectDirs::from("", "", LEGACY_APP_SLUG) {
        migrate_legacy_dir(legacy.config_dir(), &dirs.config);
        migrate_legacy_dir(legacy.data_dir(), &dirs.data);
        let legacy_state = legacy.state_dir().unwrap_or_else(|| legacy.data_dir());
        migrate_legacy_dir(legacy_state, &dirs.state);
    }

    Ok(dirs)
}

/// Moves a directory left behind under the previous name over to the one
/// in use today. Returns whether anything was moved.
///
/// Two conditions, and the order of the checks is the whole argument:
/// `current` must not exist yet, and `legacy` must be a directory. Once
/// anything exists under the new name it is the truth — a directory still
/// lying around under the old one is then a leftover, not a source, and
/// overwriting today's settings with it would lose data rather than save
/// it.
///
/// `rename` is the only operation used. Both paths sit under the same XDG
/// base directory, so they are on the same filesystem, where a rename is
/// atomic: a crash in the middle leaves either the old directory or the
/// new one, never a half-copied mixture of the two. A recursive copy could
/// not promise that.
///
/// A rename that fails is not an error the caller has to deal with. The
/// worst case is a program starting with its default settings — what a
/// fresh installation does anyway — and the old directory still sitting
/// there untouched for the user to move by hand. Refusing to start over a
/// leftover directory would be the far worse outcome.
///
/// A `legacy` that is a symlink to somewhere else is renamed as the
/// symlink it is, not followed and copied: a configuration directory
/// managed by a dotfiles tool keeps pointing where it pointed before.
fn migrate_legacy_dir(legacy: &Path, current: &Path) -> bool {
    if current.exists() || !legacy.is_dir() {
        return false;
    }
    if let Some(parent) = current.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return false;
        }
    }

    match std::fs::rename(legacy, current) {
        Ok(()) => {
            tracing::info!(
                from = %legacy.display(),
                to = %current.display(),
                "moved a directory of the previous application name"
            );
            true
        }
        Err(e) => {
            tracing::warn!(
                from = %legacy.display(),
                to = %current.display(),
                error = %e,
                "could not move a directory of the previous application name"
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a directory tree matching a real installation.
    fn fixture() -> (tempfile::TempDir, GamePaths) {
        let tmp = tempfile::tempdir().unwrap();
        let library = tmp.path().join("SteamLibrary");
        let game = library.join("steamapps/common/Space Marine 2");

        std::fs::create_dir_all(game.join("client_pc/root/mods")).unwrap();
        std::fs::create_dir_all(game.join("client_pc/root/bin/pc")).unwrap();

        let saves = library
            .join("steamapps/compatdata/2183900/pfx/drive_c/users/steamuser")
            .join("AppData/Local/Saber/Space Marine 2/storage/steam/user/76561198412726373/Main");
        std::fs::create_dir_all(&saves).unwrap();

        let paths = GamePaths::from_game_dir(&game, &library).unwrap();
        (tmp, paths)
    }

    #[test]
    fn rejects_directory_without_mods_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let err = GamePaths::from_game_dir(tmp.path(), tmp.path()).unwrap_err();
        assert!(matches!(err, Error::NotAGameDir(_)));
    }

    // --- find_in_roots (fallback for discover() when steamlocate fails) ---

    #[test]
    fn find_in_roots_locates_the_game_in_a_later_root() {
        let tmp = tempfile::tempdir().unwrap();
        let empty_root = tmp.path().join("KeinSteamHier");
        let real_root = tmp.path().join("SteamRoot");
        let game = real_root.join("steamapps/common/Space Marine 2");
        std::fs::create_dir_all(game.join("client_pc/root/mods")).unwrap();

        let paths = GamePaths::find_in_roots(&[empty_root, real_root.clone()]).unwrap();

        assert_eq!(paths.game_dir, game);
        assert_eq!(paths.library_dir, real_root, "the library is the root itself");
    }

    #[test]
    fn find_in_roots_fails_when_no_root_has_the_game() {
        let tmp = tempfile::tempdir().unwrap();
        let err = GamePaths::find_in_roots(&[tmp.path().join("a"), tmp.path().join("b")]).unwrap_err();
        assert!(matches!(err, Error::SteamNotFound));
    }

    #[test]
    fn finds_mods_dir_and_config_path() {
        let (_tmp, paths) = fixture();
        assert!(paths.mods_dir().ends_with("client_pc/root/mods"));
        assert!(paths.pak_config_path().ends_with("client_pc/root/mods/pak_config.yaml"));
    }

    // Unix only: the fixture builds the savegames inside a Proton prefix
    // under a temporary directory, which is where `user_profile_root`
    // looks on Linux. On Windows it returns the real `%USERPROFILE%`,
    // which a test must not write into. See CLAUDE.md → Platforms: this is
    // the gap that leaves Windows save handling without coverage.
    #[cfg(unix)]
    #[test]
    fn resolves_save_dir_in_proton_prefix() {
        let (_tmp, paths) = fixture();
        let saves = paths.save_dir(None).unwrap();
        assert!(saves.ends_with("storage/steam/user/76561198412726373/Main"));
        assert!(saves.is_dir());
    }

    // Unix only: the fixture builds the savegames inside a Proton prefix
    // under a temporary directory, which is where `user_profile_root`
    // looks on Linux. On Windows it returns the real `%USERPROFILE%`,
    // which a test must not write into. See CLAUDE.md → Platforms: this is
    // the gap that leaves Windows save handling without coverage.
    #[cfg(unix)]
    #[test]
    fn reports_missing_prefix_clearly() {
        let tmp = tempfile::tempdir().unwrap();
        let library = tmp.path().join("SteamLibrary");
        let game = library.join("steamapps/common/Space Marine 2");
        std::fs::create_dir_all(game.join("client_pc/root/mods")).unwrap();

        let paths = GamePaths::from_game_dir(&game, &library).unwrap();
        assert!(matches!(paths.save_dir(None).unwrap_err(), Error::PrefixMissing(2183900)));
    }

    /// The prefix exists (Proton created it on the first launch), but the
    /// Saber folder is still missing because the game was never taken past
    /// the main menu and never saved. This must not look like an I/O error;
    /// it has to be reported as "no user profile found".
    #[test]
    fn reports_no_save_user_for_fresh_install_without_saber_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let library = tmp.path().join("SteamLibrary");
        let game = library.join("steamapps/common/Space Marine 2");
        std::fs::create_dir_all(game.join("client_pc/root/mods")).unwrap();
        // The prefix exists, but without AppData/Local/Saber/...
        std::fs::create_dir_all(
            library.join("steamapps/compatdata/2183900/pfx/drive_c/users/steamuser"),
        )
        .unwrap();

        let paths = GamePaths::from_game_dir(&game, &library).unwrap();
        assert!(matches!(paths.save_dir(None).unwrap_err(), Error::NoSaveUser(_)));
    }

    // Unix only: the fixture builds the savegames inside a Proton prefix
    // under a temporary directory, which is where `user_profile_root`
    // looks on Linux. On Windows it returns the real `%USERPROFILE%`,
    // which a test must not write into. See CLAUDE.md → Platforms: this is
    // the gap that leaves Windows save handling without coverage.
    #[cfg(unix)]
    #[test]
    fn reports_multiple_steam_profiles_instead_of_guessing() {
        let (tmp, paths) = fixture();
        let user = tmp
            .path()
            .join("SteamLibrary/steamapps/compatdata/2183900/pfx/drive_c/users/steamuser")
            .join("AppData/Local/Saber/Space Marine 2/storage/steam/user");
        std::fs::create_dir_all(user.join("76561198000000000/Main")).unwrap();

        assert!(matches!(paths.save_dir(None).unwrap_err(), Error::AmbiguousSaveUser(_)));
    }

    /// `settings.toml`'s `steam_user` (2b): when several user directories
    /// are present, a matching preset resolves the otherwise fatal
    /// ambiguity.
    // Unix only: the fixture builds the savegames inside a Proton prefix
    // under a temporary directory, which is where `user_profile_root`
    // looks on Linux. On Windows it returns the real `%USERPROFILE%`,
    // which a test must not write into. See CLAUDE.md → Platforms: this is
    // the gap that leaves Windows save handling without coverage.
    #[cfg(unix)]
    #[test]
    fn steam_user_override_resolves_ambiguity_when_it_matches_one_of_the_found_ids() {
        let (tmp, paths) = fixture();
        let user_root = tmp
            .path()
            .join("SteamLibrary/steamapps/compatdata/2183900/pfx/drive_c/users/steamuser")
            .join("AppData/Local/Saber/Space Marine 2/storage/steam/user");
        std::fs::create_dir_all(user_root.join("76561198000000000/Main")).unwrap();
        // The fixture already creates 76561198412726373/Main.

        let saves = paths.save_dir(Some("76561198000000000")).unwrap();

        assert!(saves.ends_with("76561198000000000/Main"));
    }

    /// A preset that matches none of the user directories found (a typo in
    /// `settings.toml`, say) must produce a clear error of its own that
    /// names the profiles actually present — rather than being silently
    /// ignored or disappearing into `AmbiguousSaveUser`.
    // Unix only: the fixture builds the savegames inside a Proton prefix
    // under a temporary directory, which is where `user_profile_root`
    // looks on Linux. On Windows it returns the real `%USERPROFILE%`,
    // which a test must not write into. See CLAUDE.md → Platforms: this is
    // the gap that leaves Windows save handling without coverage.
    #[cfg(unix)]
    #[test]
    fn steam_user_override_that_matches_nothing_names_the_available_ids() {
        let (_tmp, paths) = fixture();

        let err = paths.save_dir(Some("00000000000000000")).unwrap_err();

        let Error::UnknownSaveUser { requested, available } = err else {
            panic!("expected Error::UnknownSaveUser, got {err:?}");
        };
        assert_eq!(requested, "00000000000000000");
        assert_eq!(available, vec!["76561198412726373".to_string()]);
    }

    #[test]
    fn lists_only_pak_files_alphabetically() {
        let (_tmp, paths) = fixture();
        let mods = paths.mods_dir();
        std::fs::write(mods.join("z.pak"), b"x").unwrap();
        std::fs::write(mods.join("a.pak"), b"x").unwrap();
        std::fs::write(mods.join("readme.txt"), b"x").unwrap();
        std::fs::write(mods.join("pak_config.yaml"), b"[]").unwrap();

        assert_eq!(paths.list_paks().unwrap(), vec!["a.pak", "z.pak"]);
    }

    /// Builds the two sides of a rename: a directory under the old name
    /// holding one recognisable file, and the path the new name would
    /// use, which does not exist yet.
    fn legacy_and_current(tmp: &Path) -> (PathBuf, PathBuf) {
        let legacy = tmp.join(crate::LEGACY_APP_SLUG);
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("settings.toml"), b"language = \"de\"\n").unwrap();
        (legacy, tmp.join(crate::APP_SLUG))
    }

    #[test]
    fn a_directory_of_the_previous_name_is_moved_over_with_its_contents() {
        let tmp = tempfile::tempdir().unwrap();
        let (legacy, current) = legacy_and_current(tmp.path());

        assert!(migrate_legacy_dir(&legacy, &current));

        assert_eq!(
            std::fs::read_to_string(current.join("settings.toml")).unwrap(),
            "language = \"de\"\n"
        );
        assert!(!legacy.exists(), "the old directory must not be left behind as a second copy");
    }

    /// The decisive guard: once the program has written anything under the
    /// new name, that is the current state. A leftover directory under the
    /// old name must not be allowed to overwrite it — that would not
    /// rescue settings, it would discard them.
    #[test]
    fn an_existing_current_directory_is_never_overwritten() {
        let tmp = tempfile::tempdir().unwrap();
        let (legacy, current) = legacy_and_current(tmp.path());
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(current.join("settings.toml"), b"language = \"en\"\n").unwrap();

        assert!(!migrate_legacy_dir(&legacy, &current));

        assert_eq!(
            std::fs::read_to_string(current.join("settings.toml")).unwrap(),
            "language = \"en\"\n"
        );
        assert!(legacy.exists(), "the old directory stays untouched for the user to look at");
    }

    /// The ordinary case on a fresh installation: nothing to move, and no
    /// directory conjured up in passing.
    #[test]
    fn a_missing_legacy_directory_is_not_an_error_and_creates_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join(crate::LEGACY_APP_SLUG);
        let current = tmp.path().join(crate::APP_SLUG);

        assert!(!migrate_legacy_dir(&legacy, &current));

        assert!(!current.exists());
    }

    /// A stray *file* under the old name is not a configuration directory.
    /// Renaming it would put a file where the program expects to create a
    /// directory, and every later write would fail.
    #[test]
    fn a_file_under_the_previous_name_is_not_mistaken_for_a_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join(crate::LEGACY_APP_SLUG);
        std::fs::write(&legacy, b"not a directory").unwrap();
        let current = tmp.path().join(crate::APP_SLUG);

        assert!(!migrate_legacy_dir(&legacy, &current));

        assert!(!current.exists());
        assert!(legacy.is_file());
    }

    /// The new name's parent (`~/.local/state`, say) need not exist yet on
    /// a machine where nothing has ever written there.
    #[test]
    fn a_missing_parent_of_the_target_is_created() {
        let tmp = tempfile::tempdir().unwrap();
        let (legacy, _) = legacy_and_current(tmp.path());
        let current = tmp.path().join("state").join(crate::APP_SLUG);

        assert!(migrate_legacy_dir(&legacy, &current));

        assert!(current.join("settings.toml").is_file());
    }

    /// A configuration directory pointed somewhere else by a dotfiles tool
    /// has to survive the rename as a symlink. Following it and moving the
    /// target would tear the file out of the dotfiles repository it is
    /// checked into.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_legacy_directory_is_moved_as_a_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let elsewhere = tmp.path().join("dotfiles");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("settings.toml"), b"language = \"de\"\n").unwrap();

        let legacy = tmp.path().join(crate::LEGACY_APP_SLUG);
        std::os::unix::fs::symlink(&elsewhere, &legacy).unwrap();
        let current = tmp.path().join(crate::APP_SLUG);

        assert!(migrate_legacy_dir(&legacy, &current));

        assert!(current.symlink_metadata().unwrap().file_type().is_symlink());
        assert!(elsewhere.join("settings.toml").is_file(), "the target stays where it is");
    }

    #[cfg(unix)]
    #[test]
    fn lists_pak_files_reached_through_a_symlink() {
        let (_tmp, paths) = fixture();
        let mods = paths.mods_dir();
        let real_dir = _tmp.path().join("outside");
        std::fs::create_dir_all(&real_dir).unwrap();
        let real_pak = real_dir.join("b.pak");
        std::fs::write(&real_pak, b"x").unwrap();
        std::os::unix::fs::symlink(&real_pak, mods.join("b.pak")).unwrap();
        std::fs::write(mods.join("a.pak"), b"x").unwrap();

        assert_eq!(paths.list_paks().unwrap(), vec!["a.pak", "b.pak"]);
    }
}

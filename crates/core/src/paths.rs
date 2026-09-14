use crate::error::{Error, Result};
use crate::platform::{Current, Platform};
use crate::APP_ID;
use std::path::{Path, PathBuf};

/// Alle Pfade rund um eine Spielinstallation.
#[derive(Debug, Clone)]
pub struct GamePaths {
    pub game_dir: PathBuf,
    /// Die Steam-Bibliothek, in der das Spiel liegt. Der Proton-Prefix hängt
    /// daran, nicht am Spielverzeichnis.
    pub library_dir: PathBuf,
}

impl GamePaths {
    /// Prüft, ob `game_dir` tatsächlich eine Space-Marine-2-Installation ist.
    pub fn from_game_dir(game_dir: &Path, library_dir: &Path) -> Result<Self> {
        if !game_dir.join("client_pc/root/mods").is_dir() {
            return Err(Error::NotAGameDir(game_dir.to_path_buf()));
        }
        Ok(Self {
            game_dir: game_dir.to_path_buf(),
            library_dir: library_dir.to_path_buf(),
        })
    }

    /// Findet das Spiel über die Steam-Bibliotheken.
    ///
    /// `steamlocate` deckt den Regelfall ab (Standard- wie benutzerdefinierte
    /// Bibliotheken über `libraryfolders.vdf`, Flatpak-Steam eingeschlossen).
    /// Findet es überhaupt keine Steam-Installation, greift als Rückfall
    /// `Platform::steam_roots` (siehe `find_in_roots`) – bislang deklariert,
    /// aber ungenutzt, obwohl `steamlocate` selbst intern denselben
    /// Wurzel-Suchraum abdeckt. Der Rückfall prüft nur die Standard-Bibliothek
    /// direkt unter jeder Wurzel, nicht deren eigene
    /// `libraryfolders.vdf` – das deckt `steamlocate` im Erfolgsfall bereits
    /// ab, und wenn schon dieses robustere Vorgehen scheitert, ist eine
    /// ungewöhnliche Custom-Bibliothek ohnehin nur noch manuell über
    /// `settings.toml`s `game_dir` erreichbar.
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

    /// Prüft jede der übergebenen Steam-Wurzeln direkt auf eine
    /// Space-Marine-2-Installation unter `steamapps/common/Space Marine 2`.
    /// Eigene Funktion (statt inline in `discover`), damit sie ohne Umweg
    /// über `$HOME` (von dem `Platform::steam_roots` abhängt) mit
    /// synthetischen Wurzeln aus einem `tempfile`-Fixture getestet werden
    /// kann.
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

    /// Das Savegame-Verzeichnis im Proton-Prefix.
    ///
    /// Unterhalb von `AppData/Local` ist der Pfad mit Windows identisch –
    /// nur die Wurzel liefert die Plattform.
    pub fn save_dir(&self) -> Result<PathBuf> {
        let prefix_root = Current::user_profile_root(APP_ID, &self.library_dir);
        if !prefix_root.is_dir() {
            return Err(Error::PrefixMissing(APP_ID));
        }

        let user_root =
            prefix_root.join("AppData/Local/Saber/Space Marine 2/storage/steam/user");

        // Ein fehlgeschlagenes read_dir hier bedeutet nicht zwingend, dass der
        // Ordner nie existiert hat: bei einer frischen Installation legt Proton
        // den Prefix beim ersten Start an, aber der Saber-Ordner entsteht erst,
        // wenn das Spiel tatsächlich einen Spielstand schreibt. Beide Fälle
        // sind für den Nutzer gleich zu behandeln – es gibt (noch) kein Profil.
        let mut user_ids: Vec<String> = std::fs::read_dir(&user_root)
            .map_err(|_| Error::NoSaveUser(user_root.clone()))?
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        user_ids.sort();

        match user_ids.len() {
            0 => Err(Error::NoSaveUser(user_root)),
            1 => Ok(user_root.join(&user_ids[0]).join("Main")),
            _ => Err(Error::AmbiguousSaveUser(user_ids)),
        }
    }

    /// Alle `.pak`-Dateien im Mods-Verzeichnis, alphabetisch.
    ///
    /// `Path::is_file` folgt Symlinks, ein Symlink auf eine `.pak`-Datei wird
    /// also korrekt mitgezählt; ein toter oder auf ein Verzeichnis zeigender
    /// Symlink wird korrekt ausgeschlossen.
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

/// Die XDG-Verzeichnisse der Anwendung.
#[derive(Debug, Clone)]
pub struct AppDirs {
    pub config: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
}

pub fn app_dirs() -> Result<AppDirs> {
    let project_dirs = directories::ProjectDirs::from("", "", "sm2-modloader").ok_or_else(|| {
        Error::PlainIo(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Basisverzeichnisse des Systems nicht ermittelbar",
        ))
    })?;

    Ok(AppDirs {
        config: project_dirs.config_dir().to_path_buf(),
        data: project_dirs.data_dir().to_path_buf(),
        // state_dir() liefert unter Linux mit XDG_STATE_HOME praktisch immer
        // Some(..); der Fallback greift nur auf Plattformen ohne eigenes
        // State-Verzeichnis und verwendet dann bewusst denselben Ort wie die
        // übrigen persistenten Daten.
        state: project_dirs
            .state_dir()
            .unwrap_or_else(|| project_dirs.data_dir())
            .to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Baut einen Verzeichnisbaum, der einer echten Installation entspricht.
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

    // --- find_in_roots (Rückfall für discover(), wenn steamlocate scheitert) --

    #[test]
    fn find_in_roots_locates_the_game_in_a_later_root() {
        let tmp = tempfile::tempdir().unwrap();
        let empty_root = tmp.path().join("KeinSteamHier");
        let real_root = tmp.path().join("SteamRoot");
        let game = real_root.join("steamapps/common/Space Marine 2");
        std::fs::create_dir_all(game.join("client_pc/root/mods")).unwrap();

        let paths = GamePaths::find_in_roots(&[empty_root, real_root.clone()]).unwrap();

        assert_eq!(paths.game_dir, game);
        assert_eq!(paths.library_dir, real_root, "Bibliothek ist die Wurzel selbst");
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

    #[test]
    fn resolves_save_dir_in_proton_prefix() {
        let (_tmp, paths) = fixture();
        let saves = paths.save_dir().unwrap();
        assert!(saves.ends_with("storage/steam/user/76561198412726373/Main"));
        assert!(saves.is_dir());
    }

    #[test]
    fn reports_missing_prefix_clearly() {
        let tmp = tempfile::tempdir().unwrap();
        let library = tmp.path().join("SteamLibrary");
        let game = library.join("steamapps/common/Space Marine 2");
        std::fs::create_dir_all(game.join("client_pc/root/mods")).unwrap();

        let paths = GamePaths::from_game_dir(&game, &library).unwrap();
        assert!(matches!(paths.save_dir().unwrap_err(), Error::PrefixMissing(2183900)));
    }

    /// Prefix existiert (Proton hat ihn beim ersten Start angelegt), aber der
    /// Saber-Ordner fehlt noch, weil das Spiel nie über das Hauptmenü hinaus
    /// gestartet und nie gespeichert wurde. Das darf nicht wie ein I/O-Fehler
    /// aussehen, sondern muss als "kein Nutzerprofil gefunden" gemeldet werden.
    #[test]
    fn reports_no_save_user_for_fresh_install_without_saber_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let library = tmp.path().join("SteamLibrary");
        let game = library.join("steamapps/common/Space Marine 2");
        std::fs::create_dir_all(game.join("client_pc/root/mods")).unwrap();
        // Prefix existiert, aber ohne AppData/Local/Saber/...
        std::fs::create_dir_all(
            library.join("steamapps/compatdata/2183900/pfx/drive_c/users/steamuser"),
        )
        .unwrap();

        let paths = GamePaths::from_game_dir(&game, &library).unwrap();
        assert!(matches!(paths.save_dir().unwrap_err(), Error::NoSaveUser(_)));
    }

    #[test]
    fn reports_multiple_steam_profiles_instead_of_guessing() {
        let (tmp, paths) = fixture();
        let user = tmp
            .path()
            .join("SteamLibrary/steamapps/compatdata/2183900/pfx/drive_c/users/steamuser")
            .join("AppData/Local/Saber/Space Marine 2/storage/steam/user");
        std::fs::create_dir_all(user.join("76561198000000000/Main")).unwrap();

        assert!(matches!(paths.save_dir().unwrap_err(), Error::AmbiguousSaveUser(_)));
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

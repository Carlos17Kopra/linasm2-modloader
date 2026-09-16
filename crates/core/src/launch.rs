use crate::error::{Error, Result};
use crate::paths::GamePaths;
use crate::platform::{Current, Platform};
use crate::APP_ID;
use std::path::Path;

/// How the game should be launched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    /// Regular launch through Steam, with EAC.
    Steam,
    /// Direct launch in the existing Proton prefix, without EAC.
    /// For offline modding only — no multiplayer.
    NoEac,
}

/// Launches the game according to `mode`.
///
/// With `LaunchMode::Steam`, Steam resolves the executable itself from its
/// own manifest. `paths` is deliberately left unused here, because
/// `GamePaths::from_game_dir` and `discover` have already validated the
/// installation directory and there is nothing extra to verify for the
/// Steam launch.
pub fn launch(paths: &GamePaths, mode: LaunchMode) -> Result<()> {
    match mode {
        LaunchMode::Steam => Current::launch_via_steam(APP_ID),
        LaunchMode::NoEac => {
            let exe = paths.executable();
            if !exe.is_file() {
                return Err(Error::io(
                    &exe,
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        crate::t!("error.retail_exe_not_found"),
                    ),
                ));
            }
            let env = no_eac_env(&paths.library_dir);
            let env_refs: Vec<(&str, &str)> =
                env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            Current::launch_direct(&exe, &env_refs)
        }
    }
}

/// Is a launch without EAC possible on this system at all?
///
/// Reports whether `umu-run` is installed — that is, whether a direct
/// launch *without* EAC is technically feasible, not whether EAC itself is
/// active or available. The name deliberately mirrors the `bool` returned:
/// `true` means "the EAC-less launch is available". The GUI shows or hides
/// the option accordingly.
pub fn no_eac_available() -> bool {
    Current::direct_launch_available()
}

/// Builds the environment variables for the EAC-less direct launch.
///
/// `WINEPREFIX` is only set when the Proton prefix for this game actually
/// exists. `Platform::launch_direct` sets every variable handed to it with
/// `Command::env`, empty value included — so an empty string would be worse
/// than a missing variable, because `umu-run` would then see a genuinely
/// empty prefix path instead of picking a sensible default itself.
fn no_eac_env(library_dir: &Path) -> Vec<(String, String)> {
    let mut env = vec![
        ("SteamAppId".to_string(), APP_ID.to_string()),
        ("SteamGameId".to_string(), APP_ID.to_string()),
        ("GAMEID".to_string(), format!("umu-{APP_ID}")),
    ];

    let prefix = library_dir
        .join("steamapps/compatdata")
        .join(APP_ID.to_string())
        .join("pfx");
    if prefix.is_dir() {
        env.push(("WINEPREFIX".to_string(), prefix.display().to_string()));
    }

    env
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game_dir_fixture() -> (tempfile::TempDir, GamePaths) {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("Space Marine 2");
        std::fs::create_dir_all(game.join("client_pc/root/mods")).unwrap();
        let paths = GamePaths::from_game_dir(&game, tmp.path()).unwrap();
        (tmp, paths)
    }

    #[test]
    fn no_eac_reports_missing_executable_before_launching_it() {
        let (_tmp, paths) = game_dir_fixture();

        // The .exe does not exist — the error has to name that, not umu. A
        // plain substring check for "Retail.exe" would be unfalsifiable
        // here: `Error::io` always renders the path it is given into the
        // message, whatever the actual text is. A test that only checks the
        // rendered string would therefore stay green even if a completely
        // different error with the same path arose here by accident.
        // Instead the variant and the actual path inside the error are
        // verified.
        let error = launch(&paths, LaunchMode::NoEac).unwrap_err();
        match error {
            Error::Io { path, .. } => {
                assert!(
                    path.to_string_lossy().contains("Retail.exe"),
                    "the error has to name the path of the executable, was: {}",
                    path.display()
                );
            }
            other => panic!("expected Error::Io, got {other:?}"),
        }
    }

    #[test]
    fn no_eac_env_contains_gameid_derived_from_app_id() {
        let dir = tempfile::tempdir().unwrap();
        let env = no_eac_env(dir.path());

        assert!(
            env.contains(&("GAMEID".to_string(), format!("umu-{APP_ID}"))),
            "GAMEID has to be derived from APP_ID: {env:?}"
        );
    }

    #[test]
    fn no_eac_env_omits_wineprefix_when_the_proton_prefix_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        let env = no_eac_env(dir.path());

        assert!(
            env.iter().all(|(k, _)| k != "WINEPREFIX"),
            "without an existing prefix WINEPREFIX must not be set (not even empty): {env:?}"
        );
    }

    #[test]
    fn no_eac_env_sets_wineprefix_to_the_real_path_when_the_proton_prefix_exists() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = dir
            .path()
            .join("steamapps/compatdata")
            .join(APP_ID.to_string())
            .join("pfx");
        std::fs::create_dir_all(&prefix).unwrap();

        let env = no_eac_env(dir.path());

        assert!(
            env.contains(&("WINEPREFIX".to_string(), prefix.display().to_string())),
            "WINEPREFIX has to point at the real prefix path: {env:?}"
        );
    }
}

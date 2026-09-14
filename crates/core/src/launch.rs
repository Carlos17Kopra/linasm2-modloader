use crate::error::{Error, Result};
use crate::paths::GamePaths;
use crate::platform::{Current, Platform};
use crate::APP_ID;
use std::path::Path;

/// Wie das Spiel gestartet werden soll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    /// Regulärer Start über Steam, mit EAC.
    Steam,
    /// Direktstart im vorhandenen Proton-Prefix, ohne EAC.
    /// Nur für Offline-Modding – kein Multiplayer.
    NoEac,
}

/// Startet das Spiel gemäß `mode`.
///
/// Bei `LaunchMode::Steam` übernimmt Steam selbst die Auflösung der
/// Executable anhand seines eigenen Manifests – `paths` wird hier bewusst
/// nicht verwendet, da `GamePaths::from_game_dir` bzw. `discover` das
/// Installationsverzeichnis bereits validiert haben und es für den
/// Steam-Start nichts zusätzlich zu prüfen gibt.
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
                        "Retail.exe nicht gefunden – Spielverzeichnis prüfen",
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

/// Ist ein Start ohne EAC auf diesem System überhaupt möglich?
///
/// Meldet, ob `umu-run` installiert ist – also ob ein Direktstart *ohne*
/// EAC technisch durchführbar ist, nicht ob EAC aktiv oder verfügbar wäre.
/// Der Name spiegelt bewusst das zurückgegebene `bool`: `true` heißt "der
/// EAC-lose Start ist verfügbar". Die GUI blendet die Option danach ein
/// oder aus.
pub fn no_eac_available() -> bool {
    crate::platform::unix::Unix::umu_launcher().is_some()
}

/// Baut die Umgebungsvariablen für den EAC-losen Direktstart.
///
/// `WINEPREFIX` wird nur gesetzt, wenn der Proton-Prefix für dieses Spiel
/// tatsächlich existiert. `Platform::launch_direct` setzt jede übergebene
/// Variable mit `Command::env`, auch mit leerem Wert – ein leerer String
/// wäre also schlimmer als eine fehlende Variable, weil `umu-run` dann
/// einen tatsächlich leeren Prefix-Pfad sieht statt selbst einen sinnvollen
/// Default zu wählen.
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

        // Die .exe existiert nicht – der Fehler muss das benennen, nicht umu.
        let fehler = launch(&paths, LaunchMode::NoEac).unwrap_err();
        let text = fehler.to_string();
        assert!(
            text.contains("Retail.exe") || text.contains("nicht gefunden"),
            "unklare Meldung: {text}"
        );
    }

    #[test]
    fn no_eac_env_contains_gameid_derived_from_app_id() {
        let dir = tempfile::tempdir().unwrap();
        let env = no_eac_env(dir.path());

        assert!(
            env.contains(&("GAMEID".to_string(), format!("umu-{APP_ID}"))),
            "GAMEID muss aus APP_ID abgeleitet werden: {env:?}"
        );
    }

    #[test]
    fn no_eac_env_omits_wineprefix_when_the_proton_prefix_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        let env = no_eac_env(dir.path());

        assert!(
            env.iter().all(|(k, _)| k != "WINEPREFIX"),
            "ohne vorhandenen Prefix darf WINEPREFIX nicht gesetzt werden (auch nicht leer): {env:?}"
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
            "WINEPREFIX muss auf den echten Prefix-Pfad zeigen: {env:?}"
        );
    }
}

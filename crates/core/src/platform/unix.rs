use super::Platform;
use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

pub struct Unix;

impl Unix {
    fn home() -> PathBuf {
        std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
    }

    /// Sucht `umu-run` im PATH. Nötig, um die Windows-Executable im
    /// vorhandenen Proton-Prefix ohne Steam zu starten.
    pub fn umu_launcher() -> Option<PathBuf> {
        which_in_path("umu-run")
    }
}

impl Platform for Unix {
    fn steam_roots() -> Vec<PathBuf> {
        let home = Self::home();
        vec![
            home.join(".local/share/Steam"),
            home.join(".steam/steam"),
            home.join(".steam/root"),
            home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
        ]
    }

    fn user_profile_root(app_id: u32, library: &Path) -> PathBuf {
        library
            .join("steamapps/compatdata")
            .join(app_id.to_string())
            .join("pfx/drive_c/users/steamuser")
    }

    fn launch_via_steam(app_id: u32) -> Result<()> {
        let url = format!("steam://rungameid/{app_id}");
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| Error::io(&url, e))?;
        Ok(())
    }

    fn launch_direct(exe: &Path, env: &[(&str, &str)]) -> Result<()> {
        let umu = Self::umu_launcher().ok_or_else(|| {
            Error::io(
                exe,
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "umu-run nicht im PATH gefunden – für den Start ohne Steam wird \
                     umu-launcher benötigt (https://github.com/Open-Wine-Components/umu-launcher)",
                ),
            )
        })?;

        let arbeitsverzeichnis = exe.parent().unwrap_or(Path::new("."));
        let mut cmd = std::process::Command::new(umu);
        cmd.arg(exe).current_dir(arbeitsverzeichnis);
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.spawn().map_err(|e| Error::io(exe, e))?;
        Ok(())
    }

    fn open_folder(path: &Path) -> Result<()> {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|e| Error::io(path, e))?;
        Ok(())
    }
}

/// Minimaler PATH-Lookup – vermeidet eine Abhängigkeit für zwanzig Zeilen.
fn which_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|kandidat| kandidat.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_profile_root_zeigt_in_den_proton_prefix() {
        let library = Path::new("/spiele/SteamLibrary");
        let wurzel = Unix::user_profile_root(2183900, library);

        assert_eq!(
            wurzel,
            PathBuf::from(
                "/spiele/SteamLibrary/steamapps/compatdata/2183900/pfx/drive_c/users/steamuser"
            )
        );
    }

    #[test]
    fn steam_roots_enthaelt_die_ueblichen_orte() {
        let roots = Unix::steam_roots();
        let als_text: Vec<String> =
            roots.iter().map(|p| p.display().to_string()).collect();

        assert!(als_text.iter().any(|p| p.ends_with(".local/share/Steam")));
        assert!(als_text.iter().any(|p| p.ends_with(".steam/steam")));
        assert!(
            als_text.iter().any(|p| p.contains("com.valvesoftware.Steam")),
            "Flatpak-Steam muss berücksichtigt werden"
        );
    }
}

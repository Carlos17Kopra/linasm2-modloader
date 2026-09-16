use crate::atomic::write_atomic;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Application-wide settings, independent of any single profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Overrides the automatic detection.
    pub game_dir: Option<PathBuf>,
    /// Create a save backup before every modded launch.
    pub auto_backup: bool,
    /// SteamID64, in case several profiles live in the prefix.
    pub steam_user: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { game_dir: None, auto_backup: true, steam_user: None }
    }
}

impl Settings {
    /// Loads the settings from `path`. A missing file yields the default
    /// values (fresh installation).
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| {
                let message = crate::error::describe_toml_error(&text, &e);
                Error::io(path, std::io::Error::new(std::io::ErrorKind::InvalidData, message))
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    /// Writes the settings atomically to `path`, creating the parent
    /// directory if it does not exist yet.
    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).map_err(|e| {
            Error::io(path, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        write_atomic(path, &text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_backup_is_on_by_default() {
        assert!(Settings::default().auto_backup);
    }

    #[test]
    fn missing_file_yields_default_values() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings::load(&dir.path().join("gibt_es_nicht.toml")).unwrap();
        assert!(settings.auto_backup);
        assert!(settings.game_dir.is_none());
        assert!(settings.steam_user.is_none());
    }

    #[test]
    fn empty_file_yields_default_values() {
        // An empty file is valid (empty) TOML; #[serde(default)] has to
        // make sure that turns into the default values instead of an error
        // about missing fields.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, b"").unwrap();

        let settings = Settings::load(&path).unwrap();

        assert!(settings.auto_backup);
        assert!(settings.game_dir.is_none());
        assert!(settings.steam_user.is_none());
    }

    #[test]
    fn save_and_load_round_trips_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        let settings = Settings {
            game_dir: Some(PathBuf::from("/spiele/SM2")),
            auto_backup: false,
            // Obviously made-up SteamID64, not a real one.
            steam_user: Some("11111111111111111".into()),
        };
        settings.save(&path).unwrap();

        let loaded = Settings::load(&path).unwrap();
        assert_eq!(loaded.game_dir, settings.game_dir);
        assert!(!loaded.auto_backup);
        assert_eq!(loaded.steam_user, settings.steam_user);
    }

    #[test]
    fn save_creates_missing_parent_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("deep").join("settings.toml");

        Settings::default().save(&path).unwrap();

        assert!(path.exists());
    }

    #[test]
    fn corrupt_file_is_reported_with_path_in_german() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, b"das ist kein toml : : :").unwrap();

        let err = Settings::load(&path).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains(path.to_str().unwrap()),
            "the error message must contain the path: {message}"
        );
        assert!(
            !message.contains("expected") && !message.contains("invalid"),
            "the error message should be in German, not carry the raw toml message: {message}"
        );
    }
}

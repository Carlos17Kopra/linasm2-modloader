use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Steam-Installation nicht gefunden")]
    SteamNotFound,

    #[error("Space Marine 2 (AppID {0}) ist in keiner Steam-Bibliothek installiert")]
    GameNotFound(u32),

    #[error("Verzeichnis sieht nicht nach Space Marine 2 aus: {0} (erwartet: client_pc/root/mods)")]
    NotAGameDir(PathBuf),

    #[error("Proton-Prefix für AppID {0} fehlt – das Spiel muss mindestens einmal gestartet worden sein")]
    PrefixMissing(u32),

    #[error("kein Steam-Nutzerprofil unter {0} gefunden")]
    NoSaveUser(PathBuf),

    #[error("mehrere Steam-Nutzerprofile gefunden ({0:?}) – bitte eines in den Einstellungen festlegen")]
    AmbiguousSaveUser(Vec<String>),

    #[error("Steam-Nutzerprofil '{requested}' aus den Einstellungen wurde nicht gefunden (vorhanden: {available:?})")]
    UnknownSaveUser { requested: String, available: Vec<String> },

    #[error("pak_config.yaml ist fehlerhaft: {0}")]
    PakConfig(String),

    #[error("Backup beschädigt: {0}")]
    CorruptBackup(String),

    #[error("Save-Verzeichnis enthält an einer sicherheitsrelevanten Stelle einen Symlink, Wiederherstellung abgebrochen: {0}")]
    UnsafeSaveDir(PathBuf),

    #[error("Wiederherstellung fehlgeschlagen, nachdem bereits eine Sicherung des vorherigen Standes angelegt wurde (liegt unter {safety_backup}): {source}")]
    RestoreFailedAfterBackup {
        safety_backup: PathBuf,
        #[source]
        source: Box<Error>,
    },

    #[error("kein Werkzeug zum Entpacken von .rar gefunden – bitte 'unar' oder '7zip' installieren (Debian/Ubuntu: apt install unar, Arch: pacman -S unarchiver, Fedora: dnf install unar)")]
    NoRarTool,

    #[error("Archiv enthält keine .pak-Datei: {0}")]
    NoPakInArchive(PathBuf),

    #[error("Archiv enthält keine Savegame-Dateien (.cfg oder .sav): {0}")]
    NoSaveInArchive(PathBuf),

    #[error("Archiv kann nicht importiert werden: {0}")]
    UnusableArchive(String),

    #[error("kein Schreibrecht für {0}")]
    NotWritable(PathBuf),

    #[error("E/A-Fehler bei {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(transparent)]
    PlainIo(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Helper for enriching I/O errors with the path they concern.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io { path: path.into(), source }
    }
}

/// Translates a `toml` deserialization error into a German message with
/// line and column, instead of passing the raw (English) library message on
/// to the user. Mirrors how `Library::load` handles `serde_json` errors
/// (there line and column come straight from `serde_json`;
/// `toml::de::Error` only provides a byte range via `span()`, from which
/// line and column are computed here). Shared between `settings.rs` and
/// `profile.rs`, the two places that load TOML.
pub(crate) fn describe_toml_error(text: &str, error: &toml::de::Error) -> String {
    match error.span() {
        Some(span) => {
            let (line, column) = line_and_column(text, span.start);
            format!("ungültiges TOML (Zeile {line}, Spalte {column})")
        }
        None => "ungültiges TOML".to_string(),
    }
}

/// The 1-based line and column (in characters, not bytes) of the given byte
/// offset in `text`.
fn line_and_column(text: &str, byte_offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut column = 1usize;
    for (i, ch) in text.char_indices() {
        if i >= byte_offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_toml_error_names_line_and_column() {
        let text = "gueltig = 1\nkaputt : : :\n";
        let error = toml::from_str::<toml::Value>(text).unwrap_err();

        let message = describe_toml_error(text, &error);

        assert!(message.contains("Zeile 2"), "{message}");
        assert!(
            !message.contains("expected") && !message.contains("invalid"),
            "the message should be in German, not carry the raw toml message: {message}"
        );
    }
}

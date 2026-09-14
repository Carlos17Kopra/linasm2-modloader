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
    /// Hilfsfunktion, um E/A-Fehler mit dem betroffenen Pfad anzureichern.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io { path: path.into(), source }
    }
}

/// Übersetzt einen `toml`-Deserialisierungsfehler in eine deutsche Meldung
/// mit Zeile und Spalte, statt die rohe (englische) Bibliotheksmeldung an
/// den Nutzer weiterzureichen. Analog zu `Library::load`s Behandlung von
/// `serde_json`-Fehlern (Zeile/Spalte dort direkt von `serde_json`
/// geliefert; `toml::de::Error` liefert nur einen Byte-Bereich über
/// `span()`, aus dem Zeile/Spalte hier selbst berechnet werden). Geteilt
/// zwischen `settings.rs` und `profile.rs`, den beiden TOML-Ladestellen.
pub(crate) fn describe_toml_error(text: &str, error: &toml::de::Error) -> String {
    match error.span() {
        Some(span) => {
            let (line, column) = line_and_column(text, span.start);
            format!("ungültiges TOML (Zeile {line}, Spalte {column})")
        }
        None => "ungültiges TOML".to_string(),
    }
}

/// 1-basierte Zeile und Spalte (in Zeichen, nicht Bytes) des gegebenen
/// Byte-Offsets in `text`.
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
            "Meldung soll auf Deutsch sein, nicht die rohe toml-Meldung enthalten: {message}"
        );
    }
}

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

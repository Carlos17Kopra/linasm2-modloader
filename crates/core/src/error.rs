use crate::i18n;
use std::path::PathBuf;

#[derive(Debug)]
pub enum Error {
    SteamNotFound,
    GameNotFound(u32),
    NotAGameDir(PathBuf),
    PrefixMissing(u32),
    NoSaveUser(PathBuf),
    AmbiguousSaveUser(Vec<String>),
    UnknownSaveUser { requested: String, available: Vec<String> },
    PakConfig(PakConfigDefect),
    CorruptBackup(BackupDefect),
    UnsafeSaveDir(PathBuf),
    RestoreFailedAfterBackup { safety_backup: PathBuf, source: Box<Error> },
    NoRarTool,
    NoPakInArchive(PathBuf),
    NoSaveInArchive(PathBuf),
    UnusableArchive(ArchiveDefect),
    NotWritable(PathBuf),
    Io { path: PathBuf, source: std::io::Error },
    PlainIo(std::io::Error),
}

/// Why a backup failed verification. Its own type rather than a ready-made
/// sentence, so that the detail can be translated as well (see
/// `BackupDefect::text`, looked up through the catalogue like everything
/// else `Display` produces).
#[derive(Debug)]
pub enum BackupDefect {
    NotAZip { path: PathBuf },
    UnknownEntry { name: String },
    DuplicateEntry { name: String },
    SizeMismatch { name: String },
    HashMismatch { name: String },
    CountMismatch { found: usize, expected: usize },
    InvalidPath { name: String },
    EmptyPath,
    OutsideSaveDir,
}

/// Why an archive cannot be imported — same idea as `BackupDefect`.
#[derive(Debug)]
pub enum ArchiveDefect {
    NotAZip { path: PathBuf },
    Symlink { name: String },
    InvalidPath { name: String },
    DuplicateName { name: String },
    TooLarge { limit: u64 },
}

/// Why `PakConfig::parse` rejected the input — same idea as `BackupDefect`.
/// `pak_config.rs` used to build these sentences itself with `format!`;
/// that made every one of them permanently German, no matter the active
/// language, because a `String` payload has already frozen its wording by
/// the time `Display` runs. Structured data lets `PakConfigDefect::text`
/// look the sentence up in the catalogue instead.
#[derive(Debug)]
pub enum PakConfigDefect {
    InvalidYaml { line: usize, column: usize },
    RootNotAList,
    EntryNotAnObject { index: usize },
    EntryMissingPakKey { index: usize },
    EntryInvalidDisabledValue { index: usize, found: YamlScalarShape },
}

/// What was found in place of a boolean `disabled:` value. `Literal`
/// already carries its rendered, language-independent form (a quoted
/// string, a number, `true`/`false`, `null`) — data, not text. `List` and
/// `Object` carry no data at all, only a shape: the one-word description
/// ("a list" / "an object") is resolved by `PakConfigDefect::text` at
/// `Display` time, not baked in at parse time. Baking it in earlier would
/// freeze the word in whichever language happened to be active while
/// `PakConfig::parse` ran — which, now that the GUI can switch languages
/// live, is not necessarily the language active when the error is shown.
#[derive(Debug)]
pub enum YamlScalarShape {
    Literal(String),
    List,
    Object,
}

impl BackupDefect {
    fn text(&self) -> String {
        match self {
            BackupDefect::NotAZip { path } => {
                i18n::format("error.backup_defect.not_a_zip", &[("path", path.display().to_string())])
            }
            BackupDefect::UnknownEntry { name } => {
                i18n::format("error.backup_defect.unknown_entry", &[("name", name.clone())])
            }
            BackupDefect::DuplicateEntry { name } => {
                i18n::format("error.backup_defect.duplicate_entry", &[("name", name.clone())])
            }
            BackupDefect::SizeMismatch { name } => {
                i18n::format("error.backup_defect.size_mismatch", &[("name", name.clone())])
            }
            BackupDefect::HashMismatch { name } => {
                i18n::format("error.backup_defect.hash_mismatch", &[("name", name.clone())])
            }
            BackupDefect::CountMismatch { found, expected } => i18n::format(
                "error.backup_defect.count_mismatch",
                &[("found", found.to_string()), ("expected", expected.to_string())],
            ),
            BackupDefect::InvalidPath { name } => {
                i18n::format("error.backup_defect.invalid_path", &[("name", name.clone())])
            }
            BackupDefect::EmptyPath => i18n::lookup("error.backup_defect.empty_path"),
            BackupDefect::OutsideSaveDir => i18n::lookup("error.backup_defect.outside_save_dir"),
        }
    }
}

impl ArchiveDefect {
    fn text(&self) -> String {
        match self {
            ArchiveDefect::NotAZip { path } => {
                i18n::format("error.archive_defect.not_a_zip", &[("path", path.display().to_string())])
            }
            ArchiveDefect::Symlink { name } => {
                i18n::format("error.archive_defect.symlink", &[("name", name.clone())])
            }
            ArchiveDefect::InvalidPath { name } => {
                i18n::format("error.archive_defect.invalid_path", &[("name", name.clone())])
            }
            ArchiveDefect::DuplicateName { name } => {
                i18n::format("error.archive_defect.duplicate_name", &[("name", name.clone())])
            }
            ArchiveDefect::TooLarge { limit } => {
                i18n::format("error.archive_defect.too_large", &[("limit", describe_size(*limit))])
            }
        }
    }
}

impl PakConfigDefect {
    fn text(&self) -> String {
        match self {
            PakConfigDefect::InvalidYaml { line, column } => i18n::format(
                "error.pak_config_defect.invalid_yaml",
                &[("line", line.to_string()), ("column", column.to_string())],
            ),
            PakConfigDefect::RootNotAList => i18n::lookup("error.pak_config_defect.root_not_a_list"),
            PakConfigDefect::EntryNotAnObject { index } => i18n::format(
                "error.pak_config_defect.entry_not_an_object",
                &[("index", index.to_string())],
            ),
            PakConfigDefect::EntryMissingPakKey { index } => i18n::format(
                "error.pak_config_defect.entry_missing_pak_key",
                &[("index", index.to_string())],
            ),
            PakConfigDefect::EntryInvalidDisabledValue { index, found } => {
                let value = match found {
                    YamlScalarShape::Literal(text) => text.clone(),
                    YamlScalarShape::List => i18n::lookup("error.pak_config_defect.scalar_list"),
                    YamlScalarShape::Object => i18n::lookup("error.pak_config_defect.scalar_object"),
                };
                i18n::format(
                    "error.pak_config_defect.entry_invalid_disabled_value",
                    &[("index", index.to_string()), ("value", value)],
                )
            }
        }
    }
}

/// A byte count for an error message: whole MiB once it is worth it, plain
/// bytes below that — a limit shown as "0 MiB" would tell the user nothing.
/// Lives here rather than in `saves.rs` because it is exclusively error
/// text now (only `ArchiveDefect::TooLarge` calls it).
fn describe_size(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    if bytes >= MIB {
        i18n::format("error.size.mib", &[("value", (bytes / MIB).to_string())])
    } else {
        i18n::format("error.size.bytes", &[("value", bytes.to_string())])
    }
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Helper for enriching I/O errors with the path they concern.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io { path: path.into(), source }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Error::SteamNotFound => i18n::lookup("error.steam_not_found"),
            Error::GameNotFound(app_id) => {
                i18n::format("error.game_not_found", &[("app_id", app_id.to_string())])
            }
            Error::NotAGameDir(path) => {
                i18n::format("error.not_a_game_dir", &[("path", path.display().to_string())])
            }
            Error::PrefixMissing(app_id) => {
                i18n::format("error.prefix_missing", &[("app_id", app_id.to_string())])
            }
            Error::NoSaveUser(path) => {
                i18n::format("error.no_save_user", &[("path", path.display().to_string())])
            }
            Error::AmbiguousSaveUser(users) => {
                i18n::format("error.ambiguous_save_user", &[("users", users.join(", "))])
            }
            Error::UnknownSaveUser { requested, available } => i18n::format(
                "error.unknown_save_user",
                &[("requested", requested.clone()), ("available", available.join(", "))],
            ),
            Error::PakConfig(defect) => {
                i18n::format("error.pak_config", &[("detail", defect.text())])
            }
            Error::CorruptBackup(defect) => {
                i18n::format("error.corrupt_backup", &[("detail", defect.text())])
            }
            Error::UnsafeSaveDir(path) => {
                i18n::format("error.unsafe_save_dir", &[("path", path.display().to_string())])
            }
            Error::RestoreFailedAfterBackup { safety_backup, source } => i18n::format(
                "error.restore_failed_after_backup",
                &[
                    ("safety_backup", safety_backup.display().to_string()),
                    ("source", source.to_string()),
                ],
            ),
            Error::NoRarTool => i18n::lookup("error.no_rar_tool"),
            Error::NoPakInArchive(path) => {
                i18n::format("error.no_pak_in_archive", &[("path", path.display().to_string())])
            }
            Error::NoSaveInArchive(path) => {
                i18n::format("error.no_save_in_archive", &[("path", path.display().to_string())])
            }
            Error::UnusableArchive(defect) => {
                i18n::format("error.unusable_archive", &[("detail", defect.text())])
            }
            Error::NotWritable(path) => {
                i18n::format("error.not_writable", &[("path", path.display().to_string())])
            }
            Error::Io { path, source } => i18n::format(
                "error.io",
                &[("path", path.display().to_string()), ("source", source.to_string())],
            ),
            Error::PlainIo(source) => source.to_string(),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            Error::PlainIo(source) => Some(source),
            Error::RestoreFailedAfterBackup { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(source: std::io::Error) -> Self {
        Error::PlainIo(source)
    }
}

/// Translates a `toml` deserialization error into a message with line and
/// column, instead of passing the raw (English) library message on to the
/// user. Mirrors how `Library::load` handles `serde_json` errors (there line
/// and column come straight from `serde_json`; `toml::de::Error` only
/// provides a byte range via `span()`, from which line and column are
/// computed here). Shared between `settings.rs` and `profile.rs`, the two
/// places that load TOML.
pub(crate) fn describe_toml_error(text: &str, error: &toml::de::Error) -> String {
    match error.span() {
        Some(span) => {
            let (line, column) = line_and_column(text, span.start);
            i18n::format(
                "error.invalid_toml",
                &[("line", line.to_string()), ("column", column.to_string())],
            )
        }
        None => i18n::lookup("error.invalid_toml_unknown"),
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
    use crate::i18n::{set_language, Language};

    #[test]
    fn describe_toml_error_names_line_and_column() {
        // Guards against the same race the three tests below guard
        // against: `set_language` flips one process-wide static, and
        // `cargo test` runs this crate's tests concurrently by default.
        let _guard = crate::i18n::language_test_lock();
        let text = "gueltig = 1\nkaputt : : :\n";
        let error = toml::from_str::<toml::Value>(text).unwrap_err();

        set_language(Language::German);
        let message = describe_toml_error(text, &error);
        set_language(Language::English);

        assert!(message.contains("Zeile 2"), "{message}");
        assert!(
            !message.contains("expected") && !message.contains("invalid"),
            "the message must be composed from the catalogue, not carry the raw toml message: {message}"
        );
    }

    /// The point of the whole rebuild: the same error speaks whichever
    /// language is set, without a single caller changing.
    #[test]
    fn an_error_speaks_the_active_language() {
        let _guard = crate::i18n::language_test_lock();
        let error = Error::GameNotFound(2183900);

        set_language(Language::English);
        let english = error.to_string();
        set_language(Language::German);
        let german = error.to_string();
        set_language(Language::English);

        assert!(english.contains("is not installed"), "{english}");
        assert!(german.contains("ist in keiner"), "{german}");
        assert!(english.contains("2183900") && german.contains("2183900"));
    }

    #[test]
    fn a_defect_reason_is_part_of_the_message() {
        let _guard = crate::i18n::language_test_lock();
        set_language(Language::English);
        let error = Error::CorruptBackup(BackupDefect::DuplicateEntry { name: "slot1.sav".into() });

        let text = error.to_string();

        assert!(text.contains("slot1.sav"), "{text}");
        assert!(text.contains("more than once"), "{text}");
    }

    /// The chain has to survive the loss of `thiserror`: an I/O error still
    /// names its cause.
    #[test]
    fn an_io_error_keeps_its_source() {
        let error = Error::io("/tmp/x", std::io::Error::new(std::io::ErrorKind::NotFound, "weg"));

        assert!(std::error::Error::source(&error).is_some());
    }
}

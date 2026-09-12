use crate::error::{Error, Result};
use std::io::Write;
use std::path::Path;

/// Schreibt `contents` nach `path`, ohne dass ein Abbruch eine
/// unvollständige Datei hinterlassen kann: temporäre Datei im selben
/// Verzeichnis, fsync, dann rename. Rename ist auf POSIX atomar.
pub fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let dir = path.parent().ok_or_else(|| {
        Error::io(path, std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Pfad hat kein Elternverzeichnis",
        ))
    })?;

    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;

    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|e| Error::io(dir, e))?;
    tmp.write_all(contents.as_bytes()).map_err(|e| Error::io(path, e))?;
    tmp.as_file().sync_all().map_err(|e| Error::io(path, e))?;
    tmp.persist(path).map_err(|e| Error::io(path, e.error))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schreibt_inhalt_und_hinterlaesst_keine_temp_dateien() {
        let dir = tempfile::tempdir().unwrap();
        let ziel = dir.path().join("pak_config.yaml");

        write_atomic(&ziel, "- pak: a.pak\n").unwrap();

        assert_eq!(std::fs::read_to_string(&ziel).unwrap(), "- pak: a.pak\n");
        let eintraege: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(eintraege.len(), 1, "temporäre Datei wurde nicht aufgeräumt");
    }

    #[test]
    fn ueberschreibt_vorhandene_datei_vollstaendig() {
        let dir = tempfile::tempdir().unwrap();
        let ziel = dir.path().join("pak_config.yaml");
        std::fs::write(&ziel, "sehr langer alter Inhalt der weg muss").unwrap();

        write_atomic(&ziel, "kurz\n").unwrap();

        assert_eq!(std::fs::read_to_string(&ziel).unwrap(), "kurz\n");
    }
}

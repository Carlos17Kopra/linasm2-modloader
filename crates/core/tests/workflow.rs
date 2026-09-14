//! Integrationstest über den gesamten Ablauf: Import, Aktivierung, Profile,
//! Abgleich mit dem Verzeichnis und Savegame-Sicherung greifen hier
//! zusammen, wie ein Nutzer sie tatsächlich durchläuft. Die Modultests in
//! `sm2-core` selbst prüfen jeden Baustein einzeln.

use std::io::Write;
use std::path::Path;

use sm2_core::library::Library;
use sm2_core::pak_config::PakConfig;
use sm2_core::paths::GamePaths;
use sm2_core::profile::Profile;
use sm2_core::{import, saves};

/// Baut ein Spielverzeichnis samt Proton-Prefix und Savegames, wie es einer
/// echten Installation entspricht.
///
/// Die SteamID ist frei erfunden (17 Ziffern, aber an keine echte ID
/// angelehnt) – `GamePaths::save_dir` wählt ohnehin das einzige vorhandene
/// Nutzerverzeichnis aus, unabhängig von dessen konkretem Namen.
fn world() -> (tempfile::TempDir, GamePaths) {
    let tmp = tempfile::tempdir().unwrap();
    let library = tmp.path().join("SteamLibrary");
    let game = library.join("steamapps/common/Space Marine 2");
    std::fs::create_dir_all(game.join("client_pc/root/mods")).unwrap();

    let save_files_dir = library
        .join("steamapps/compatdata/2183900/pfx/drive_c/users/steamuser")
        .join("AppData/Local/Saber/Space Marine 2/storage/steam/user/11111111111111111/Main");
    std::fs::create_dir_all(&save_files_dir).unwrap();
    std::fs::write(save_files_dir.join("profile.sav"), b"FORTSCHRITT").unwrap();

    let paths = GamePaths::from_game_dir(&game, &library).unwrap();
    (tmp, paths)
}

/// Baut ein Zip-Archiv mit den gegebenen Einträgen – dieselbe Hilfsfunktion
/// wie in den Modultests von `import.rs`, hier gebraucht, um den Weg über ein
/// echtes Archiv statt einer bloßen `.pak`-Datei zu prüfen.
fn zip_with(files: &[(&str, &[u8])], to: &Path) {
    let file = std::fs::File::create(to).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
    for (name, content) in files {
        zip.start_file(*name, opts).unwrap();
        zip.write_all(content).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn from_import_through_profile_to_restoration() {
    let (tmp, paths) = world();
    let app_data = tmp.path().join("appdata");
    let backup_root = app_data.join("backups/saves");
    let profile_dir = app_data.join("profiles");
    std::fs::create_dir_all(&profile_dir).unwrap();

    let mut lib = Library::default();
    let mut cfg = PakConfig::default();

    // Zwei Mods importieren – beide landen deaktiviert.
    let downloads = tmp.path().join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    for (name, content) in [("astartes.pak", &b"ASTARTES"[..]), ("chaplain.pak", &b"CHAPLAIN"[..])] {
        let source = downloads.join(name);
        std::fs::write(&source, content).unwrap();
        import::import_pak(&paths, &mut lib, &mut cfg, &source, None).unwrap();
    }
    assert_eq!(cfg.entries.len(), 2);
    assert!(cfg.entries.iter().all(|e| e.disabled), "Import darf nichts aktivieren");

    // Einen aktivieren, Reihenfolge festlegen, in die Engine-Datei schreiben.
    cfg.entries.iter_mut().find(|e| e.pak == "astartes.pak").unwrap().disabled = false;
    cfg.entries.reverse();
    cfg.save(&paths.pak_config_path()).unwrap();

    // Die Engine würde exakt das lesen.
    let loaded = PakConfig::load(&paths.pak_config_path()).unwrap();
    assert_eq!(loaded, cfg);
    assert_eq!(loaded.enabled().count(), 1);

    // Zustand als Profil sichern, dann alles abschalten (Vanilla).
    let profile = Profile::from_config("Astartes", &cfg);
    profile.save(&profile_dir).unwrap();

    for entry in &mut cfg.entries {
        entry.disabled = true;
    }
    cfg.save(&paths.pak_config_path()).unwrap();
    assert_eq!(
        PakConfig::load(&paths.pak_config_path()).unwrap().enabled().count(),
        0,
        "Vanilla-Zustand: keine Mods aktiv"
    );

    // Profil wiederherstellen.
    let (restored, missing) = profile.apply(&paths.list_paks().unwrap());
    assert!(missing.is_empty());
    restored.save(&paths.pak_config_path()).unwrap();
    let after_apply = PakConfig::load(&paths.pak_config_path()).unwrap();
    assert_eq!(
        after_apply.enabled().map(|e| e.pak.as_str()).collect::<Vec<_>>(),
        vec!["astartes.pak"]
    );
    assert_eq!(
        after_apply, restored,
        "die Engine-Datei muss exakt dem wiederhergestellten Profil-Zustand entsprechen"
    );

    // Savegame sichern, kaputtmachen, wiederherstellen.
    let save_dir = paths.save_dir(None).unwrap();
    let backup_entry = saves::backup(&save_dir, &backup_root, Some("vor Modded-Start")).unwrap();
    saves::verify(&backup_entry).unwrap();

    std::fs::write(save_dir.join("profile.sav"), b"ZERSTOERT").unwrap();
    let safety_backup = saves::restore(&backup_entry, &save_dir, &backup_root).unwrap();

    assert_eq!(std::fs::read(save_dir.join("profile.sav")).unwrap(), b"FORTSCHRITT");
    // Auch der zerstörte Stand ist noch da, falls die Wiederherstellung falsch gewesen wäre –
    // genau die Garantie, um derentwillen `restore` seine eigene Sicherung anlegt und verifiziert.
    saves::verify(&safety_backup).unwrap();
}

#[test]
fn reconcile_catches_manual_interventions() {
    let (_tmp, paths) = world();
    let mut cfg = PakConfig::default();

    // Jemand kopiert ein Pak von Hand hinein – die Engine würde es ungesteuert laden.
    std::fs::write(paths.mods_dir().join("vonhand.pak"), b"X").unwrap();

    let result = cfg.reconcile(&paths.list_paks().unwrap(), &std::collections::HashMap::new());

    assert_eq!(result.added, vec!["vonhand.pak"]);
    assert_eq!(cfg.entries.len(), 1);
    assert!(!cfg.entries[0].disabled, "es lädt ohnehin – also steuerbar machen");
}

/// Die meisten Nutzer importieren nicht eine nackte `.pak`-Datei, sondern ein
/// heruntergeladenes Archiv. Dieser Test prüft den vollständigen Weg über
/// `extract_paks` und `import_pak` gemeinsam.
#[test]
fn importing_from_a_real_archive_extracts_and_registers_disabled() {
    let (tmp, paths) = world();
    let downloads = tmp.path().join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();

    let archive = downloads.join("mod_pack.zip");
    zip_with(&[("readme.txt", b"lies mich"), ("Mein Mod/gunner.pak", b"GUNNER")], &archive);

    let extracted_dir = tmp.path().join("extracted");
    std::fs::create_dir_all(&extracted_dir).unwrap();
    let extracted = import::extract_paks(&archive, &extracted_dir).unwrap();
    assert_eq!(extracted.len(), 1, "nur die .pak-Datei zählt, readme.txt wird ignoriert");

    let mut lib = Library::default();
    let mut cfg = PakConfig::default();
    let outcome =
        import::import_pak(&paths, &mut lib, &mut cfg, &extracted[0], Some("mod_pack.zip")).unwrap();

    assert_eq!(outcome.pak, "gunner.pak");
    assert!(outcome.duplicate_of.is_none());
    assert!(paths.mods_dir().join("gunner.pak").is_file());
    assert_eq!(std::fs::read(paths.mods_dir().join("gunner.pak")).unwrap(), b"GUNNER");
    assert!(
        cfg.entries.last().unwrap().disabled,
        "Import aus einem Archiv darf ebenso wenig aktivieren wie der Import einer bloßen .pak-Datei"
    );
    assert_eq!(lib.mods["gunner.pak"].source.as_deref(), Some("mod_pack.zip"));
}

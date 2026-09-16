//! Integration test over the whole flow: import, activation, profiles,
//! reconciliation with the directory and savegame backup all come together
//! here the way a user actually goes through them. The unit tests in
//! `sm2-core` itself check each building block on its own.

use std::io::Write;
use std::path::Path;

use sm2_core::library::Library;
use sm2_core::pak_config::PakConfig;
use sm2_core::paths::GamePaths;
use sm2_core::profile::Profile;
use sm2_core::{import, saves};

/// Builds a game directory with Proton prefix and savegames, matching a
/// real installation.
///
/// The SteamID is made up (17 digits, but modelled on no real ID) —
/// `GamePaths::save_dir` picks the only user directory present anyway,
/// whatever its concrete name.
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

/// Builds a zip archive with the given entries — the same helper as in the
/// unit tests of `import.rs`, needed here to check the path through a real
/// archive rather than a bare `.pak` file.
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

    // Import two mods — both end up disabled.
    let downloads = tmp.path().join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    for (name, content) in [("astartes.pak", &b"ASTARTES"[..]), ("chaplain.pak", &b"CHAPLAIN"[..])] {
        let source = downloads.join(name);
        std::fs::write(&source, content).unwrap();
        import::import_pak(&paths, &mut lib, &mut cfg, &source, None).unwrap();
    }
    assert_eq!(cfg.entries.len(), 2);
    assert!(cfg.entries.iter().all(|e| e.disabled), "an import must not enable anything");

    // Enable one, fix the order, write it to the engine file.
    cfg.entries.iter_mut().find(|e| e.pak == "astartes.pak").unwrap().disabled = false;
    cfg.entries.reverse();
    cfg.save(&paths.pak_config_path()).unwrap();

    // The engine would read exactly this.
    let loaded = PakConfig::load(&paths.pak_config_path()).unwrap();
    assert_eq!(loaded, cfg);
    assert_eq!(loaded.enabled().count(), 1);

    // Save the state as a profile, then switch everything off (vanilla).
    let profile = Profile::from_config("Astartes", &cfg);
    profile.save(&profile_dir).unwrap();

    for entry in &mut cfg.entries {
        entry.disabled = true;
    }
    cfg.save(&paths.pak_config_path()).unwrap();
    assert_eq!(
        PakConfig::load(&paths.pak_config_path()).unwrap().enabled().count(),
        0,
        "vanilla state: no mods active"
    );

    // Restore the profile.
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
        "the engine file has to match the restored profile state exactly"
    );

    // Back up the save, break it, restore it.
    let save_dir = paths.save_dir(None).unwrap();
    let backup_entry = saves::backup(&save_dir, &backup_root, Some("vor Modded-Start")).unwrap();
    saves::verify(&backup_entry).unwrap();

    std::fs::write(save_dir.join("profile.sav"), b"ZERSTOERT").unwrap();
    let safety_backup = saves::restore(&backup_entry, &save_dir, &backup_root).unwrap();

    assert_eq!(std::fs::read(save_dir.join("profile.sav")).unwrap(), b"FORTSCHRITT");
    // The destroyed state is still there too, in case the restore had been
    // the wrong one — exactly the guarantee for whose sake `restore`
    // creates and verifies a backup of its own.
    saves::verify(&safety_backup).unwrap();
}

#[test]
fn reconcile_catches_manual_interventions() {
    let (_tmp, paths) = world();
    let mut cfg = PakConfig::default();

    // Someone copies a pak in by hand — the engine would load it
    // uncontrolled.
    std::fs::write(paths.mods_dir().join("vonhand.pak"), b"X").unwrap();

    let result = cfg.reconcile(&paths.list_paks().unwrap(), &std::collections::HashMap::new());

    assert_eq!(result.added, vec!["vonhand.pak"]);
    assert_eq!(cfg.entries.len(), 1);
    assert!(!cfg.entries[0].disabled, "it loads anyway – so make it controllable");
}

/// Most users do not import a bare `.pak` file but a downloaded archive.
/// This test checks the complete path through `extract_paks` and
/// `import_pak` together.
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
    assert_eq!(extracted.len(), 1, "only the .pak file counts, readme.txt is ignored");

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
        "an import from an archive must enable just as little as the import of a bare .pak file"
    );
    assert_eq!(lib.mods["gunner.pak"].source.as_deref(), Some("mod_pack.zip"));
}

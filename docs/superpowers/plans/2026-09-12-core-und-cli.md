# SM2 Mod Loader — Plan 1: Kern und CLI

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eine lauffähige Kommandozeilen-Anwendung, die Space-Marine-2-Mods über die engine-eigene `pak_config.yaml` verwaltet, Savegames sichert und wiederherstellt und das Spiel startet.

**Architecture:** Cargo-Workspace mit `crates/core` (Bibliothek, kennt weder GUI noch CLI) und `crates/app` (Binary). Mods werden nie bewegt — sie liegen dauerhaft in `<game>/client_pc/root/mods/`, und `pak_config.yaml` ist die einzige Quelle für Aktivierungszustand und Ladereihenfolge. Sämtliche Schreibvorgänge an Nutzerdaten sind atomar.

**Tech Stack:** Rust 1.94, `yaml-rust2`, `steamlocate`, `zip`, `sevenz-rust2`, `blake3`, `clap`, `directories`, `thiserror`, `anyhow`, `tracing`, `tempfile`

**Spec:** `docs/superpowers/specs/2026-09-12-sm2-modloader-design.md`

**Nachfolgeplan:** GUI, CI und Release entstehen in Plan 2. Dieser Plan endet mit einem vollständig nutzbaren CLI-Werkzeug.

## Global Constraints

- **Rust Edition 2021**, MSRV 1.85. Keine Nightly-Features.
- **Kein `tokio`, kein Async-Runtime.** Hintergrundarbeit ist `std::thread` plus Channel.
- **Kein `unrar`-Crate** (bindet unfreien Quellcode ein). `.rar` wird über ein externes `unar`/`7z` im `PATH` behandelt.
- **Keine `serde_yaml`-Familie** (deprecated). Lesen mit `yaml-rust2`, Schreiben von Hand.
- **AppID ist 2183900.**
- `crates/core` darf **keine** Abhängigkeit auf `eframe`, `egui`, `clap` oder `rfd` haben.
- Jeder Schreibvorgang an einer Datei, die dem Nutzer oder dem Spiel gehört, läuft über `core::atomic::write_atomic`.
- Alle Fehlermeldungen und CLI-Ausgaben auf **Deutsch**. Bezeichner im Code auf Englisch.
- Exakte Crate-Versionen (am 2026-09-12 geprüft): `yaml-rust2` 0.13, `steamlocate` 2.1, `zip` 8.6, `sevenz-rust2` 0.22, `blake3` 1.8, `clap` 4.6, `directories` 6.0, `thiserror` 2.0, `anyhow` 1.0, `tracing` 0.1, `tempfile` 3.27.

## Dateistruktur

| Datei | Verantwortung |
|---|---|
| `crates/core/src/lib.rs` | Modul-Wurzel, Re-Exports |
| `crates/core/src/error.rs` | `Error`, `Result` — typisierte Fehler der Bibliothek |
| `crates/core/src/atomic.rs` | Atomares Schreiben (temp + fsync + rename) |
| `crates/core/src/pak_config.rs` | `pak_config.yaml` lesen, schreiben, abgleichen |
| `crates/core/src/platform/mod.rs` | Plattform-Trait |
| `crates/core/src/platform/unix.rs` | Linux-Implementierung |
| `crates/core/src/paths.rs` | Spiel-, Prefix- und Save-Verzeichnisse auflösen |
| `crates/core/src/library.rs` | Mod-Metadaten (`library.json`) |
| `crates/core/src/import.rs` | Archive entpacken, Paks importieren |
| `crates/core/src/saves.rs` | Backup, Wiederherstellung, Manifest |
| `crates/core/src/profile.rs` | Benannte Zusammenstellungen |
| `crates/core/src/launch.rs` | Spielstart, EAC-Bypass |
| `crates/app/src/main.rs` | Einstiegspunkt: Argumente vorhanden → CLI |
| `crates/app/src/cli.rs` | `clap`-Definition und Kommando-Ausführung |

---

## Task 0: T0 — Engine-Verhalten verifizieren

**Dies ist ein manueller Test, kein Code. Er blockiert alles Weitere.**

Die gesamte Architektur beruht darauf, dass die Engine `disabled: true` in `pak_config.yaml` befolgt. Die `readme.txt` im Mods-Ordner dokumentiert es, aber dokumentiert ist nicht verifiziert. Schlägt dieser Test fehl, ändert sich das Aktivierungsmodell (Risiko R1 der Spec) und die Tasks 2–4 müssen neu geschrieben werden.

- [ ] **Schritt 1: Aktuellen Zustand sichern**

```bash
GD="/home/carloskopra/SSD2000/SteamLibrary/steamapps/common/Space Marine 2"
cp "$GD/client_pc/root/mods/pak_config.yaml" ~/pak_config.yaml.bak
cat ~/pak_config.yaml.bak
```

Erwartet: `- pak: wa_astartes_14_1.pak`

- [ ] **Schritt 2: Mod auf deaktiviert setzen**

```bash
GD="/home/carloskopra/SSD2000/SteamLibrary/steamapps/common/Space Marine 2"
printf -- '- pak: wa_astartes_14_1.pak\n  disabled: true\n' > "$GD/client_pc/root/mods/pak_config.yaml"
```

- [ ] **Schritt 3: Spiel starten und prüfen**

Spiel über Steam starten. Prüfen, ob die Astartes-Modifikation sichtbar ist (Rüstungs-/Modelländerung im Charakterbildschirm).

Erwartet: Mod ist **nicht** aktiv → R1 entkräftet, Plan gilt unverändert.
Bei Misserfolg: Mod ist trotzdem aktiv → **hier stoppen**, Ergebnis melden. Der Plan wird auf das Umbenennungs-Modell (`mods/` ↔ `mods_disabled/` per `rename` auf demselben Dateisystem) angepasst.

- [ ] **Schritt 4: Ursprungszustand wiederherstellen**

```bash
GD="/home/carloskopra/SSD2000/SteamLibrary/steamapps/common/Space Marine 2"
cp ~/pak_config.yaml.bak "$GD/client_pc/root/mods/pak_config.yaml"
```

- [ ] **Schritt 5: Ergebnis in der Spec festhalten**

In `docs/superpowers/specs/2026-09-12-sm2-modloader-design.md`, Abschnitt 9, bei R1 ergänzen: `**Verifiziert am 2026-09-12: bestätigt.**` (oder das abweichende Ergebnis).

```bash
git add docs/superpowers/specs/2026-09-12-sm2-modloader-design.md
git commit -m "docs: T0-Ergebnis zu Risiko R1 festhalten"
```

---

## Task 1: Workspace, Fehlertypen, atomares Schreiben

**Files:**
- Create: `Cargo.toml`, `crates/core/Cargo.toml`, `crates/core/src/lib.rs`, `crates/core/src/error.rs`, `crates/core/src/atomic.rs`
- Test: `crates/core/src/atomic.rs` (Modul-Tests)

**Interfaces:**
- Consumes: nichts
- Produces:
  - `core::error::Error` (enum, `thiserror`), `core::error::Result<T> = std::result::Result<T, Error>`
  - `core::atomic::write_atomic(path: &Path, contents: &str) -> Result<()>`

- [ ] **Schritt 1: Workspace anlegen**

`Cargo.toml` im Projektwurzelverzeichnis:

```toml
[workspace]
members = ["crates/core", "crates/app"]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"
rust-version = "1.85"
license = "MIT"

[workspace.dependencies]
thiserror = "2.0"
anyhow = "1.0"
tracing = "0.1"
tempfile = "3.27"
blake3 = "1.8"
yaml-rust2 = "0.13"
steamlocate = "2.1"
zip = "8.6"
sevenz-rust2 = "0.22"
directories = "6.0"
clap = { version = "4.6", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.9"
```

`crates/core/Cargo.toml`:

```toml
[package]
name = "sm2-core"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
thiserror.workspace = true
tracing.workspace = true
tempfile.workspace = true
blake3.workspace = true
yaml-rust2.workspace = true
steamlocate.workspace = true
zip.workspace = true
sevenz-rust2.workspace = true
directories.workspace = true
serde.workspace = true
serde_json.workspace = true
toml.workspace = true
```

`crates/app` wird in Task 11 angelegt. Damit der Workspace bis dahin baut, jetzt schon ein Platzhalter-Binary:

`crates/app/Cargo.toml`:

```toml
[package]
name = "sm2-modloader"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
sm2-core = { path = "../core" }
anyhow.workspace = true
tracing.workspace = true
clap.workspace = true
```

`crates/app/src/main.rs`:

```rust
fn main() {
    println!("sm2-modloader");
}
```

- [ ] **Schritt 2: Fehlertypen schreiben**

`crates/core/src/error.rs`:

```rust
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
```

- [ ] **Schritt 3: Fehlgeschlagenen Test für atomares Schreiben schreiben**

`crates/core/src/atomic.rs`:

```rust
use crate::error::{Error, Result};
use std::io::Write;
use std::path::Path;

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
```

- [ ] **Schritt 4: Test laufen lassen, Fehlschlag bestätigen**

Run: `cargo test -p sm2-core atomic`
Expected: FAIL — `cannot find function 'write_atomic' in this scope`

- [ ] **Schritt 5: Implementierung schreiben**

In `crates/core/src/atomic.rs` oberhalb des Testmoduls:

```rust
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
```

- [ ] **Schritt 6: `lib.rs` anlegen**

`crates/core/src/lib.rs`:

```rust
pub mod atomic;
pub mod error;

pub use error::{Error, Result};

/// Steam-AppID von Warhammer 40.000: Space Marine 2.
pub const APP_ID: u32 = 2183900;
```

- [ ] **Schritt 7: Tests laufen lassen**

Run: `cargo test -p sm2-core`
Expected: PASS, 2 Tests

- [ ] **Schritt 8: Commit**

```bash
git add Cargo.toml crates/
git commit -m "feat(core): Workspace, Fehlertypen und atomares Schreiben"
```

---

## Task 2: `pak_config.yaml` lesen

**Files:**
- Create: `crates/core/src/pak_config.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `core::error::{Error, Result}`
- Produces:
  - `PakEntry { pak: String, disabled: bool }` — `Debug, Clone, PartialEq, Eq`
  - `PakConfig { entries: Vec<PakEntry> }` — `Debug, Clone, Default, PartialEq, Eq`
  - `PakConfig::parse(text: &str) -> Result<PakConfig>`
  - `PakConfig::enabled(&self) -> impl Iterator<Item = &PakEntry>`

Die Reihenfolge in `entries` **ist** die Ladereihenfolge. Sie darf nie umsortiert werden, außer der Nutzer verlangt es.

- [ ] **Schritt 1: API von `yaml-rust2` prüfen**

Die Crate ist jung; die genauen Typnamen müssen bestätigt werden, bevor Code darauf aufbaut.

```bash
cargo doc -p yaml-rust2 --no-deps --open
```

Zu bestätigen: `YamlLoader::load_from_str(&str) -> Result<Vec<Yaml>, ScanError>`, die Varianten `Yaml::Array`, `Yaml::Hash`, `Yaml::Null`, sowie `Yaml::as_str()` und `Yaml::as_bool()`. Weicht die API ab, Schritt 3 entsprechend anpassen — Signatur und Verhalten von `parse` bleiben unverändert.

- [ ] **Schritt 2: Fehlgeschlagene Tests schreiben**

`crates/core/src/pak_config.rs`:

```rust
use crate::error::{Error, Result};

#[cfg(test)]
mod tests {
    use super::*;

    fn eintrag(pak: &str, disabled: bool) -> PakEntry {
        PakEntry { pak: pak.to_string(), disabled }
    }

    #[test]
    fn liest_das_beispiel_aus_der_engine_readme() {
        let text = "- pak: mod_a.pak\n- pak: mod_b.pak\n  disabled: true\n";
        let cfg = PakConfig::parse(text).unwrap();
        assert_eq!(cfg.entries, vec![eintrag("mod_a.pak", false), eintrag("mod_b.pak", true)]);
    }

    #[test]
    fn liest_die_echte_config_des_referenzsystems() {
        let cfg = PakConfig::parse("- pak: wa_astartes_14_1.pak\n").unwrap();
        assert_eq!(cfg.entries, vec![eintrag("wa_astartes_14_1.pak", false)]);
    }

    #[test]
    fn behaelt_die_reihenfolge_bei() {
        let text = "- pak: z.pak\n- pak: a.pak\n- pak: m.pak\n";
        let cfg = PakConfig::parse(text).unwrap();
        let namen: Vec<&str> = cfg.entries.iter().map(|e| e.pak.as_str()).collect();
        assert_eq!(namen, vec!["z.pak", "a.pak", "m.pak"], "Reihenfolge ist die Ladereihenfolge");
    }

    #[test]
    fn leere_datei_ergibt_leere_konfiguration() {
        assert_eq!(PakConfig::parse("").unwrap(), PakConfig::default());
        assert_eq!(PakConfig::parse("\n\n").unwrap(), PakConfig::default());
        assert_eq!(PakConfig::parse("[]\n").unwrap(), PakConfig::default());
    }

    #[test]
    fn toleriert_kommentare_und_anfuehrungszeichen() {
        let text = "# von Hand bearbeitet\n- pak: \"mit leerzeichen.pak\"\n  disabled: false\n";
        let cfg = PakConfig::parse(text).unwrap();
        assert_eq!(cfg.entries, vec![eintrag("mit leerzeichen.pak", false)]);
    }

    #[test]
    fn weist_nicht_listenfoermige_wurzel_zurueck() {
        let fehler = PakConfig::parse("pak: a.pak\n").unwrap_err();
        assert!(matches!(fehler, Error::PakConfig(_)));
    }

    #[test]
    fn weist_eintrag_ohne_pak_schluessel_zurueck() {
        let fehler = PakConfig::parse("- disabled: true\n").unwrap_err();
        assert!(matches!(fehler, Error::PakConfig(_)));
    }

    #[test]
    fn enabled_filtert_deaktivierte_heraus() {
        let cfg = PakConfig::parse("- pak: a.pak\n- pak: b.pak\n  disabled: true\n- pak: c.pak\n").unwrap();
        let namen: Vec<&str> = cfg.enabled().map(|e| e.pak.as_str()).collect();
        assert_eq!(namen, vec!["a.pak", "c.pak"]);
    }
}
```

- [ ] **Schritt 3: Test laufen lassen, Fehlschlag bestätigen**

Run: `cargo test -p sm2-core pak_config`
Expected: FAIL — `cannot find type 'PakConfig' in this scope`

- [ ] **Schritt 4: Implementierung schreiben**

In `crates/core/src/pak_config.rs` oberhalb des Testmoduls:

```rust
use yaml_rust2::{Yaml, YamlLoader};

/// Ein Eintrag in pak_config.yaml.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PakEntry {
    pub pak: String,
    pub disabled: bool,
}

/// Der Inhalt von pak_config.yaml. Die Reihenfolge der Einträge ist die
/// Ladereihenfolge der Engine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PakConfig {
    pub entries: Vec<PakEntry>,
}

impl PakConfig {
    pub fn parse(text: &str) -> Result<Self> {
        if text.trim().is_empty() {
            return Ok(Self::default());
        }

        let docs = YamlLoader::load_from_str(text)
            .map_err(|e| Error::PakConfig(e.to_string()))?;

        let Some(doc) = docs.first() else {
            return Ok(Self::default());
        };

        let items = match doc {
            Yaml::Array(items) => items,
            Yaml::Null => return Ok(Self::default()),
            _ => return Err(Error::PakConfig(
                "die Wurzel muss eine Liste von Einträgen sein".into(),
            )),
        };

        let mut entries = Vec::with_capacity(items.len());
        for (i, item) in items.iter().enumerate() {
            let Yaml::Hash(map) = item else {
                return Err(Error::PakConfig(format!(
                    "Eintrag {} ist kein Objekt mit 'pak'-Schlüssel", i + 1
                )));
            };

            let pak = map
                .get(&Yaml::String("pak".into()))
                .and_then(Yaml::as_str)
                .ok_or_else(|| Error::PakConfig(format!(
                    "Eintrag {} hat keinen gültigen 'pak'-Schlüssel", i + 1
                )))?;

            let disabled = map
                .get(&Yaml::String("disabled".into()))
                .and_then(Yaml::as_bool)
                .unwrap_or(false);

            entries.push(PakEntry { pak: pak.to_string(), disabled });
        }

        Ok(Self { entries })
    }

    /// Die aktiven Einträge in Ladereihenfolge.
    pub fn enabled(&self) -> impl Iterator<Item = &PakEntry> {
        self.entries.iter().filter(|e| !e.disabled)
    }
}
```

- [ ] **Schritt 5: In `lib.rs` eintragen**

```rust
pub mod pak_config;
```

- [ ] **Schritt 6: Tests laufen lassen**

Run: `cargo test -p sm2-core pak_config`
Expected: PASS, 8 Tests

- [ ] **Schritt 7: Commit**

```bash
git add crates/core/src/pak_config.rs crates/core/src/lib.rs
git commit -m "feat(core): pak_config.yaml lesen"
```

---

## Task 3: `pak_config.yaml` schreiben

**Files:**
- Modify: `crates/core/src/pak_config.rs`

**Interfaces:**
- Consumes: `PakConfig`, `PakEntry` aus Task 2; `core::atomic::write_atomic`
- Produces:
  - `PakConfig::to_yaml(&self) -> String`
  - `PakConfig::load(path: &Path) -> Result<PakConfig>` — fehlende Datei ergibt leere Konfiguration
  - `PakConfig::save(&self, path: &Path) -> Result<()>`

Die Ausgabe wird von Hand erzeugt, nicht von einer Serialisierungsbibliothek. Das Format hat zwei Felder; eigene Ausgabe bedeutet byte-genaue Kontrolle über das, was die Engine liest.

- [ ] **Schritt 1: Fehlgeschlagene Tests schreiben**

In `crates/core/src/pak_config.rs`, im `mod tests`:

```rust
    #[test]
    fn schreibt_das_format_der_engine_readme() {
        let cfg = PakConfig {
            entries: vec![eintrag("mod_a.pak", false), eintrag("mod_b.pak", true)],
        };
        assert_eq!(cfg.to_yaml(), "- pak: mod_a.pak\n- pak: mod_b.pak\n  disabled: true\n");
    }

    #[test]
    fn laesst_disabled_bei_aktiven_eintraegen_weg() {
        let cfg = PakConfig { entries: vec![eintrag("a.pak", false)] };
        assert_eq!(cfg.to_yaml(), "- pak: a.pak\n");
    }

    #[test]
    fn schreibt_leere_liste_als_gueltiges_yaml() {
        // Eine leere Datei wäre YAML-Null, keine Liste. "[]" ist eindeutig.
        assert_eq!(PakConfig::default().to_yaml(), "[]\n");
    }

    #[test]
    fn setzt_namen_mit_sonderzeichen_in_anfuehrungszeichen() {
        let cfg = PakConfig { entries: vec![eintrag("mit: doppelpunkt.pak", false)] };
        assert_eq!(cfg.to_yaml(), "- pak: \"mit: doppelpunkt.pak\"\n");
    }

    #[test]
    fn round_trip_erhaelt_die_konfiguration() {
        let original = PakConfig {
            entries: vec![
                eintrag("z.pak", false),
                eintrag("mit: doppelpunkt.pak", true),
                eintrag("a.pak", false),
            ],
        };
        let wieder = PakConfig::parse(&original.to_yaml()).unwrap();
        assert_eq!(wieder, original);
    }

    #[test]
    fn load_bei_fehlender_datei_ergibt_leere_konfiguration() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = PakConfig::load(&dir.path().join("gibt_es_nicht.yaml")).unwrap();
        assert_eq!(cfg, PakConfig::default());
    }

    #[test]
    fn save_und_load_sind_zueinander_invers() {
        let dir = tempfile::tempdir().unwrap();
        let pfad = dir.path().join("pak_config.yaml");
        let cfg = PakConfig { entries: vec![eintrag("a.pak", true), eintrag("b.pak", false)] };

        cfg.save(&pfad).unwrap();

        assert_eq!(PakConfig::load(&pfad).unwrap(), cfg);
    }
```

- [ ] **Schritt 2: Test laufen lassen, Fehlschlag bestätigen**

Run: `cargo test -p sm2-core pak_config`
Expected: FAIL — `no method named 'to_yaml' found`

- [ ] **Schritt 3: Implementierung schreiben**

In `impl PakConfig` ergänzen, und oben `use std::path::Path;` sowie `use crate::atomic::write_atomic;` hinzufügen:

```rust
    /// Erzeugt den Dateiinhalt für die Engine.
    ///
    /// Von Hand geschrieben statt serialisiert: das Format hat zwei Felder,
    /// und byte-genaue Kontrolle ist hier wichtiger als Bequemlichkeit.
    pub fn to_yaml(&self) -> String {
        if self.entries.is_empty() {
            // Eine leere Datei parst als YAML-Null, nicht als leere Liste.
            return "[]\n".to_string();
        }

        let mut out = String::new();
        for entry in &self.entries {
            out.push_str("- pak: ");
            out.push_str(&quote_if_needed(&entry.pak));
            out.push('\n');
            if entry.disabled {
                out.push_str("  disabled: true\n");
            }
        }
        out
    }

    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        write_atomic(path, &self.to_yaml())
    }
```

Als freie Funktion im selben Modul:

```rust
/// Setzt einen Dateinamen in Anführungszeichen, wenn er sonst als anderes
/// YAML-Konstrukt gelesen würde.
fn quote_if_needed(name: &str) -> String {
    let braucht_quotes = name.is_empty()
        || name.contains(':')
        || name.contains('#')
        || name.contains('"')
        || name.contains('\'')
        || name.starts_with(['-', '?', '&', '*', '!', '|', '>', '%', '@', '`', '[', '{'])
        || name.trim() != name;

    if braucht_quotes {
        format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        name.to_string()
    }
}
```

- [ ] **Schritt 4: Tests laufen lassen**

Run: `cargo test -p sm2-core pak_config`
Expected: PASS, 15 Tests

- [ ] **Schritt 5: Commit**

```bash
git add crates/core/src/pak_config.rs
git commit -m "feat(core): pak_config.yaml byte-genau schreiben"
```

---

## Task 4: Abgleich zwischen Verzeichnis und Konfiguration

**Files:**
- Modify: `crates/core/src/pak_config.rs`

**Interfaces:**
- Consumes: `PakConfig`, `PakEntry`
- Produces:
  - `Reconciliation { added: Vec<String>, removed: Vec<String> }` — `Debug, Default, PartialEq, Eq`
  - `Reconciliation::is_empty(&self) -> bool`
  - `PakConfig::reconcile(&mut self, present: &[String]) -> Reconciliation`

**Warum das nötig ist:** Die Engine-readme legt fest, dass Paks, die *nicht* in der Konfiguration stehen, **zuerst** und in alphabetischer Reihenfolge geladen werden. Ein nicht aufgeführtes Pak lässt sich also weder deaktivieren noch einordnen. Deshalb muss jedes vorhandene Pak in der Konfiguration stehen.

- [ ] **Schritt 1: Fehlgeschlagene Tests schreiben**

In `mod tests`:

```rust
    fn namen(cfg: &PakConfig) -> Vec<&str> {
        cfg.entries.iter().map(|e| e.pak.as_str()).collect()
    }

    #[test]
    fn ergaenzt_unbekannte_paks_aktiv_am_ende() {
        // Nicht aufgeführte Paks lädt die Engine ohnehin – also aktiv aufnehmen,
        // damit sie steuerbar werden.
        let mut cfg = PakConfig { entries: vec![eintrag("a.pak", false)] };
        let ergebnis = cfg.reconcile(&["a.pak".into(), "neu.pak".into()]);

        assert_eq!(namen(&cfg), vec!["a.pak", "neu.pak"]);
        assert!(!cfg.entries[1].disabled);
        assert_eq!(ergebnis.added, vec!["neu.pak"]);
        assert!(ergebnis.removed.is_empty());
    }

    #[test]
    fn entfernt_eintraege_ohne_datei() {
        let mut cfg = PakConfig {
            entries: vec![eintrag("a.pak", false), eintrag("weg.pak", true)],
        };
        let ergebnis = cfg.reconcile(&["a.pak".into()]);

        assert_eq!(namen(&cfg), vec!["a.pak"]);
        assert_eq!(ergebnis.removed, vec!["weg.pak"]);
    }

    #[test]
    fn erhaelt_reihenfolge_und_zustand_vorhandener_eintraege() {
        let mut cfg = PakConfig {
            entries: vec![eintrag("z.pak", true), eintrag("a.pak", false)],
        };
        cfg.reconcile(&["a.pak".into(), "z.pak".into()]);

        assert_eq!(namen(&cfg), vec!["z.pak", "a.pak"], "Reihenfolge darf sich nicht ändern");
        assert!(cfg.entries[0].disabled, "Deaktivierung darf nicht verloren gehen");
    }

    #[test]
    fn ergaenzt_mehrere_neue_paks_alphabetisch() {
        let mut cfg = PakConfig::default();
        let ergebnis = cfg.reconcile(&["b.pak".into(), "a.pak".into()]);

        assert_eq!(namen(&cfg), vec!["a.pak", "b.pak"]);
        assert_eq!(ergebnis.added, vec!["a.pak", "b.pak"]);
    }

    #[test]
    fn abgleich_ohne_aenderung_meldet_nichts() {
        let mut cfg = PakConfig { entries: vec![eintrag("a.pak", false)] };
        let ergebnis = cfg.reconcile(&["a.pak".into()]);

        assert!(ergebnis.is_empty());
    }
```

- [ ] **Schritt 2: Test laufen lassen, Fehlschlag bestätigen**

Run: `cargo test -p sm2-core pak_config::tests::ergaenzt`
Expected: FAIL — `no method named 'reconcile' found`

- [ ] **Schritt 3: Implementierung schreiben**

```rust
/// Was ein Abgleich zwischen Verzeichnis und Konfiguration verändert hat.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Reconciliation {
    /// Paks, die im Verzeichnis lagen, aber nicht in der Konfiguration standen.
    pub added: Vec<String>,
    /// Einträge, deren Datei fehlt.
    pub removed: Vec<String>,
}

impl Reconciliation {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
}
```

In `impl PakConfig`:

```rust
    /// Bringt die Konfiguration mit dem tatsächlichen Verzeichnisinhalt in
    /// Einklang. Reihenfolge und Aktivierungszustand bestehender Einträge
    /// bleiben unangetastet.
    pub fn reconcile(&mut self, present: &[String]) -> Reconciliation {
        let vorhanden: std::collections::HashSet<&str> =
            present.iter().map(String::as_str).collect();

        let mut removed = Vec::new();
        self.entries.retain(|e| {
            if vorhanden.contains(e.pak.as_str()) {
                true
            } else {
                removed.push(e.pak.clone());
                false
            }
        });

        let bekannt: std::collections::HashSet<&str> =
            self.entries.iter().map(|e| e.pak.as_str()).collect();

        let mut added: Vec<String> = present
            .iter()
            .filter(|p| !bekannt.contains(p.as_str()))
            .cloned()
            .collect();
        added.sort();

        for pak in &added {
            self.entries.push(PakEntry { pak: pak.clone(), disabled: false });
        }

        Reconciliation { added, removed }
    }
```

- [ ] **Schritt 4: Tests laufen lassen**

Run: `cargo test -p sm2-core pak_config`
Expected: PASS, 20 Tests

- [ ] **Schritt 5: Commit**

```bash
git add crates/core/src/pak_config.rs
git commit -m "feat(core): Verzeichnis und pak_config abgleichen"
```

---

## Task 5: Plattform-Trait und Unix-Implementierung

**Files:**
- Create: `crates/core/src/platform/mod.rs`, `crates/core/src/platform/unix.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `core::error::{Error, Result}`, `core::APP_ID`
- Produces:
  - `trait Platform` mit `steam_roots()`, `user_profile_root(app_id, library)`, `launch_via_steam(app_id)`, `launch_direct(exe, env)`, `open_folder(path)`
  - `pub type Current = unix::Unix;` unter `cfg(unix)`

Dies ist die **einzige** plattformabhängige Fläche des Projekts. Alles andere ist neutral. `windows.rs` entsteht erst, wenn getestet werden kann (Spec 3.2).

- [ ] **Schritt 1: Fehlgeschlagenen Test schreiben**

`crates/core/src/platform/unix.rs`:

```rust
use super::Platform;
use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

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
```

- [ ] **Schritt 2: Test laufen lassen, Fehlschlag bestätigen**

Run: `cargo test -p sm2-core platform`
Expected: FAIL — `cannot find type 'Unix' in this scope`

- [ ] **Schritt 3: Trait schreiben**

`crates/core/src/platform/mod.rs`:

```rust
use crate::error::Result;
use std::path::{Path, PathBuf};

#[cfg(unix)]
pub mod unix;

/// Die gesamte plattformabhängige Fläche des Projekts.
///
/// Windows und Linux unterscheiden sich nicht in der Pfadlogik, sondern nur
/// in deren Wurzel: der Proton-Prefix ist ein alternatives `C:\`.
pub trait Platform {
    /// Orte, an denen eine Steam-Installation liegen kann.
    fn steam_roots() -> Vec<PathBuf>;

    /// Wurzel, unterhalb derer `AppData/Local/...` liegt.
    ///
    /// Linux: `<library>/steamapps/compatdata/<app_id>/pfx/drive_c/users/steamuser`
    /// Windows: `%USERPROFILE%`
    fn user_profile_root(app_id: u32, library: &Path) -> PathBuf;

    /// Startet das Spiel regulär über Steam.
    fn launch_via_steam(app_id: u32) -> Result<()>;

    /// Startet die Executable unter Umgehung von Steam (EAC-Bypass).
    fn launch_direct(exe: &Path, env: &[(&str, &str)]) -> Result<()>;

    /// Öffnet ein Verzeichnis im Dateimanager.
    fn open_folder(path: &Path) -> Result<()>;
}

#[cfg(unix)]
pub type Current = unix::Unix;
```

- [ ] **Schritt 4: Unix-Implementierung schreiben**

In `crates/core/src/platform/unix.rs` oberhalb des Testmoduls:

```rust
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
```

- [ ] **Schritt 5: In `lib.rs` eintragen**

```rust
pub mod platform;
```

- [ ] **Schritt 6: Tests laufen lassen**

Run: `cargo test -p sm2-core platform`
Expected: PASS, 2 Tests

- [ ] **Schritt 7: Commit**

```bash
git add crates/core/src/platform crates/core/src/lib.rs
git commit -m "feat(core): Plattform-Trait mit Unix-Implementierung"
```

---

## Task 6: Pfadauflösung

**Files:**
- Create: `crates/core/src/paths.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `Platform`, `Current`, `Error`, `Result`, `APP_ID`
- Produces:
  - `GamePaths { game_dir, library_dir }` — `Debug, Clone`
  - `GamePaths::from_game_dir(game_dir, library_dir) -> Result<GamePaths>` — validiert, testbar ohne Steam
  - `GamePaths::discover() -> Result<GamePaths>` — über `steamlocate`
  - `GamePaths::mods_dir(&self) -> PathBuf`
  - `GamePaths::pak_config_path(&self) -> PathBuf`
  - `GamePaths::executable(&self) -> PathBuf`
  - `GamePaths::save_dir(&self) -> Result<PathBuf>`
  - `GamePaths::list_paks(&self) -> Result<Vec<String>>`
  - `app_dirs() -> Result<AppDirs>` mit Feldern `config`, `data`, `state`

`discover()` ist nicht automatisiert testbar (es braucht eine echte Steam-Installation). Deshalb trägt `from_game_dir` die gesamte Logik, und `discover` findet nur das Verzeichnis.

- [ ] **Schritt 1: Fehlgeschlagene Tests mit Fixture schreiben**

`crates/core/src/paths.rs`:

```rust
use crate::error::{Error, Result};
use crate::platform::{Current, Platform};
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests {
    use super::*;

    /// Baut einen Verzeichnisbaum, der einer echten Installation entspricht.
    fn fixture() -> (tempfile::TempDir, GamePaths) {
        let tmp = tempfile::tempdir().unwrap();
        let library = tmp.path().join("SteamLibrary");
        let game = library.join("steamapps/common/Space Marine 2");

        std::fs::create_dir_all(game.join("client_pc/root/mods")).unwrap();
        std::fs::create_dir_all(game.join("client_pc/root/bin/pc")).unwrap();

        let saves = library
            .join("steamapps/compatdata/2183900/pfx/drive_c/users/steamuser")
            .join("AppData/Local/Saber/Space Marine 2/storage/steam/user/76561198412726373/Main");
        std::fs::create_dir_all(&saves).unwrap();

        let paths = GamePaths::from_game_dir(&game, &library).unwrap();
        (tmp, paths)
    }

    #[test]
    fn weist_verzeichnis_ohne_mods_ordner_zurueck() {
        let tmp = tempfile::tempdir().unwrap();
        let fehler = GamePaths::from_game_dir(tmp.path(), tmp.path()).unwrap_err();
        assert!(matches!(fehler, Error::NotAGameDir(_)));
    }

    #[test]
    fn findet_mods_verzeichnis_und_konfiguration() {
        let (_tmp, paths) = fixture();
        assert!(paths.mods_dir().ends_with("client_pc/root/mods"));
        assert!(paths.pak_config_path().ends_with("client_pc/root/mods/pak_config.yaml"));
    }

    #[test]
    fn loest_das_save_verzeichnis_im_proton_prefix_auf() {
        let (_tmp, paths) = fixture();
        let saves = paths.save_dir().unwrap();
        assert!(saves.ends_with("storage/steam/user/76561198412726373/Main"));
        assert!(saves.is_dir());
    }

    #[test]
    fn meldet_fehlenden_prefix_verstaendlich() {
        let tmp = tempfile::tempdir().unwrap();
        let library = tmp.path().join("SteamLibrary");
        let game = library.join("steamapps/common/Space Marine 2");
        std::fs::create_dir_all(game.join("client_pc/root/mods")).unwrap();

        let paths = GamePaths::from_game_dir(&game, &library).unwrap();
        assert!(matches!(paths.save_dir().unwrap_err(), Error::PrefixMissing(2183900)));
    }

    #[test]
    fn meldet_mehrere_steam_profile_statt_zu_raten() {
        let (tmp, paths) = fixture();
        let user = tmp
            .path()
            .join("SteamLibrary/steamapps/compatdata/2183900/pfx/drive_c/users/steamuser")
            .join("AppData/Local/Saber/Space Marine 2/storage/steam/user");
        std::fs::create_dir_all(user.join("76561198000000000/Main")).unwrap();

        assert!(matches!(paths.save_dir().unwrap_err(), Error::AmbiguousSaveUser(_)));
    }

    #[test]
    fn listet_nur_pak_dateien_alphabetisch() {
        let (_tmp, paths) = fixture();
        let mods = paths.mods_dir();
        std::fs::write(mods.join("z.pak"), b"x").unwrap();
        std::fs::write(mods.join("a.pak"), b"x").unwrap();
        std::fs::write(mods.join("readme.txt"), b"x").unwrap();
        std::fs::write(mods.join("pak_config.yaml"), b"[]").unwrap();

        assert_eq!(paths.list_paks().unwrap(), vec!["a.pak", "z.pak"]);
    }
}
```

- [ ] **Schritt 2: Test laufen lassen, Fehlschlag bestätigen**

Run: `cargo test -p sm2-core paths`
Expected: FAIL — `cannot find type 'GamePaths' in this scope`

- [ ] **Schritt 3: Implementierung schreiben**

Oberhalb des Testmoduls:

```rust
use crate::APP_ID;

/// Alle Pfade rund um eine Spielinstallation.
#[derive(Debug, Clone)]
pub struct GamePaths {
    pub game_dir: PathBuf,
    /// Die Steam-Bibliothek, in der das Spiel liegt. Der Proton-Prefix hängt
    /// daran, nicht am Spielverzeichnis.
    pub library_dir: PathBuf,
}

impl GamePaths {
    /// Prüft, ob `game_dir` tatsächlich eine Space-Marine-2-Installation ist.
    pub fn from_game_dir(game_dir: &Path, library_dir: &Path) -> Result<Self> {
        if !game_dir.join("client_pc/root/mods").is_dir() {
            return Err(Error::NotAGameDir(game_dir.to_path_buf()));
        }
        Ok(Self {
            game_dir: game_dir.to_path_buf(),
            library_dir: library_dir.to_path_buf(),
        })
    }

    /// Findet das Spiel über die Steam-Bibliotheken.
    pub fn discover() -> Result<Self> {
        let steam = steamlocate::SteamDir::locate().map_err(|_| Error::SteamNotFound)?;
        let (app, library) = steam
            .find_app(APP_ID)
            .map_err(|_| Error::GameNotFound(APP_ID))?
            .ok_or(Error::GameNotFound(APP_ID))?;

        let game_dir = library
            .path()
            .join("steamapps/common")
            .join(&app.install_dir);

        Self::from_game_dir(&game_dir, library.path())
    }

    pub fn mods_dir(&self) -> PathBuf {
        self.game_dir.join("client_pc/root/mods")
    }

    pub fn pak_config_path(&self) -> PathBuf {
        self.mods_dir().join("pak_config.yaml")
    }

    pub fn executable(&self) -> PathBuf {
        self.game_dir
            .join("client_pc/root/bin/pc")
            .join("Warhammer 40000 Space Marine 2 - Retail.exe")
    }

    /// Das Savegame-Verzeichnis im Proton-Prefix.
    ///
    /// Unterhalb von `AppData/Local` ist der Pfad mit Windows identisch –
    /// nur die Wurzel liefert die Plattform.
    pub fn save_dir(&self) -> Result<PathBuf> {
        let profil = Current::user_profile_root(APP_ID, &self.library_dir);
        if !profil.is_dir() {
            return Err(Error::PrefixMissing(APP_ID));
        }

        let user_root = profil
            .join("AppData/Local/Saber/Space Marine 2/storage/steam/user");

        let mut profile: Vec<String> = std::fs::read_dir(&user_root)
            .map_err(|_| Error::NoSaveUser(user_root.clone()))?
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        profile.sort();

        match profile.len() {
            0 => Err(Error::NoSaveUser(user_root)),
            1 => Ok(user_root.join(&profile[0]).join("Main")),
            _ => Err(Error::AmbiguousSaveUser(profile)),
        }
    }

    /// Alle `.pak`-Dateien im Mods-Verzeichnis, alphabetisch.
    pub fn list_paks(&self) -> Result<Vec<String>> {
        let dir = self.mods_dir();
        let mut paks: Vec<String> = std::fs::read_dir(&dir)
            .map_err(|e| Error::io(&dir, e))?
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().is_file())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.to_lowercase().ends_with(".pak"))
            .collect();
        paks.sort();
        Ok(paks)
    }
}

/// Die XDG-Verzeichnisse der Anwendung.
#[derive(Debug, Clone)]
pub struct AppDirs {
    pub config: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
}

pub fn app_dirs() -> Result<AppDirs> {
    let pd = directories::ProjectDirs::from("", "", "sm2-modloader")
        .ok_or_else(|| Error::PlainIo(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Basisverzeichnisse des Systems nicht ermittelbar",
        )))?;

    Ok(AppDirs {
        config: pd.config_dir().to_path_buf(),
        data: pd.data_dir().to_path_buf(),
        state: pd
            .state_dir()
            .unwrap_or_else(|| pd.data_dir())
            .to_path_buf(),
    })
}
```

- [ ] **Schritt 4: In `lib.rs` eintragen**

```rust
pub mod paths;
```

- [ ] **Schritt 5: Tests laufen lassen**

Run: `cargo test -p sm2-core paths`
Expected: PASS, 6 Tests

- [ ] **Schritt 6: `discover()` gegen die echte Installation prüfen**

`discover` braucht eine echte Steam-Installation und ist deshalb nicht automatisiert testbar. Ein Beispielprogramm macht es prüfbar.

`crates/core/examples/discover.rs` anlegen:

```rust
fn main() {
    match sm2_core::paths::GamePaths::discover() {
        Ok(p) => {
            println!("Spiel:  {}", p.game_dir.display());
            println!("Mods:   {}", p.mods_dir().display());
            match p.save_dir() {
                Ok(s) => println!("Saves:  {}", s.display()),
                Err(e) => println!("Saves:  {e}"),
            }
        }
        Err(e) => println!("Fehler: {e}"),
    }
}
```

Run: `cargo run -p sm2-core --example discover`

Expected: Spielpfad `…/SSD2000/SteamLibrary/steamapps/common/Space Marine 2`, Save-Pfad endend auf `76561198412726373/Main`.

- [ ] **Schritt 7: Commit**

```bash
git add crates/core/src/paths.rs crates/core/src/lib.rs crates/core/examples/discover.rs
git commit -m "feat(core): Spiel-, Prefix- und Save-Pfade auflösen"
```

---

## Task 7: Mod-Bibliothek

**Files:**
- Create: `crates/core/src/library.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `atomic::write_atomic`, `Error`, `Result`
- Produces:
  - `ModInfo { pak: String, name: String, author: Option<String>, version: Option<String>, nexus_id: Option<u32>, notes: Option<String>, hash: String, size: u64, imported_at: String, source: Option<String> }` — `Serialize, Deserialize, Debug, Clone, PartialEq`
  - `Library { mods: BTreeMap<String, ModInfo> }` — `Serialize, Deserialize, Debug, Clone, Default`
  - `Library::load(path: &Path) -> Result<Library>`
  - `Library::save(&self, path: &Path) -> Result<()>`
  - `Library::find_by_hash(&self, hash: &str) -> Option<&ModInfo>`
  - `hash_file(path: &Path) -> Result<String>`

Die Bibliothek enthält **nur Metadaten**. Die `.pak`-Dateien liegen ausschließlich im Spielverzeichnis.

- [ ] **Schritt 1: Fehlgeschlagene Tests schreiben**

`crates/core/src/library.rs`:

```rust
use crate::atomic::write_atomic;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;

    fn info(pak: &str, hash: &str) -> ModInfo {
        ModInfo {
            pak: pak.to_string(),
            name: pak.trim_end_matches(".pak").to_string(),
            author: None,
            version: None,
            nexus_id: None,
            notes: None,
            hash: hash.to_string(),
            size: 42,
            imported_at: "2026-09-12T18:00:00Z".to_string(),
            source: None,
        }
    }

    #[test]
    fn hash_ist_stabil_und_unterscheidet_inhalte() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.bin");
        let b = dir.path().join("b.bin");
        std::fs::write(&a, b"identisch").unwrap();
        std::fs::write(&b, b"identisch").unwrap();

        assert_eq!(hash_file(&a).unwrap(), hash_file(&b).unwrap());

        std::fs::write(&b, b"anders").unwrap();
        assert_ne!(hash_file(&a).unwrap(), hash_file(&b).unwrap());
    }

    #[test]
    fn load_bei_fehlender_datei_ergibt_leere_bibliothek() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::load(&dir.path().join("gibt_es_nicht.json")).unwrap();
        assert!(lib.mods.is_empty());
    }

    #[test]
    fn save_und_load_sind_zueinander_invers() {
        let dir = tempfile::tempdir().unwrap();
        let pfad = dir.path().join("library.json");
        let mut lib = Library::default();
        lib.mods.insert("a.pak".into(), info("a.pak", "hash-a"));

        lib.save(&pfad).unwrap();

        assert_eq!(Library::load(&pfad).unwrap().mods, lib.mods);
    }

    #[test]
    fn findet_dublette_ueber_den_hash() {
        let mut lib = Library::default();
        lib.mods.insert("a.pak".into(), info("a.pak", "hash-a"));

        assert_eq!(lib.find_by_hash("hash-a").map(|m| m.pak.as_str()), Some("a.pak"));
        assert!(lib.find_by_hash("unbekannt").is_none());
    }

    #[test]
    fn beschaedigte_json_datei_wird_als_fehler_gemeldet() {
        let dir = tempfile::tempdir().unwrap();
        let pfad = dir.path().join("library.json");
        std::fs::write(&pfad, "{kein json").unwrap();

        assert!(Library::load(&pfad).is_err());
    }
}
```

- [ ] **Schritt 2: Test laufen lassen, Fehlschlag bestätigen**

Run: `cargo test -p sm2-core library`
Expected: FAIL — `cannot find type 'ModInfo' in this scope`

- [ ] **Schritt 3: Implementierung schreiben**

```rust
/// Metadaten zu einem importierten Mod. Die Pak-Datei selbst liegt im
/// Spielverzeichnis; hier steht nur, was wir darüber wissen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModInfo {
    pub pak: String,
    pub name: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub nexus_id: Option<u32>,
    #[serde(default)]
    pub notes: Option<String>,
    /// blake3 des Pak-Inhalts – erkennt Dubletten und Änderungen von außen.
    pub hash: String,
    pub size: u64,
    /// RFC-3339-Zeitstempel.
    pub imported_at: String,
    /// Pfad des Archivs oder der Datei, aus der importiert wurde.
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Library {
    /// Schlüssel ist der Pak-Dateiname.
    #[serde(default)]
    pub mods: BTreeMap<String, ModInfo>,
}

impl Library {
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| {
                Error::io(path, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(|e| {
            Error::io(path, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        write_atomic(path, &format!("{json}\n"))
    }

    pub fn find_by_hash(&self, hash: &str) -> Option<&ModInfo> {
        self.mods.values().find(|m| m.hash == hash)
    }
}

/// blake3-Hash einer Datei, streamend gelesen – Paks können Gigabytes groß sein.
pub fn hash_file(path: &Path) -> Result<String> {
    let mut datei = std::fs::File::open(path).map_err(|e| Error::io(path, e))?;
    let mut hasher = blake3::Hasher::new();
    std::io::copy(&mut datei, &mut hasher).map_err(|e| Error::io(path, e))?;
    Ok(hasher.finalize().to_hex().to_string())
}
```

- [ ] **Schritt 4: In `lib.rs` eintragen**

```rust
pub mod library;
```

- [ ] **Schritt 5: Tests laufen lassen**

Run: `cargo test -p sm2-core library`
Expected: PASS, 5 Tests

- [ ] **Schritt 6: Commit**

```bash
git add crates/core/src/library.rs crates/core/src/lib.rs
git commit -m "feat(core): Mod-Bibliothek mit Hash-basierter Dublettenerkennung"
```

---

## Task 8: Import aus Archiven

**Files:**
- Create: `crates/core/src/import.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `GamePaths`, `Library`, `ModInfo`, `hash_file`, `PakConfig`, `PakEntry`, `Error`, `Result`
- Produces:
  - `ImportOutcome { pak: String, duplicate_of: Option<String> }` — `Debug, PartialEq`
  - `extract_paks(archive: &Path, into: &Path) -> Result<Vec<PathBuf>>`
  - `import_pak(paths: &GamePaths, lib: &mut Library, cfg: &mut PakConfig, pak: &Path, source: Option<&str>) -> Result<ImportOutcome>`

Ein Import fügt den Mod **deaktiviert ans Ende** der Konfiguration ein — ein Import verändert nie ein laufendes Setup.

- [ ] **Schritt 1: Fehlgeschlagene Tests schreiben**

`crates/core/src/import.rs`:

```rust
use crate::error::{Error, Result};
use crate::library::{hash_file, Library, ModInfo};
use crate::pak_config::{PakConfig, PakEntry};
use crate::paths::GamePaths;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn spiel_fixture() -> (tempfile::TempDir, GamePaths) {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("Space Marine 2");
        std::fs::create_dir_all(game.join("client_pc/root/mods")).unwrap();
        let paths = GamePaths::from_game_dir(&game, tmp.path()).unwrap();
        (tmp, paths)
    }

    fn zip_mit(dateien: &[(&str, &[u8])], nach: &Path) {
        let datei = std::fs::File::create(nach).unwrap();
        let mut zip = zip::ZipWriter::new(datei);
        let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        for (name, inhalt) in dateien {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(inhalt).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn entpackt_paks_aus_zip_und_ignoriert_beiwerk() {
        let tmp = tempfile::tempdir().unwrap();
        let archiv = tmp.path().join("mod.zip");
        zip_mit(&[("readme.txt", b"hallo"), ("cool_mod.pak", b"PAKDATEN")], &archiv);

        let ziel = tmp.path().join("out");
        std::fs::create_dir_all(&ziel).unwrap();
        let paks = extract_paks(&archiv, &ziel).unwrap();

        assert_eq!(paks.len(), 1);
        assert!(paks[0].ends_with("cool_mod.pak"));
        assert_eq!(std::fs::read(&paks[0]).unwrap(), b"PAKDATEN");
    }

    #[test]
    fn entpackt_paks_aus_unterverzeichnissen() {
        let tmp = tempfile::tempdir().unwrap();
        let archiv = tmp.path().join("mod.zip");
        zip_mit(&[("Mein Mod/v2/tief.pak", b"DATEN")], &archiv);

        let ziel = tmp.path().join("out");
        std::fs::create_dir_all(&ziel).unwrap();
        let paks = extract_paks(&archiv, &ziel).unwrap();

        assert_eq!(paks.len(), 1);
        assert!(paks[0].ends_with("tief.pak"), "Verzeichnisstruktur wird abgeflacht");
    }

    #[test]
    fn zip_ohne_pak_wird_klar_gemeldet() {
        let tmp = tempfile::tempdir().unwrap();
        let archiv = tmp.path().join("leer.zip");
        zip_mit(&[("readme.txt", b"nichts")], &archiv);

        let ziel = tmp.path().join("out");
        std::fs::create_dir_all(&ziel).unwrap();

        assert!(matches!(
            extract_paks(&archiv, &ziel).unwrap_err(),
            Error::NoPakInArchive(_)
        ));
    }

    #[test]
    fn zip_slip_pfade_werden_abgewehrt() {
        // Ein Archiv darf niemals außerhalb des Zielverzeichnisses schreiben.
        let tmp = tempfile::tempdir().unwrap();
        let archiv = tmp.path().join("boese.zip");
        zip_mit(&[("../../entkommen.pak", b"DATEN")], &archiv);

        let ziel = tmp.path().join("out");
        std::fs::create_dir_all(&ziel).unwrap();
        let paks = extract_paks(&archiv, &ziel).unwrap();

        for p in &paks {
            assert!(p.starts_with(&ziel), "Datei landete außerhalb: {}", p.display());
        }
        assert!(!tmp.path().join("entkommen.pak").exists());
    }

    #[test]
    fn import_legt_pak_ab_und_traegt_es_deaktiviert_ein() {
        let (_tmp, paths) = spiel_fixture();
        let quelle = tempfile::tempdir().unwrap();
        let pak = quelle.path().join("neu.pak");
        std::fs::write(&pak, b"PAKDATEN").unwrap();

        let mut lib = Library::default();
        let mut cfg = PakConfig { entries: vec![PakEntry { pak: "alt.pak".into(), disabled: false }] };

        let ergebnis = import_pak(&paths, &mut lib, &mut cfg, &pak, Some("mod.zip")).unwrap();

        assert_eq!(ergebnis, ImportOutcome { pak: "neu.pak".into(), duplicate_of: None });
        assert!(paths.mods_dir().join("neu.pak").is_file());
        assert_eq!(cfg.entries.last().unwrap().pak, "neu.pak");
        assert!(cfg.entries.last().unwrap().disabled, "Import darf nichts aktivieren");
        assert_eq!(cfg.entries[0].pak, "alt.pak", "bestehende Einträge bleiben vorn");
        assert_eq!(lib.mods["neu.pak"].source.as_deref(), Some("mod.zip"));
    }

    #[test]
    fn import_erkennt_inhaltsgleiche_dublette() {
        let (_tmp, paths) = spiel_fixture();
        let quelle = tempfile::tempdir().unwrap();
        let erste = quelle.path().join("erst.pak");
        let zweite = quelle.path().join("nochmal.pak");
        std::fs::write(&erste, b"GLEICHER INHALT").unwrap();
        std::fs::write(&zweite, b"GLEICHER INHALT").unwrap();

        let mut lib = Library::default();
        let mut cfg = PakConfig::default();

        import_pak(&paths, &mut lib, &mut cfg, &erste, None).unwrap();
        let ergebnis = import_pak(&paths, &mut lib, &mut cfg, &zweite, None).unwrap();

        assert_eq!(ergebnis.duplicate_of.as_deref(), Some("erst.pak"));
        assert_eq!(cfg.entries.len(), 1, "Dublette wird nicht erneut eingetragen");
    }
}
```

- [ ] **Schritt 2: Test laufen lassen, Fehlschlag bestätigen**

Run: `cargo test -p sm2-core import`
Expected: FAIL — `cannot find function 'extract_paks' in this scope`

- [ ] **Schritt 3: Entpacken implementieren**

```rust
/// Ergebnis eines Imports.
#[derive(Debug, PartialEq)]
pub struct ImportOutcome {
    pub pak: String,
    /// Gesetzt, wenn ein inhaltsgleicher Mod bereits vorhanden war.
    pub duplicate_of: Option<String>,
}

/// Holt alle `.pak`-Dateien aus einem Archiv nach `into`.
///
/// Die Verzeichnisstruktur wird abgeflacht – Mod-Archive verpacken Paks
/// gern in Ordner, für die Engine zählt nur die Datei.
pub fn extract_paks(archive: &Path, into: &Path) -> Result<Vec<PathBuf>> {
    let endung = archive
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_lowercase();

    let paks = match endung.as_str() {
        "pak" => vec![archive.to_path_buf()],
        "zip" => extract_zip(archive, into)?,
        "7z" => extract_7z(archive, into)?,
        "rar" => extract_rar(archive, into)?,
        _ => return Err(Error::NoPakInArchive(archive.to_path_buf())),
    };

    if paks.is_empty() {
        return Err(Error::NoPakInArchive(archive.to_path_buf()));
    }
    Ok(paks)
}

fn ist_pak(name: &str) -> bool {
    name.to_lowercase().ends_with(".pak")
}

/// Nimmt nur den Dateinamen – wehrt zugleich Zip-Slip ab, da kein
/// Verzeichnisanteil des Archivs übernommen wird.
fn ziel_fuer(into: &Path, eintrag_name: &str) -> Option<PathBuf> {
    let datei = Path::new(eintrag_name).file_name()?;
    Some(into.join(datei))
}

fn extract_zip(archive: &Path, into: &Path) -> Result<Vec<PathBuf>> {
    let datei = std::fs::File::open(archive).map_err(|e| Error::io(archive, e))?;
    let mut zip = zip::ZipArchive::new(datei)
        .map_err(|e| Error::io(archive, std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;

    let mut ergebnis = Vec::new();
    for i in 0..zip.len() {
        let mut eintrag = zip
            .by_index(i)
            .map_err(|e| Error::io(archive, std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;
        if !eintrag.is_file() || !ist_pak(eintrag.name()) {
            continue;
        }
        let Some(ziel) = ziel_fuer(into, eintrag.name()) else { continue };
        let mut aus = std::fs::File::create(&ziel).map_err(|e| Error::io(&ziel, e))?;
        std::io::copy(&mut eintrag, &mut aus).map_err(|e| Error::io(&ziel, e))?;
        ergebnis.push(ziel);
    }
    Ok(ergebnis)
}

fn extract_7z(archive: &Path, into: &Path) -> Result<Vec<PathBuf>> {
    let roh = into.join("__7z");
    std::fs::create_dir_all(&roh).map_err(|e| Error::io(&roh, e))?;
    sevenz_rust2::decompress_file(archive, &roh)
        .map_err(|e| Error::io(archive, std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;
    sammle_paks_rekursiv(&roh, into)
}

/// `.rar` über ein externes Werkzeug – die `unrar`-Crate bindet unfreien
/// Quellcode ein und ist für eine Veröffentlichung nicht tragbar.
fn extract_rar(archive: &Path, into: &Path) -> Result<Vec<PathBuf>> {
    let roh = into.join("__rar");
    std::fs::create_dir_all(&roh).map_err(|e| Error::io(&roh, e))?;

    let werkzeuge: [(&str, Vec<&str>); 3] = [
        ("unar", vec!["-quiet", "-force-overwrite", "-output-directory"]),
        ("7z", vec!["x", "-y"]),
        ("7zz", vec!["x", "-y"]),
    ];

    for (name, args) in &werkzeuge {
        let Some(pfad) = finde_im_path(name) else { continue };
        let mut cmd = std::process::Command::new(pfad);
        if *name == "unar" {
            cmd.args(args).arg(&roh).arg(archive);
        } else {
            cmd.args(args).arg(format!("-o{}", roh.display())).arg(archive);
        }
        let status = cmd.status().map_err(|e| Error::io(archive, e))?;
        if status.success() {
            return sammle_paks_rekursiv(&roh, into);
        }
    }

    Err(Error::NoRarTool)
}

fn finde_im_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|k| k.is_file())
}

/// Sucht `.pak`-Dateien in einem entpackten Baum und legt sie flach in `into`.
fn sammle_paks_rekursiv(von: &Path, into: &Path) -> Result<Vec<PathBuf>> {
    let mut ergebnis = Vec::new();
    let mut offen = vec![von.to_path_buf()];

    while let Some(dir) = offen.pop() {
        for eintrag in std::fs::read_dir(&dir).map_err(|e| Error::io(&dir, e))? {
            let eintrag = eintrag.map_err(|e| Error::io(&dir, e))?;
            let pfad = eintrag.path();
            if pfad.is_dir() {
                offen.push(pfad);
            } else if pfad.file_name().and_then(|n| n.to_str()).is_some_and(ist_pak) {
                let ziel = into.join(pfad.file_name().unwrap());
                if pfad != ziel {
                    std::fs::rename(&pfad, &ziel).map_err(|e| Error::io(&ziel, e))?;
                }
                ergebnis.push(ziel);
            }
        }
    }
    ergebnis.sort();
    Ok(ergebnis)
}
```

- [ ] **Schritt 4: Import implementieren**

```rust
/// Legt ein Pak im Spielverzeichnis ab und trägt es deaktiviert ein.
pub fn import_pak(
    paths: &GamePaths,
    lib: &mut Library,
    cfg: &mut PakConfig,
    pak: &Path,
    source: Option<&str>,
) -> Result<ImportOutcome> {
    let name = pak
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::NoPakInArchive(pak.to_path_buf()))?
        .to_string();

    let hash = hash_file(pak)?;
    if let Some(vorhanden) = lib.find_by_hash(&hash) {
        return Ok(ImportOutcome { pak: name, duplicate_of: Some(vorhanden.pak.clone()) });
    }

    let groesse = std::fs::metadata(pak).map_err(|e| Error::io(pak, e))?.len();
    let ziel = paths.mods_dir().join(&name);

    // rename schlägt über Gerätegrenzen fehl – dann kopieren.
    if std::fs::rename(pak, &ziel).is_err() {
        std::fs::copy(pak, &ziel).map_err(|e| Error::io(&ziel, e))?;
        let _ = std::fs::remove_file(pak);
    }

    lib.mods.insert(
        name.clone(),
        ModInfo {
            pak: name.clone(),
            name: name.trim_end_matches(".pak").replace(['_', '-'], " "),
            author: None,
            version: None,
            nexus_id: None,
            notes: None,
            hash,
            size: groesse,
            imported_at: jetzt_rfc3339(),
            source: source.map(str::to_string),
        },
    );

    if !cfg.entries.iter().any(|e| e.pak == name) {
        cfg.entries.push(PakEntry { pak: name.clone(), disabled: true });
    }

    Ok(ImportOutcome { pak: name, duplicate_of: None })
}

/// RFC-3339-Zeitstempel in UTC, ohne Datums-Crate.
pub(crate) fn jetzt_rfc3339() -> String {
    let sekunden = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    format_utc(sekunden)
}

pub(crate) fn format_utc(unix: i64) -> String {
    // Tage seit Epoche in ein Kalenderdatum umrechnen (Howard Hinnants Algorithmus).
    let tage = unix.div_euclid(86_400);
    let rest = unix.rem_euclid(86_400);
    let z = tage + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let jahr = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        jahr, m, d, rest / 3600, (rest % 3600) / 60, rest % 60
    )
}
```

- [ ] **Schritt 5: Test für die Datumsformatierung ergänzen**

In `mod tests`:

```rust
    #[test]
    fn formatiert_unix_zeit_als_rfc3339() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(1_757_700_000), "2025-09-12T18:00:00Z");
        // Schaltjahr
        assert_eq!(format_utc(1_709_164_800), "2024-02-29T00:00:00Z");
    }
```

- [ ] **Schritt 6: In `lib.rs` eintragen**

```rust
pub mod import;
```

- [ ] **Schritt 7: Tests laufen lassen**

Run: `cargo test -p sm2-core import`
Expected: PASS, 7 Tests

- [ ] **Schritt 8: Commit**

```bash
git add crates/core/src/import.rs crates/core/src/lib.rs
git commit -m "feat(core): Mod-Import aus pak, zip, 7z und rar"
```

---

## Task 9: Savegames sichern und wiederherstellen

**Files:**
- Create: `crates/core/src/saves.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `import::{jetzt_rfc3339}`, `atomic::write_atomic`, `Error`, `Result`
- Produces:
  - `BackupManifest { created_at: String, source: String, files: BTreeMap<String, FileRecord> }` — `Serialize, Deserialize, Debug, Clone, PartialEq`
  - `FileRecord { hash: String, size: u64 }` — `Serialize, Deserialize, Debug, Clone, PartialEq`
  - `BackupEntry { archive: PathBuf, manifest: PathBuf, created_at: String, label: Option<String> }` — `Debug, Clone`
  - `backup(save_dir: &Path, backup_root: &Path, label: Option<&str>) -> Result<BackupEntry>`
  - `list_backups(backup_root: &Path) -> Result<Vec<BackupEntry>>` — neueste zuerst
  - `verify(entry: &BackupEntry) -> Result<()>`
  - `restore(entry: &BackupEntry, save_dir: &Path, backup_root: &Path) -> Result<BackupEntry>` — legt **immer** zuerst ein Sicherungs-Backup an und gibt es zurück
  - `steam_is_running() -> bool`

**Sicherheitsregel:** `restore` überschreibt nie, ohne vorher den aktuellen Stand gesichert zu haben. Das kostet nichts und verhindert den einzigen wirklich schmerzhaften Fehler.

- [ ] **Schritt 1: Fehlgeschlagene Tests schreiben**

`crates/core/src/saves.rs`:

```rust
use crate::atomic::write_atomic;
use crate::error::{Error, Result};
use crate::import::jetzt_rfc3339;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests {
    use super::*;

    fn save_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let saves = tmp.path().join("Main");
        std::fs::create_dir_all(saves.join("slot1")).unwrap();
        std::fs::write(saves.join("profile.sav"), b"PROFILDATEN").unwrap();
        std::fs::write(saves.join("slot1/campaign.sav"), b"KAMPAGNE").unwrap();
        let backups = tmp.path().join("backups");
        (tmp, saves, backups)
    }

    #[test]
    fn backup_erzeugt_archiv_und_manifest() {
        let (_tmp, saves, backups) = save_fixture();
        let eintrag = backup(&saves, &backups, Some("vor Chaplain")).unwrap();

        assert!(eintrag.archive.is_file());
        assert!(eintrag.manifest.is_file());
        assert_eq!(eintrag.label.as_deref(), Some("vor Chaplain"));

        let manifest: BackupManifest =
            serde_json::from_str(&std::fs::read_to_string(&eintrag.manifest).unwrap()).unwrap();
        assert_eq!(manifest.files.len(), 2);
        assert!(manifest.files.contains_key("profile.sav"));
        assert!(manifest.files.contains_key("slot1/campaign.sav"));
    }

    #[test]
    fn verify_akzeptiert_unversehrtes_backup() {
        let (_tmp, saves, backups) = save_fixture();
        let eintrag = backup(&saves, &backups, None).unwrap();
        verify(&eintrag).unwrap();
    }

    #[test]
    fn verify_weist_manipuliertes_archiv_zurueck() {
        let (_tmp, saves, backups) = save_fixture();
        let eintrag = backup(&saves, &backups, None).unwrap();
        std::fs::write(&eintrag.archive, b"kaputt").unwrap();

        assert!(matches!(verify(&eintrag).unwrap_err(), Error::CorruptBackup(_)));
    }

    #[test]
    fn restore_stellt_inhalt_bit_genau_wieder_her() {
        let (_tmp, saves, backups) = save_fixture();
        let eintrag = backup(&saves, &backups, None).unwrap();

        std::fs::write(saves.join("profile.sav"), b"KAPUTTGESPIELT").unwrap();
        std::fs::remove_file(saves.join("slot1/campaign.sav")).unwrap();

        restore(&eintrag, &saves, &backups).unwrap();

        assert_eq!(std::fs::read(saves.join("profile.sav")).unwrap(), b"PROFILDATEN");
        assert_eq!(std::fs::read(saves.join("slot1/campaign.sav")).unwrap(), b"KAMPAGNE");
    }

    #[test]
    fn restore_sichert_den_aktuellen_stand_vorher_immer() {
        let (_tmp, saves, backups) = save_fixture();
        let eintrag = backup(&saves, &backups, None).unwrap();
        std::fs::write(saves.join("profile.sav"), b"NEUER FORTSCHRITT").unwrap();

        let sicherung = restore(&eintrag, &saves, &backups).unwrap();

        verify(&sicherung).unwrap();
        assert_eq!(sicherung.label.as_deref(), Some("vor Wiederherstellung"));

        // Der überschriebene Fortschritt ist aus der Sicherung wiederholbar.
        restore(&sicherung, &saves, &backups).unwrap();
        assert_eq!(std::fs::read(saves.join("profile.sav")).unwrap(), b"NEUER FORTSCHRITT");
    }

    #[test]
    fn zwei_backups_in_derselben_sekunde_ueberschreiben_sich_nicht() {
        let (_tmp, saves, backups) = save_fixture();

        let erst = backup(&saves, &backups, Some("gleich")).unwrap();
        std::fs::write(saves.join("profile.sav"), b"SPAETER").unwrap();
        let zweit = backup(&saves, &backups, Some("gleich")).unwrap();

        assert_ne!(erst.archive, zweit.archive, "Namenskollision innerhalb einer Sekunde");
        verify(&erst).unwrap();
        verify(&zweit).unwrap();
    }

    #[test]
    fn zweimaliges_wiederherstellen_zerstoert_kein_archiv() {
        // restore() sichert vor dem Lesen – die Sicherung darf das zu lesende
        // Archiv niemals überschreiben.
        let (_tmp, saves, backups) = save_fixture();
        let original = backup(&saves, &backups, None).unwrap();

        std::fs::write(saves.join("profile.sav"), b"ZWISCHENSTAND").unwrap();
        let sicherung = restore(&original, &saves, &backups).unwrap();
        assert_eq!(std::fs::read(saves.join("profile.sav")).unwrap(), b"PROFILDATEN");

        restore(&sicherung, &saves, &backups).unwrap();
        assert_eq!(std::fs::read(saves.join("profile.sav")).unwrap(), b"ZWISCHENSTAND");
    }

    #[test]
    fn list_backups_liefert_neueste_zuerst() {
        let (_tmp, saves, backups) = save_fixture();
        let erst = backup(&saves, &backups, Some("a")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let zweit = backup(&saves, &backups, Some("b")).unwrap();

        let liste = list_backups(&backups).unwrap();
        assert_eq!(liste.len(), 2);
        assert_eq!(liste[0].archive, zweit.archive);
        assert_eq!(liste[1].archive, erst.archive);
    }
}
```

- [ ] **Schritt 2: Test laufen lassen, Fehlschlag bestätigen**

Run: `cargo test -p sm2-core saves`
Expected: FAIL — `cannot find function 'backup' in this scope`

- [ ] **Schritt 3: Manifest und Backup implementieren**

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileRecord {
    pub hash: String,
    pub size: u64,
}

/// Begleitet jedes Backup und macht Beschädigung erkennbar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackupManifest {
    pub created_at: String,
    pub source: String,
    #[serde(default)]
    pub label: Option<String>,
    /// Schlüssel ist der Pfad relativ zum Save-Verzeichnis, mit '/' getrennt.
    pub files: BTreeMap<String, FileRecord>,
}

#[derive(Debug, Clone)]
pub struct BackupEntry {
    pub archive: PathBuf,
    pub manifest: PathBuf,
    pub created_at: String,
    pub label: Option<String>,
}

fn dateien_rekursiv(wurzel: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut ergebnis = Vec::new();
    let mut offen = vec![wurzel.to_path_buf()];

    while let Some(dir) = offen.pop() {
        for eintrag in std::fs::read_dir(&dir).map_err(|e| Error::io(&dir, e))? {
            let pfad = eintrag.map_err(|e| Error::io(&dir, e))?.path();
            if pfad.is_dir() {
                offen.push(pfad);
            } else if pfad.is_file() {
                let rel = pfad
                    .strip_prefix(wurzel)
                    .map_err(|_| Error::CorruptBackup("Pfad außerhalb des Save-Verzeichnisses".into()))?
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                ergebnis.push((rel, pfad));
            }
        }
    }
    ergebnis.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(ergebnis)
}

/// Zeitstempel, der sich als Dateiname eignet: 2026-09-12_180000
fn zeitstempel_fuer_dateinamen(rfc: &str) -> String {
    rfc.trim_end_matches('Z').replace(':', "").replace('T', "_")
}

/// Findet ein noch unbelegtes Paar aus Archiv- und Manifestnamen.
fn freier_name(backup_root: &Path, basis: &str) -> (PathBuf, PathBuf) {
    let mut versuch = 0u32;
    loop {
        let name = if versuch == 0 {
            basis.to_string()
        } else {
            format!("{basis}-{versuch}")
        };
        let archiv = backup_root.join(format!("{name}.zip"));
        let manifest = backup_root.join(format!("{name}.json"));
        if !archiv.exists() && !manifest.exists() {
            return (archiv, manifest);
        }
        versuch += 1;
    }
}

fn etikett_saeubern(label: &str) -> String {
    label
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

pub fn backup(save_dir: &Path, backup_root: &Path, label: Option<&str>) -> Result<BackupEntry> {
    if !save_dir.is_dir() {
        return Err(Error::io(
            save_dir,
            std::io::Error::new(std::io::ErrorKind::NotFound, "Save-Verzeichnis fehlt"),
        ));
    }
    std::fs::create_dir_all(backup_root).map_err(|e| Error::io(backup_root, e))?;

    let jetzt = jetzt_rfc3339();
    let mut basis = zeitstempel_fuer_dateinamen(&jetzt);
    if let Some(l) = label {
        let sauber = etikett_saeubern(l);
        if !sauber.is_empty() {
            basis = format!("{basis}_{sauber}");
        }
    }

    // Der Zeitstempel hat Sekundenauflösung. Zwei Backups in derselben Sekunde
    // dürfen einander nicht überschreiben – restore() legt unmittelbar vor dem
    // Lesen eines Archivs eine Sicherung an und würde sonst genau das Archiv
    // zerstören, das es gleich einliest.
    let (archiv_pfad, manifest_pfad) = freier_name(backup_root, &basis);

    let dateien = dateien_rekursiv(save_dir)?;
    let mut records = BTreeMap::new();

    let datei = std::fs::File::create(&archiv_pfad).map_err(|e| Error::io(&archiv_pfad, e))?;
    let mut zip = zip::ZipWriter::new(datei);
    let opts: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    for (rel, absolut) in &dateien {
        let inhalt = std::fs::read(absolut).map_err(|e| Error::io(absolut, e))?;
        zip.start_file(rel, opts).map_err(|e| {
            Error::io(&archiv_pfad, std::io::Error::new(std::io::ErrorKind::Other, e))
        })?;
        use std::io::Write;
        zip.write_all(&inhalt).map_err(|e| Error::io(&archiv_pfad, e))?;

        records.insert(
            rel.clone(),
            FileRecord {
                hash: blake3::hash(&inhalt).to_hex().to_string(),
                size: inhalt.len() as u64,
            },
        );
    }
    zip.finish().map_err(|e| {
        Error::io(&archiv_pfad, std::io::Error::new(std::io::ErrorKind::Other, e))
    })?;

    let manifest = BackupManifest {
        created_at: jetzt.clone(),
        source: save_dir.display().to_string(),
        label: label.map(str::to_string),
        files: records,
    };
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| {
        Error::io(&manifest_pfad, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    })?;
    write_atomic(&manifest_pfad, &format!("{json}\n"))?;

    Ok(BackupEntry {
        archive: archiv_pfad,
        manifest: manifest_pfad,
        created_at: jetzt,
        label: label.map(str::to_string),
    })
}
```

- [ ] **Schritt 4: Prüfung, Auflistung und Wiederherstellung implementieren**

```rust
fn manifest_lesen(entry: &BackupEntry) -> Result<BackupManifest> {
    let text = std::fs::read_to_string(&entry.manifest)
        .map_err(|e| Error::io(&entry.manifest, e))?;
    serde_json::from_str(&text)
        .map_err(|e| Error::CorruptBackup(format!("Manifest unlesbar: {e}")))
}

/// Prüft jede Datei im Archiv gegen den Hash im Manifest.
pub fn verify(entry: &BackupEntry) -> Result<()> {
    let manifest = manifest_lesen(entry)?;

    let datei = std::fs::File::open(&entry.archive).map_err(|e| Error::io(&entry.archive, e))?;
    let mut zip = zip::ZipArchive::new(datei)
        .map_err(|e| Error::CorruptBackup(format!("Archiv unlesbar: {e}")))?;

    let mut gesehen = 0usize;
    for i in 0..zip.len() {
        let mut eintrag = zip
            .by_index(i)
            .map_err(|e| Error::CorruptBackup(format!("Eintrag {i} unlesbar: {e}")))?;
        if !eintrag.is_file() {
            continue;
        }
        let name = eintrag.name().to_string();
        let erwartet = manifest
            .files
            .get(&name)
            .ok_or_else(|| Error::CorruptBackup(format!("{name} steht nicht im Manifest")))?;

        let mut inhalt = Vec::new();
        std::io::copy(&mut eintrag, &mut inhalt)
            .map_err(|e| Error::CorruptBackup(format!("{name} nicht entpackbar: {e}")))?;

        let ist = blake3::hash(&inhalt).to_hex().to_string();
        if ist != erwartet.hash {
            return Err(Error::CorruptBackup(format!("{name} hat einen abweichenden Hash")));
        }
        gesehen += 1;
    }

    if gesehen != manifest.files.len() {
        return Err(Error::CorruptBackup(format!(
            "Archiv enthält {gesehen} von {} erwarteten Dateien",
            manifest.files.len()
        )));
    }
    Ok(())
}

/// Alle Backups, neueste zuerst.
pub fn list_backups(backup_root: &Path) -> Result<Vec<BackupEntry>> {
    if !backup_root.is_dir() {
        return Ok(Vec::new());
    }

    let mut ergebnis = Vec::new();
    for eintrag in std::fs::read_dir(backup_root).map_err(|e| Error::io(backup_root, e))? {
        let pfad = eintrag.map_err(|e| Error::io(backup_root, e))?.path();
        if pfad.extension().and_then(|e| e.to_str()) != Some("zip") {
            continue;
        }
        let manifest = pfad.with_extension("json");
        if !manifest.is_file() {
            continue;
        }
        let kandidat = BackupEntry {
            archive: pfad,
            manifest,
            created_at: String::new(),
            label: None,
        };
        if let Ok(m) = manifest_lesen(&kandidat) {
            ergebnis.push(BackupEntry {
                created_at: m.created_at,
                label: m.label,
                ..kandidat
            });
        }
    }

    // Neueste zuerst. Bei gleicher Sekunde entscheidet der Dateiname, damit die
    // Reihenfolge nicht von der Verzeichnisreihenfolge abhängt.
    ergebnis.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.archive.cmp(&a.archive))
    });
    Ok(ergebnis)
}

/// Stellt ein Backup wieder her. Legt **immer** zuvor eine Sicherung des
/// aktuellen Standes an und gibt sie zurück.
pub fn restore(entry: &BackupEntry, save_dir: &Path, backup_root: &Path) -> Result<BackupEntry> {
    verify(entry)?;

    let sicherung = backup(save_dir, backup_root, Some("vor Wiederherstellung"))?;

    let datei = std::fs::File::open(&entry.archive).map_err(|e| Error::io(&entry.archive, e))?;
    let mut zip = zip::ZipArchive::new(datei)
        .map_err(|e| Error::CorruptBackup(format!("Archiv unlesbar: {e}")))?;

    for i in 0..zip.len() {
        let mut eintrag = zip
            .by_index(i)
            .map_err(|e| Error::CorruptBackup(format!("Eintrag {i} unlesbar: {e}")))?;
        if !eintrag.is_file() {
            continue;
        }

        // Nur Pfade aus dem eigenen Manifest, Komponente für Komponente geprüft.
        let name = eintrag.name().to_string();
        let mut ziel = save_dir.to_path_buf();
        for teil in name.split('/') {
            if teil.is_empty() || teil == "." || teil == ".." {
                return Err(Error::CorruptBackup(format!("unzulässiger Pfad im Archiv: {name}")));
            }
            ziel.push(teil);
        }

        if let Some(eltern) = ziel.parent() {
            std::fs::create_dir_all(eltern).map_err(|e| Error::io(eltern, e))?;
        }
        let mut aus = std::fs::File::create(&ziel).map_err(|e| Error::io(&ziel, e))?;
        std::io::copy(&mut eintrag, &mut aus).map_err(|e| Error::io(&ziel, e))?;
    }

    Ok(sicherung)
}

/// Läuft Steam gerade? Cloud-Synchronisation kann eine Wiederherstellung
/// überschreiben – der wahrscheinlichste Weg zu Datenverlust.
pub fn steam_is_running() -> bool {
    let Ok(eintraege) = std::fs::read_dir("/proc") else {
        return false;
    };
    for eintrag in eintraege.filter_map(std::result::Result::ok) {
        let comm = eintrag.path().join("comm");
        if let Ok(name) = std::fs::read_to_string(&comm) {
            if name.trim() == "steam" {
                return true;
            }
        }
    }
    false
}
```

- [ ] **Schritt 5: In `lib.rs` eintragen**

```rust
pub mod saves;
```

- [ ] **Schritt 6: Tests laufen lassen**

Run: `cargo test -p sm2-core saves`
Expected: PASS, 8 Tests

- [ ] **Schritt 7: Commit**

```bash
git add crates/core/src/saves.rs crates/core/src/lib.rs
git commit -m "feat(core): Save-Backup mit Integritätsprüfung und Zwangssicherung"
```

---

## Task 10: Profile und Einstellungen

**Files:**
- Create: `crates/core/src/profile.rs`, `crates/core/src/settings.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `PakConfig`, `PakEntry`, `atomic::write_atomic`, `Error`, `Result`
- Produces:
  - `Profile { name: String, entries: Vec<ProfileEntry> }` — `Serialize, Deserialize, Debug, Clone, PartialEq`
  - `ProfileEntry { pak: String, disabled: bool }` — `Serialize, Deserialize, Debug, Clone, PartialEq`
  - `Profile::from_config(name: &str, cfg: &PakConfig) -> Profile`
  - `Profile::apply(&self, present: &[String]) -> (PakConfig, Vec<String>)` — zweiter Rückgabewert: fehlende Paks
  - `Profile::load(path: &Path) -> Result<Profile>`, `Profile::save(&self, dir: &Path) -> Result<PathBuf>`
  - `list_profiles(dir: &Path) -> Result<Vec<Profile>>`
  - `Settings { game_dir: Option<PathBuf>, auto_backup: bool, steam_user: Option<String> }` — `Serialize, Deserialize, Debug, Clone`, `Default` mit `auto_backup: true`
  - `Settings::load(path: &Path) -> Result<Settings>`, `Settings::save(&self, path: &Path) -> Result<()>`

- [ ] **Schritt 1: Fehlgeschlagene Tests schreiben**

`crates/core/src/profile.rs`:

```rust
use crate::atomic::write_atomic;
use crate::error::{Error, Result};
use crate::pak_config::{PakConfig, PakEntry};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(eintraege: &[(&str, bool)]) -> PakConfig {
        PakConfig {
            entries: eintraege
                .iter()
                .map(|(p, d)| PakEntry { pak: p.to_string(), disabled: *d })
                .collect(),
        }
    }

    #[test]
    fn profil_uebernimmt_reihenfolge_und_zustand() {
        let profil = Profile::from_config("Astartes", &cfg(&[("z.pak", false), ("a.pak", true)]));

        assert_eq!(profil.name, "Astartes");
        assert_eq!(profil.entries[0].pak, "z.pak");
        assert!(!profil.entries[0].disabled);
        assert!(profil.entries[1].disabled);
    }

    #[test]
    fn anwenden_stellt_reihenfolge_und_zustand_wieder_her() {
        let profil = Profile::from_config("P", &cfg(&[("z.pak", false), ("a.pak", true)]));
        let (neu, fehlend) = profil.apply(&["a.pak".into(), "z.pak".into()]);

        let namen: Vec<&str> = neu.entries.iter().map(|e| e.pak.as_str()).collect();
        assert_eq!(namen, vec!["z.pak", "a.pak"]);
        assert!(neu.entries[1].disabled);
        assert!(fehlend.is_empty());
    }

    #[test]
    fn anwenden_meldet_fehlende_paks_und_ueberspringt_sie() {
        let profil = Profile::from_config("P", &cfg(&[("weg.pak", false), ("da.pak", false)]));
        let (neu, fehlend) = profil.apply(&["da.pak".into()]);

        assert_eq!(fehlend, vec!["weg.pak"]);
        assert_eq!(neu.entries.len(), 1);
        assert_eq!(neu.entries[0].pak, "da.pak");
    }

    #[test]
    fn anwenden_nimmt_unbekannte_paks_deaktiviert_auf() {
        // Ein Pak im Verzeichnis, das das Profil nicht kennt, würde sonst
        // ungesteuert zuerst geladen (Engine-Regel).
        let profil = Profile::from_config("P", &cfg(&[("bekannt.pak", false)]));
        let (neu, _) = profil.apply(&["bekannt.pak".into(), "fremd.pak".into()]);

        assert_eq!(neu.entries.len(), 2);
        let fremd = neu.entries.iter().find(|e| e.pak == "fremd.pak").unwrap();
        assert!(fremd.disabled, "unbekannte Paks dürfen nicht stillschweigend aktiv sein");
    }

    #[test]
    fn speichern_und_laden_sind_zueinander_invers() {
        let dir = tempfile::tempdir().unwrap();
        let profil = Profile::from_config("Mein Profil", &cfg(&[("a.pak", true)]));

        let pfad = profil.save(dir.path()).unwrap();

        assert_eq!(Profile::load(&pfad).unwrap(), profil);
    }

    #[test]
    fn liste_ist_alphabetisch_und_ignoriert_fremddateien() {
        let dir = tempfile::tempdir().unwrap();
        Profile::from_config("Zulu", &cfg(&[])).save(dir.path()).unwrap();
        Profile::from_config("Alpha", &cfg(&[])).save(dir.path()).unwrap();
        std::fs::write(dir.path().join("notizen.txt"), b"egal").unwrap();

        let namen: Vec<String> =
            list_profiles(dir.path()).unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(namen, vec!["Alpha", "Zulu"]);
    }
}
```

- [ ] **Schritt 2: Test laufen lassen, Fehlschlag bestätigen**

Run: `cargo test -p sm2-core profile`
Expected: FAIL — `cannot find type 'Profile' in this scope`

- [ ] **Schritt 3: Implementierung schreiben**

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileEntry {
    pub pak: String,
    #[serde(default)]
    pub disabled: bool,
}

/// Eine benannte Zusammenstellung: Mod-Auswahl plus Ladereihenfolge.
/// Genau die Information, die pak_config.yaml braucht – nur ein paar hundert Byte.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub entries: Vec<ProfileEntry>,
}

impl Profile {
    pub fn from_config(name: &str, cfg: &PakConfig) -> Self {
        Self {
            name: name.to_string(),
            entries: cfg
                .entries
                .iter()
                .map(|e| ProfileEntry { pak: e.pak.clone(), disabled: e.disabled })
                .collect(),
        }
    }

    /// Baut die Konfiguration für das Profil. Fehlende Paks werden
    /// übersprungen und zurückgemeldet; das Profil bleibt unverändert.
    /// Vorhandene, dem Profil unbekannte Paks kommen deaktiviert ans Ende –
    /// sonst würde die Engine sie ungesteuert zuerst laden.
    pub fn apply(&self, present: &[String]) -> (PakConfig, Vec<String>) {
        let vorhanden: std::collections::HashSet<&str> =
            present.iter().map(String::as_str).collect();

        let mut fehlend = Vec::new();
        let mut entries = Vec::new();

        for e in &self.entries {
            if vorhanden.contains(e.pak.as_str()) {
                entries.push(PakEntry { pak: e.pak.clone(), disabled: e.disabled });
            } else {
                fehlend.push(e.pak.clone());
            }
        }

        let bekannt: std::collections::HashSet<&str> =
            entries.iter().map(|e| e.pak.as_str()).collect();
        let mut fremd: Vec<&String> =
            present.iter().filter(|p| !bekannt.contains(p.as_str())).collect();
        fremd.sort();
        for p in fremd {
            entries.push(PakEntry { pak: p.clone(), disabled: true });
        }

        (PakConfig { entries }, fehlend)
    }

    fn dateiname(&self) -> String {
        let sauber: String = self
            .name
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
            .collect();
        format!("{}.toml", sauber.trim_matches('-').to_lowercase())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
        toml::from_str(&text).map_err(|e| {
            Error::io(path, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })
    }

    pub fn save(&self, dir: &Path) -> Result<PathBuf> {
        let pfad = dir.join(self.dateiname());
        let text = toml::to_string_pretty(self).map_err(|e| {
            Error::io(&pfad, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        write_atomic(&pfad, &text)?;
        Ok(pfad)
    }
}

pub fn list_profiles(dir: &Path) -> Result<Vec<Profile>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut ergebnis: Vec<Profile> = std::fs::read_dir(dir)
        .map_err(|e| Error::io(dir, e))?
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("toml"))
        .filter_map(|p| Profile::load(&p).ok())
        .collect();
    ergebnis.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(ergebnis)
}
```

- [ ] **Schritt 4: Einstellungen schreiben**

`crates/core/src/settings.rs`:

```rust
use crate::atomic::write_atomic;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Überschreibt die automatische Erkennung.
    pub game_dir: Option<PathBuf>,
    /// Vor jedem Modded-Start ein Save-Backup anlegen.
    pub auto_backup: bool,
    /// SteamID64, falls mehrere Profile im Prefix liegen.
    pub steam_user: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { game_dir: None, auto_backup: true, steam_user: None }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| {
                Error::io(path, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::io(path, e)),
        }
    }

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
    fn auto_backup_ist_standardmaessig_an() {
        assert!(Settings::default().auto_backup);
    }

    #[test]
    fn fehlende_datei_ergibt_standardwerte() {
        let dir = tempfile::tempdir().unwrap();
        let s = Settings::load(&dir.path().join("gibt_es_nicht.toml")).unwrap();
        assert!(s.auto_backup);
        assert!(s.game_dir.is_none());
    }

    #[test]
    fn speichern_und_laden_erhaelt_werte() {
        let dir = tempfile::tempdir().unwrap();
        let pfad = dir.path().join("settings.toml");
        let s = Settings {
            game_dir: Some(PathBuf::from("/spiele/SM2")),
            auto_backup: false,
            steam_user: Some("76561198412726373".into()),
        };
        s.save(&pfad).unwrap();

        let geladen = Settings::load(&pfad).unwrap();
        assert_eq!(geladen.game_dir, s.game_dir);
        assert!(!geladen.auto_backup);
        assert_eq!(geladen.steam_user, s.steam_user);
    }
}
```

- [ ] **Schritt 5: In `lib.rs` eintragen**

```rust
pub mod profile;
pub mod settings;
```

- [ ] **Schritt 6: Tests laufen lassen**

Run: `cargo test -p sm2-core`
Expected: PASS, alle Tests aus Task 1–10

- [ ] **Schritt 7: Commit**

```bash
git add crates/core/src/profile.rs crates/core/src/settings.rs crates/core/src/lib.rs
git commit -m "feat(core): Profile und Einstellungen"
```

---

## Task 11: Spielstart

**Files:**
- Create: `crates/core/src/launch.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `GamePaths`, `Platform`, `Current`, `APP_ID`, `Error`, `Result`
- Produces:
  - `LaunchMode { Steam, NoEac }` — `Debug, Clone, Copy, PartialEq, Eq`
  - `launch(paths: &GamePaths, mode: LaunchMode) -> Result<()>`
  - `eac_available() -> bool`

- [ ] **Schritt 1: Fehlgeschlagenen Test schreiben**

`crates/core/src/launch.rs`:

```rust
use crate::error::{Error, Result};
use crate::paths::GamePaths;
use crate::platform::{Current, Platform};
use crate::APP_ID;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_eac_meldet_fehlende_executable_bevor_gestartet_wird() {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("Space Marine 2");
        std::fs::create_dir_all(game.join("client_pc/root/mods")).unwrap();
        let paths = GamePaths::from_game_dir(&game, tmp.path()).unwrap();

        // Die .exe existiert nicht – der Fehler muss das benennen, nicht umu.
        let fehler = launch(&paths, LaunchMode::NoEac).unwrap_err();
        let text = fehler.to_string();
        assert!(text.contains("Retail.exe") || text.contains("nicht gefunden"), "unklare Meldung: {text}");
    }
}
```

- [ ] **Schritt 2: Test laufen lassen, Fehlschlag bestätigen**

Run: `cargo test -p sm2-core launch`
Expected: FAIL — `cannot find type 'LaunchMode' in this scope`

- [ ] **Schritt 3: Implementierung schreiben**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    /// Regulärer Start über Steam, mit EAC.
    Steam,
    /// Direktstart im vorhandenen Proton-Prefix, ohne EAC.
    /// Nur für Offline-Modding – kein Multiplayer.
    NoEac,
}

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
            let app_id = APP_ID.to_string();
            Current::launch_direct(
                &exe,
                &[
                    ("SteamAppId", app_id.as_str()),
                    ("SteamGameId", app_id.as_str()),
                    ("WINEPREFIX", ""),
                ],
            )
        }
    }
}

/// Ist ein Start ohne EAC auf diesem System überhaupt möglich?
pub fn eac_available() -> bool {
    crate::platform::unix::Unix::umu_launcher().is_some()
}
```

Hinweis zu `WINEPREFIX`: leer übergeben, damit `umu-run` den Prefix aus seiner eigenen Konfiguration wählt. Ob das genügt, klärt Schritt 5.

- [ ] **Schritt 4: In `lib.rs` eintragen und Tests laufen lassen**

```rust
pub mod launch;
```

Run: `cargo test -p sm2-core`
Expected: PASS

- [ ] **Schritt 5: EAC-Bypass manuell prüfen**

Dieser Pfad ist nicht automatisiert testbar — er startet ein echtes Spiel.

```bash
command -v umu-run || echo "umu-launcher fehlt – Bypass ist auf diesem System nicht verfügbar"
```

Ist `umu-run` vorhanden: den Start über die CLI aus Task 12 prüfen (`sm2-modloader play --no-eac`). Erwartet: das Spiel startet ohne EAC-Fenster.

Startet es nicht, weil der Prefix nicht gefunden wird, in `launch` statt des leeren Wertes setzen:

```rust
let prefix = paths
    .library_dir
    .join("steamapps/compatdata")
    .join(APP_ID.to_string())
    .join("pfx");
let prefix_str = prefix.display().to_string();
// ... und ("WINEPREFIX", prefix_str.as_str()) statt ("WINEPREFIX", "") übergeben
```

Fehlt `umu-run`, ist das kein Fehler des Plans — `eac_available()` meldet es, und die GUI in Plan 2 blendet die Option aus.

- [ ] **Schritt 6: Commit**

```bash
git add crates/core/src/launch.rs crates/core/src/lib.rs
git commit -m "feat(core): Spielstart über Steam und ohne EAC"
```

---

## Task 12: Kommandozeile

**Files:**
- Create: `crates/app/src/cli.rs`, `crates/app/src/app_state.rs`
- Modify: `crates/app/src/main.rs`

**Interfaces:**
- Consumes: alles aus `sm2-core`
- Produces:
  - `AppState { paths: GamePaths, settings: Settings, dirs: AppDirs, library: Library, config: PakConfig }`
  - `AppState::open() -> anyhow::Result<AppState>` — Erkennung, Laden, Abgleich in einem Schritt
  - `AppState::persist(&self) -> anyhow::Result<()>`
  - `cli::run() -> anyhow::Result<()>`

Kommandos: `list`, `enable <pak>`, `disable <pak>`, `order <pak…>`, `install <datei…>`, `profile list|save|apply`, `save backup|list|restore`, `play [--vanilla|--no-eac]`, `paths`.

- [ ] **Schritt 1: Gemeinsamen Zustand schreiben**

`crates/app/src/app_state.rs`:

```rust
use anyhow::{Context, Result};
use sm2_core::library::Library;
use sm2_core::pak_config::PakConfig;
use sm2_core::paths::{app_dirs, AppDirs, GamePaths};
use sm2_core::settings::Settings;
use std::path::PathBuf;

pub struct AppState {
    pub paths: GamePaths,
    pub settings: Settings,
    pub dirs: AppDirs,
    pub library: Library,
    pub config: PakConfig,
}

impl AppState {
    /// Erkennt das Spiel, lädt alle Zustände und gleicht die Konfiguration
    /// mit dem Verzeichnisinhalt ab. Meldet Abweichungen auf stderr.
    pub fn open() -> Result<Self> {
        let dirs = app_dirs().context("Basisverzeichnisse nicht ermittelbar")?;
        std::fs::create_dir_all(&dirs.config)?;
        std::fs::create_dir_all(&dirs.data)?;

        let settings = Settings::load(&dirs.config.join("settings.toml"))?;

        let paths = match &settings.game_dir {
            Some(dir) => {
                // Bei manueller Angabe die Bibliothek aus dem Pfad ableiten.
                let library = dir
                    .ancestors()
                    .find(|a| a.join("steamapps/common").is_dir())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| dir.clone());
                GamePaths::from_game_dir(dir, &library)?
            }
            None => GamePaths::discover()
                .context("Space Marine 2 nicht gefunden – Pfad in settings.toml unter game_dir eintragen")?,
        };

        // Spec 7: Rechte beim Start prüfen, nicht erst beim Speichern.
        pruefe_schreibrecht(&paths.mods_dir())?;

        let library = Library::load(&dirs.data.join("library.json"))?;
        let mut config = PakConfig::load(&paths.pak_config_path())?;

        let abgleich = config.reconcile(&paths.list_paks()?);
        for pak in &abgleich.added {
            eprintln!("Hinweis: {pak} war nicht in pak_config.yaml eingetragen und wurde aktiv übernommen.");
        }
        for pak in &abgleich.removed {
            eprintln!("Hinweis: {pak} steht in pak_config.yaml, die Datei fehlt aber – Eintrag entfernt.");
        }

        Ok(Self { paths, settings, dirs, library, config })
    }

    pub fn persist(&self) -> Result<()> {
        self.config.save(&self.paths.pak_config_path())?;
        self.library.save(&self.dirs.data.join("library.json"))?;
        Ok(())
    }

    pub fn profiles_dir(&self) -> PathBuf {
        self.dirs.data.join("profiles")
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.dirs.data.join("backups/saves")
    }
}

/// Stellt fest, ob wir in das Verzeichnis schreiben können – bevor der Nutzer
/// Änderungen vornimmt, die dann am Speichern scheitern.
fn pruefe_schreibrecht(dir: &std::path::Path) -> Result<()> {
    let probe = dir.join(".sm2-modloader-schreibtest");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(_) => Err(sm2_core::Error::NotWritable(dir.to_path_buf()).into()),
    }
}
```

- [ ] **Schritt 2: Kommandodefinition schreiben**

`crates/app/src/cli.rs`:

```rust
use crate::app_state::AppState;
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use sm2_core::launch::{launch, LaunchMode};
use sm2_core::pak_config::PakEntry;
use sm2_core::platform::{Current, Platform};
use sm2_core::profile::{list_profiles, Profile};
use sm2_core::{import, saves};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "sm2-modloader", about = "Mod-Loader für Space Marine 2", version)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Zeigt alle Mods in Ladereihenfolge
    List,
    /// Aktiviert einen Mod
    Enable { pak: String },
    /// Deaktiviert einen Mod
    Disable { pak: String },
    /// Setzt die Ladereihenfolge; nicht genannte Mods behalten ihre Position dahinter
    Order { paks: Vec<String> },
    /// Importiert Mods aus .pak, .zip, .7z oder .rar
    Install { dateien: Vec<PathBuf> },
    /// Zeigt die erkannten Verzeichnisse
    Paths,
    /// Öffnet ein Verzeichnis im Dateimanager
    Open {
        #[arg(value_enum)]
        was: OpenTarget,
    },
    /// Profile verwalten
    #[command(subcommand)]
    Profile(ProfileCmd),
    /// Savegames sichern und wiederherstellen
    #[command(subcommand)]
    Save(SaveCmd),
    /// Startet das Spiel
    Play {
        /// Alle Mods deaktivieren
        #[arg(long)]
        vanilla: bool,
        /// Ohne EAC starten (kein Multiplayer)
        #[arg(long)]
        no_eac: bool,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum OpenTarget {
    /// Spielverzeichnis
    Game,
    /// Mods-Verzeichnis
    Mods,
    /// Savegame-Verzeichnis im Proton-Prefix
    Saves,
    /// Backup-Verzeichnis des Loaders
    Backups,
}

#[derive(Subcommand)]
enum ProfileCmd {
    List,
    /// Speichert den aktuellen Zustand als Profil
    Save { name: String },
    /// Wendet ein Profil an
    Apply { name: String },
}

#[derive(Subcommand)]
enum SaveCmd {
    /// Legt ein Backup an
    Backup {
        #[arg(long)]
        tag: Option<String>,
    },
    List,
    /// Stellt ein Backup wieder her (Standard: das neueste)
    Restore {
        #[arg(long)]
        index: Option<usize>,
    },
}
```

- [ ] **Schritt 3: Ausführung schreiben**

In `crates/app/src/cli.rs` weiter:

```rust
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let mut state = AppState::open()?;

    match cli.command {
        Command::List => {
            if state.config.entries.is_empty() {
                println!("Keine Mods installiert.");
            }
            for (i, e) in state.config.entries.iter().enumerate() {
                let marke = if e.disabled { "○" } else { "●" };
                let name = state
                    .library
                    .mods
                    .get(&e.pak)
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| e.pak.clone());
                println!("{:>2}. {marke} {name}  ({})", i + 1, e.pak);
            }
        }

        Command::Enable { pak } => {
            setze_zustand(&mut state, &pak, false)?;
            state.persist()?;
            println!("✓ {pak} aktiviert");
        }

        Command::Disable { pak } => {
            setze_zustand(&mut state, &pak, true)?;
            state.persist()?;
            println!("✓ {pak} deaktiviert");
        }

        Command::Order { paks } => {
            let mut neu: Vec<PakEntry> = Vec::new();
            for wunsch in &paks {
                let Some(pos) = state.config.entries.iter().position(|e| &e.pak == wunsch) else {
                    bail!("{wunsch} ist nicht installiert");
                };
                neu.push(state.config.entries.remove(pos));
            }
            neu.extend(state.config.entries.drain(..));
            state.config.entries = neu;
            state.persist()?;
            println!("✓ Ladereihenfolge gesetzt");
        }

        Command::Install { dateien } => {
            let tmp = tempfile::tempdir()?;
            for datei in &dateien {
                let paks = import::extract_paks(datei, tmp.path())
                    .with_context(|| format!("{} konnte nicht gelesen werden", datei.display()))?;
                for pak in paks {
                    let quelle = datei.display().to_string();
                    let ergebnis = import::import_pak(
                        &state.paths,
                        &mut state.library,
                        &mut state.config,
                        &pak,
                        Some(&quelle),
                    )?;
                    match ergebnis.duplicate_of {
                        Some(vorhanden) => {
                            println!("– {} ist inhaltsgleich mit {vorhanden}, übersprungen", ergebnis.pak)
                        }
                        None => println!("✓ {} importiert (deaktiviert)", ergebnis.pak),
                    }
                }
            }
            state.persist()?;
        }

        Command::Paths => {
            println!("Spiel:   {}", state.paths.game_dir.display());
            println!("Mods:    {}", state.paths.mods_dir().display());
            println!("Config:  {}", state.paths.pak_config_path().display());
            match state.paths.save_dir() {
                Ok(p) => println!("Saves:   {}", p.display()),
                Err(e) => println!("Saves:   nicht verfügbar – {e}"),
            }
            println!("Backups: {}", state.backups_dir().display());
        }

        Command::Open { was } => {
            let ziel = match was {
                OpenTarget::Game => state.paths.game_dir.clone(),
                OpenTarget::Mods => state.paths.mods_dir(),
                OpenTarget::Saves => state.paths.save_dir()?,
                OpenTarget::Backups => {
                    let dir = state.backups_dir();
                    std::fs::create_dir_all(&dir)?;
                    dir
                }
            };
            Current::open_folder(&ziel)?;
        }

        Command::Profile(cmd) => profil_kommando(&mut state, cmd)?,
        Command::Save(cmd) => save_kommando(&state, cmd)?,

        Command::Play { vanilla, no_eac } => {
            if vanilla {
                for e in &mut state.config.entries {
                    e.disabled = true;
                }
            }
            state.persist()?;

            if !vanilla && state.settings.auto_backup {
                match state.paths.save_dir() {
                    Ok(saves) => {
                        let eintrag = saves::backup(&saves, &state.backups_dir(), Some("vor Modded-Start"))?;
                        println!("✓ Save gesichert: {}", eintrag.archive.display());
                    }
                    Err(e) => eprintln!("Warnung: kein Save-Backup möglich – {e}"),
                }
            }

            let modus = if no_eac { LaunchMode::NoEac } else { LaunchMode::Steam };
            if no_eac {
                eprintln!("Hinweis: Start ohne EAC – Multiplayer ist damit nicht möglich.");
            }
            launch(&state.paths, modus)?;
            println!("✓ Spiel gestartet");
        }
    }

    Ok(())
}

fn setze_zustand(state: &mut AppState, pak: &str, disabled: bool) -> Result<()> {
    let eintrag = state
        .config
        .entries
        .iter_mut()
        .find(|e| e.pak == pak)
        .with_context(|| format!("{pak} ist nicht installiert"))?;
    eintrag.disabled = disabled;
    Ok(())
}

fn profil_kommando(state: &mut AppState, cmd: ProfileCmd) -> Result<()> {
    let dir = state.profiles_dir();
    std::fs::create_dir_all(&dir)?;

    match cmd {
        ProfileCmd::List => {
            let profile = list_profiles(&dir)?;
            if profile.is_empty() {
                println!("Keine Profile gespeichert.");
            }
            for p in profile {
                let aktiv = p.entries.iter().filter(|e| !e.disabled).count();
                println!("{}  ({aktiv} aktiv von {})", p.name, p.entries.len());
            }
        }
        ProfileCmd::Save { name } => {
            let profil = Profile::from_config(&name, &state.config);
            let pfad = profil.save(&dir)?;
            println!("✓ Profil '{name}' gespeichert: {}", pfad.display());
        }
        ProfileCmd::Apply { name } => {
            let profil = list_profiles(&dir)?
                .into_iter()
                .find(|p| p.name.eq_ignore_ascii_case(&name))
                .with_context(|| format!("Profil '{name}' nicht gefunden"))?;

            let (neu, fehlend) = profil.apply(&state.paths.list_paks()?);
            for pak in &fehlend {
                eprintln!("Warnung: {pak} aus dem Profil ist nicht installiert, übersprungen.");
            }
            state.config = neu;
            state.persist()?;
            println!("✓ Profil '{}' angewendet", profil.name);
        }
    }
    Ok(())
}

fn save_kommando(state: &AppState, cmd: SaveCmd) -> Result<()> {
    let backups = state.backups_dir();

    match cmd {
        SaveCmd::Backup { tag } => {
            let saves = state.paths.save_dir()?;
            let eintrag = saves::backup(&saves, &backups, tag.as_deref())?;
            println!("✓ Backup: {}", eintrag.archive.display());
        }
        SaveCmd::List => {
            let liste = saves::list_backups(&backups)?;
            if liste.is_empty() {
                println!("Keine Backups vorhanden.");
            }
            for (i, e) in liste.iter().enumerate() {
                let etikett = e.label.clone().unwrap_or_default();
                println!("{:>2}. {}  {etikett}", i + 1, e.created_at);
            }
        }
        SaveCmd::Restore { index } => {
            let liste = saves::list_backups(&backups)?;
            if liste.is_empty() {
                bail!("keine Backups vorhanden");
            }
            let i = index.unwrap_or(1);
            let eintrag = liste
                .get(i - 1)
                .with_context(|| format!("Backup {i} gibt es nicht ({} vorhanden)", liste.len()))?;

            if saves::steam_is_running() {
                bail!(
                    "Steam läuft. Die Cloud-Synchronisation kann den wiederhergestellten Stand \
                     überschreiben. Bitte Steam beenden und erneut versuchen."
                );
            }

            let saves_dir = state.paths.save_dir()?;
            let sicherung = saves::restore(eintrag, &saves_dir, &backups)?;
            println!("✓ Wiederhergestellt: {}", eintrag.created_at);
            println!("  Vorheriger Stand gesichert: {}", sicherung.archive.display());
        }
    }
    Ok(())
}
```

- [ ] **Schritt 4: Einstiegspunkt schreiben**

`crates/app/src/main.rs`:

```rust
mod app_state;
mod cli;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    // Ohne Argumente startet später die GUI (Plan 2). Bis dahin: Hilfe zeigen.
    if std::env::args().len() == 1 {
        eprintln!("Die grafische Oberfläche folgt in Plan 2.\n");
        eprintln!("Verfügbare Kommandos: sm2-modloader --help");
        std::process::exit(2);
    }

    if let Err(e) = cli::run() {
        eprintln!("Fehler: {e:#}");
        std::process::exit(1);
    }
}
```

`crates/app/Cargo.toml` ergänzen:

```toml
tempfile.workspace = true
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

und in der Workspace-`Cargo.toml` unter `[workspace.dependencies]`:

```toml
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

- [ ] **Schritt 5: Bauen und CLI-Hilfe prüfen**

Run: `cargo build --workspace && cargo run -p sm2-modloader -- --help`
Expected: Hilfetext mit allen Kommandos, kein Compilerfehler

- [ ] **Schritt 6: Gegen die echte Installation prüfen**

```bash
cargo run -p sm2-modloader -- paths
cargo run -p sm2-modloader -- list
```

Expected: Spielpfad aus `SSD2000/SteamLibrary`, Save-Pfad auf `76561198412726373/Main`, und `wa_astartes_14_1.pak` in der Liste.

- [ ] **Schritt 7: Vollständigen Durchlauf ohne Spielstart prüfen**

```bash
cargo run -p sm2-modloader -- profile save Ausgangszustand
cargo run -p sm2-modloader -- save backup --tag "vor Test"
cargo run -p sm2-modloader -- disable wa_astartes_14_1.pak
cargo run -p sm2-modloader -- list
cargo run -p sm2-modloader -- profile apply Ausgangszustand
cargo run -p sm2-modloader -- list
```

Expected: Mod wird deaktiviert (`○`), das Profil stellt den Ausgangszustand wieder her (`●`). Das Backup liegt unter `~/.local/share/sm2-modloader/backups/saves/`.

- [ ] **Schritt 8: Commit**

```bash
git add crates/app Cargo.toml
git commit -m "feat(cli): Kommandozeile für Mods, Profile, Saves und Spielstart"
```

---

## Task 13: Integrationstest über den gesamten Ablauf

**Files:**
- Create: `crates/core/tests/ablauf.rs`

**Interfaces:**
- Consumes: die öffentliche API von `sm2-core`
- Produces: nichts — reine Absicherung

Die Modultests prüfen Bausteine einzeln. Dieser Test prüft, dass sie zusammen den Ablauf ergeben, den ein Nutzer durchläuft.

- [ ] **Schritt 1: Test schreiben**

`crates/core/tests/ablauf.rs`:

```rust
use sm2_core::library::Library;
use sm2_core::pak_config::PakConfig;
use sm2_core::paths::GamePaths;
use sm2_core::profile::Profile;
use sm2_core::{import, saves};

/// Baut Spielverzeichnis samt Proton-Prefix und Savegames.
fn welt() -> (tempfile::TempDir, GamePaths) {
    let tmp = tempfile::tempdir().unwrap();
    let library = tmp.path().join("SteamLibrary");
    let game = library.join("steamapps/common/Space Marine 2");
    std::fs::create_dir_all(game.join("client_pc/root/mods")).unwrap();

    let saves = library
        .join("steamapps/compatdata/2183900/pfx/drive_c/users/steamuser")
        .join("AppData/Local/Saber/Space Marine 2/storage/steam/user/76561198412726373/Main");
    std::fs::create_dir_all(&saves).unwrap();
    std::fs::write(saves.join("profile.sav"), b"FORTSCHRITT").unwrap();

    let paths = GamePaths::from_game_dir(&game, &library).unwrap();
    (tmp, paths)
}

#[test]
fn vom_import_ueber_profile_bis_zur_wiederherstellung() {
    let (tmp, paths) = welt();
    let daten = tmp.path().join("appdata");
    let backups = daten.join("backups/saves");
    let profile_dir = daten.join("profiles");
    std::fs::create_dir_all(&profile_dir).unwrap();

    let mut lib = Library::default();
    let mut cfg = PakConfig::default();

    // Zwei Mods importieren – beide landen deaktiviert.
    let quelle = tmp.path().join("downloads");
    std::fs::create_dir_all(&quelle).unwrap();
    for (name, inhalt) in [("astartes.pak", &b"ASTARTES"[..]), ("chaplain.pak", &b"CHAPLAIN"[..])] {
        let pfad = quelle.join(name);
        std::fs::write(&pfad, inhalt).unwrap();
        import::import_pak(&paths, &mut lib, &mut cfg, &pfad, None).unwrap();
    }
    assert_eq!(cfg.entries.len(), 2);
    assert!(cfg.entries.iter().all(|e| e.disabled), "Import darf nichts aktivieren");

    // Einen aktivieren, Reihenfolge festlegen, in die Engine-Datei schreiben.
    cfg.entries.iter_mut().find(|e| e.pak == "astartes.pak").unwrap().disabled = false;
    cfg.entries.reverse();
    cfg.save(&paths.pak_config_path()).unwrap();

    // Die Engine würde exakt das lesen.
    let gelesen = PakConfig::load(&paths.pak_config_path()).unwrap();
    assert_eq!(gelesen, cfg);
    assert_eq!(gelesen.enabled().count(), 1);

    // Zustand als Profil sichern, dann alles abschalten (Vanilla).
    let profil = Profile::from_config("Astartes", &cfg);
    profil.save(&profile_dir).unwrap();

    for e in &mut cfg.entries {
        e.disabled = true;
    }
    cfg.save(&paths.pak_config_path()).unwrap();
    assert_eq!(PakConfig::load(&paths.pak_config_path()).unwrap().enabled().count(), 0);

    // Profil wiederherstellen.
    let (wieder, fehlend) = profil.apply(&paths.list_paks().unwrap());
    assert!(fehlend.is_empty());
    wieder.save(&paths.pak_config_path()).unwrap();
    let jetzt = PakConfig::load(&paths.pak_config_path()).unwrap();
    assert_eq!(jetzt.enabled().map(|e| e.pak.as_str()).collect::<Vec<_>>(), vec!["astartes.pak"]);

    // Savegame sichern, kaputtmachen, wiederherstellen.
    let save_dir = paths.save_dir().unwrap();
    let eintrag = saves::backup(&save_dir, &backups, Some("vor Modded-Start")).unwrap();
    saves::verify(&eintrag).unwrap();

    std::fs::write(save_dir.join("profile.sav"), b"ZERSTOERT").unwrap();
    let sicherung = saves::restore(&eintrag, &save_dir, &backups).unwrap();

    assert_eq!(std::fs::read(save_dir.join("profile.sav")).unwrap(), b"FORTSCHRITT");
    // Auch der zerstörte Stand ist noch da, falls die Wiederherstellung falsch war.
    saves::verify(&sicherung).unwrap();
}

#[test]
fn abgleich_faengt_manuelle_eingriffe_ab() {
    let (_tmp, paths) = welt();
    let mut cfg = PakConfig::default();

    // Jemand kopiert ein Pak von Hand hinein – die Engine würde es ungesteuert laden.
    std::fs::write(paths.mods_dir().join("vonhand.pak"), b"X").unwrap();

    let ergebnis = cfg.reconcile(&paths.list_paks().unwrap());

    assert_eq!(ergebnis.added, vec!["vonhand.pak"]);
    assert_eq!(cfg.entries.len(), 1);
    assert!(!cfg.entries[0].disabled, "es lädt ohnehin – also steuerbar machen");
}
```

- [ ] **Schritt 2: Tests laufen lassen**

Run: `cargo test --workspace`
Expected: PASS, alle Unit- und Integrationstests

- [ ] **Schritt 3: Warnungen prüfen**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: keine Warnungen. Vorhandene beheben, bevor committet wird.

- [ ] **Schritt 4: Commit**

```bash
git add crates/core/tests/ablauf.rs
git commit -m "test: Integrationstest über Import, Profile und Save-Wiederherstellung"
```

---

## Abschluss von Plan 1

Nach Task 13 ist ein vollständig nutzbares Kommandozeilen-Werkzeug vorhanden. Was in **Plan 2** folgt:

- GUI mit `eframe`/`egui`: Mod-Tabelle mit Umsortieren per Ziehen, Profil-Umschalter, Backup-Übersicht
- Logging in Datei unter `~/.local/state/sm2-modloader/logs/`, Pfad in der GUI sichtbar
- GitHub Actions: Tests bei jedem Push, Release-Artefakt bei Tags
- Build mit `cargo-zigbuild` gegen glibc 2.31
- `README.md` mit Installationsanleitung

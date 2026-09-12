# SM2 Mod Loader — Design

**Datum:** 2026-09-12
**Status:** Entwurf zur Freigabe
**Ersetzt:** `spec.md` (Erstentwurf; siehe Abschnitt 2 für die Korrekturen)

---

## 1. Zweck und Zielsetzung

Ein Mod-Loader für *Warhammer 40.000: Space Marine 2* als native Linux-Anwendung, veröffentlicht für die Linux-Spielerschaft. Das Spiel selbst läuft weiterhin unter Proton — „nativ" bezieht sich auf den Loader, der ohne Wine, ohne Python-Runtime und ohne Systemabhängigkeiten als einzelne Binary läuft.

Unter Windows existiert mit [Mod 386](https://www.nexusmods.com/warhammer40000spacemarine2/mods/386) bereits ein etablierter Loader. Unter Linux gibt es keine Entsprechung: Mod 386 setzt Wine voraus, geht von Windows-Pfaden aus und bündelt Windows-Werkzeuge. Diese Lücke füllt dieses Projekt.

**Erfolgskriterien**

Das Projekt ist erfolgreich, wenn ein Linux-Spieler ohne Wine-Kenntnisse eine Datei herunterlädt, sie ausführt und ohne weitere Konfiguration Mods installieren, ihre Ladereihenfolge bestimmen und zwischen Mod-Zusammenstellungen wechseln kann — und wenn dabei nie ein Savegame verloren geht.

**Zwei Vorteile gegenüber Mod 386**, unabhängig von der Plattform:

- **Ladereihenfolge.** Die Engine unterstützt sie (siehe 2.1); Mod 386 bietet sie nicht an. Bei Mod-Konflikten ist sie das entscheidende Werkzeug.
- **Kostenloses Umschalten.** Mod 386 verschiebt Gigabytes zwischen Verzeichnissen. Dieser Loader schreibt eine Konfigurationsdatei (siehe 2.1) — unabhängig von der Mod-Größe unter 10 ms.

---

## 2. Realitätsabgleich

Die ursprüngliche `spec.md` enthält Annahmen, die eine Untersuchung des installierten Spiels widerlegt hat. Diese Funde prägen das gesamte Design.

### 2.1 Die Engine verwaltet Mods bereits selbst

`<game>/client_pc/root/mods/readme.txt` dokumentiert das Format, das die Engine liest:

```yaml
- pak: mod_a.pak
- pak: mod_b.pak
  disabled: true
```

Die Engine unterstützt also **Deaktivierung und Ladereihenfolge** ohne jede Dateibewegung. Die readme nennt außerdem eine Regel mit erheblicher Konsequenz:

> All `.pak` files in the mods directory that are not listed in the configuration file are loaded first (in alphabetical order). After that, paks enabled in the configuration file are loaded, following the order in which they are listed in the config.

Daraus folgt zwingend: **Jede `.pak`-Datei im Verzeichnis muss in der Konfiguration aufgeführt sein.** Ein nicht aufgeführter Mod lädt immer, lässt sich nicht deaktivieren und unterläuft jede Reihenfolge. Der Loader muss die Konfiguration deshalb bei jedem Start mit dem Verzeichnisinhalt abgleichen.

### 2.2 Das Kopiermodell der ursprünglichen Spec ist der teuerste denkbare Weg

`spec.md` sah eine Mod-Bibliothek unter `~/.local/share/` vor, aus der aktivierte Paks in das Spielverzeichnis kopiert werden. Auf dem Referenzsystem:

| Messung | Wert |
|---|---|
| Inhalt von `client_pc/root/mods/` | 2,7 GB |
| Dateisystem Spiel | `/dev/sdb1`, ext4 |
| Dateisystem `$HOME` | `/dev/mapper/arviddeb--vg-root`, ext4 |

Beides sind getrennte Geräte. Hardlinks scheiden aus, ext4 beherrscht kein Reflink. Jeder Mod-Wechsel wäre eine echte Kopie über Gerätegrenzen. Das Modell entfällt vollständig.

### 2.3 Die Pfade in der ursprünglichen Spec stimmen nicht

| Was | `spec.md` | Tatsächlich |
|---|---|---|
| Spielverzeichnis | `~/.local/share/Steam/steamapps/common/Space Marine 2/` | `<beliebige Steam-Library>/steamapps/common/Space Marine 2/` |
| Savegames | `…/AppData/Local/SpaceMarine2/Saved/SaveGames/` | `…/AppData/Local/Saber/Space Marine 2/storage/steam/user/<SteamID64>/Main/` |

Hardkodierte Pfade sind damit ausgeschlossen. Die Steam-Bibliothek wird über `libraryfolders.vdf` aufgelöst, die SteamID64 aus dem `user/`-Verzeichnis im Prefix ermittelt.

### 2.4 Windows- und Linux-Pfade sind verschachtelt, nicht verschieden

Der Proton-Prefix ist ein alternatives `C:\`:

```
…/compatdata/2183900/pfx/drive_c/users/steamuser/  AppData/Local/Saber/Space Marine 2/storage/…
└──────────── nur dieser Teil ist Linux ─────────┘  └──────── identisch mit Windows ────────────┘
```

Es gibt daher **eine** Pfadlogik, die sich lediglich in ihrer Wurzel unterscheidet. Das begründet die Plattform-Abstraktion in Abschnitt 4.2.

---

## 3. Umfang

### 3.1 In v1 enthalten

- **Mod-Verwaltung:** Import aus `.pak`, `.zip`, `.7z` (nativ) und `.rar` (über externes Werkzeug, siehe 6.1); Aktivieren und Deaktivieren; **Ladereihenfolge**; Metadaten je Mod
- **Profile:** benannte Zusammenstellungen aus Mod-Auswahl und Reihenfolge, sofort umschaltbar
- **Save-Verwaltung:** manuelles Backup, **automatisches Backup vor jedem Modded-Start**, Wiederherstellung mit Integritätsprüfung
- **Spielstart:** über Steam; optional EAC-Bypass für Offline-Modding
- **Schnellzugriffe:** Öffnen der relevanten Verzeichnisse im Dateimanager
- **Zwei Bedienoberflächen:** GUI ohne Argumente, CLI mit Argumenten — dieselbe Binary

### 3.2 Bewusst nicht in v1

| Ausgelassen | Begründung |
|---|---|
| Custom-Stratagems-Integration (`spec.md` 2.3) | Enge Kopplung an einen einzelnen Fremd-Mod (375), bricht bei dessen Updates. Der generische Profil-Mechanismus deckt den praktischen Nutzen ab. |
| Tool-Launcher (`spec.md` 2.5) | Verweist auf Integration Studio, Texmipper, Kitbash — Windows-Binaries aus dem Mod-386-Bundle, die unter Linux nicht existieren. Die Ordner-Schnellzugriffe bleiben. |
| EAC-Screen-Austausch (`spec.md` 2.4) | Kosmetik, erfordert DDS-Kompression als zusätzliche Abhängigkeit. Kandidat für v2. Der EAC-**Bypass** bleibt enthalten, da funktional relevant. |
| Windows-Build | Abstraktion wird vorbereitet (4.2), Implementierung folgt erst, wenn getestet werden kann. Kein ungetesteter Code im Release. |
| Mod-Downloads von Nexus | Erfordert API-Schlüssel und Kontoanbindung. Eigenständiges Vorhaben. |
| `.rar`-Unterstützung nativ | Die `unrar`-Crate bindet unfreien Quellcode ein — Lizenzkonflikt bei Veröffentlichung. Stattdessen: externes `unar`/`7z` aufrufen, falls vorhanden (siehe 6.1). |

---

## 4. Architektur

### 4.1 Aufteilung

```
sm2-modloader/
├── crates/
│   ├── core/            Bibliothek — kennt weder GUI noch CLI
│   │   ├── platform/    Plattformabhängige Wurzeln (4.2)
│   │   ├── paths.rs     Steam-Library, Spiel, Prefix, Saves auflösen
│   │   ├── pak_config.rs  Lesen und Schreiben von pak_config.yaml
│   │   ├── library.rs   Mod-Metadaten, Import, Deduplizierung
│   │   ├── profile.rs   Benannte Zusammenstellungen
│   │   ├── saves.rs     Backup, Wiederherstellung, Integrität
│   │   └── launch.rs    Spielstart, EAC-Bypass
│   └── app/             clap + eframe im selben Binary
└── .github/workflows/   Build, Test, Release
```

Die Trennung ist kein Selbstzweck: Pfadauflösung, YAML-Round-Trip und Backup-Integrität sind die Stellen, an denen Fehler Savegames zerstören. Ohne laufendes Fenster sind sie automatisiert testbar.

Die Binary entscheidet an einer Stelle: ohne Argumente startet `eframe`, mit Argumenten `clap`. Das hält das Versprechen einer einzelnen Datei und macht den CLI-Modus als Steam-Startoption nutzbar.

### 4.2 Plattform-Abstraktion

Die gesamte plattformabhängige Fläche:

```rust
pub trait Platform {
    /// Kandidaten für die Steam-Installationswurzel.
    fn steam_roots() -> Vec<PathBuf>;

    /// Wurzel, unter der "AppData/Local/..." liegt.
    /// Linux: <library>/steamapps/compatdata/<appid>/pfx/drive_c/users/steamuser
    /// Windows: %USERPROFILE%
    fn user_profile_root(app_id: u32, library: &Path) -> Result<PathBuf>;

    /// Spielstart über Steam.
    fn launch_via_steam(app_id: u32) -> Result<()>;

    /// Direktstart der Executable unter Umgehung von Steam (EAC-Bypass).
    /// Linux: über umu-launcher im vorhandenen Prefix.
    /// Windows: Direktaufruf.
    fn launch_direct(exe: &Path, env: &[(&str, &str)]) -> Result<()>;

    /// Verzeichnis im Dateimanager öffnen.
    fn open_folder(path: &Path) -> Result<()>;
}
```

In v1 existiert ausschließlich `unix.rs`. `windows.rs` bleibt unimplementiert; der Build für Windows wird in CI nicht erzeugt. Alles übrige — Konfiguration, Profile, Backups, Bibliothek — ist plattformneutral.

### 4.3 Abhängigkeiten

Alle Versionen am 2026-09-12 gegen crates.io geprüft.

| Zweck | Crate | Version | Begründung |
|---|---|---|---|
| GUI | `eframe` / `egui` | 0.36 | Statisch gelinkt; keine Systemabhängigkeit außer Grafiktreiber |
| CLI | `clap` | 4.6 | Standard, Derive-Makros |
| Steam-Erkennung | `steamlocate` | 2.1 | Findet Custom-Libraries, kennt Flatpak-Steam und Windows |
| YAML lesen | `yaml-rust2` | 0.13 | Aktiv gepflegt; robust gegen handgeschriebene Configs |
| Archive | `zip` | 8.6 | Mod-Zips und Save-Backups |
| Archive (7z) | `sevenz-rust2` | 0.22 | Reines Rust, keine Lizenzprobleme |
| Dateidialoge | `rfd` | 0.17 | Nutzt XDG-Portals; korrekt unter Wayland |
| XDG-Verzeichnisse | `directories` | 6.0 | Korrekte Basisverzeichnisse auf beiden Plattformen |
| Logging | `tracing` | 0.1 | Strukturierte Logs für Bug-Reports aus der Community |
| Hashing | `blake3` | 1.8 | Backup-Integrität, Mod-Deduplizierung |
| Fehler (Bibliothek) | `thiserror` | 2.0 | Typisierte Fehler mit Bedeutung für Aufrufer |
| Fehler (Anwendung) | `anyhow` | 1.0 | Kontextketten für Meldungen an Nutzer |
| Tests | `tempfile` | 3.27 | Isolierte Dateisystem-Fixtures |

**YAML wird gelesen, aber von Hand geschrieben.** Das Format hat zwei Felder. Eigene Ausgabe bedeutet byte-genaue Kontrolle über das, was die Engine zu lesen bekommt, und einen trivial testbaren Pfad. Die deprecated `serde_yaml`-Familie entfällt damit vollständig.

Bewusst **nicht** enthalten: kein `tokio` — Hintergrundarbeit ist ein `std::thread` mit Channel, kein Async-Runtime. Kein `unrar` (Lizenz, siehe 3.2).

---

## 5. Datenmodell

### 5.1 Ablageorte

```
~/.config/sm2-modloader/
└── settings.toml              Nutzereinstellungen, von Hand editierbar

~/.local/share/sm2-modloader/
├── library.json               Mod-Metadaten (maschinenverwaltet)
├── profiles/<name>.toml       Benannte Zusammenstellungen
└── backups/saves/
    ├── <zeitstempel>[_<tag>].zip
    └── <zeitstempel>[_<tag>].json    Manifest mit blake3-Hashes

~/.local/state/sm2-modloader/logs/
```

Die eigentlichen `.pak`-Dateien verbleiben **ausschließlich** in `<game>/client_pc/root/mods/`. Der Loader hält keine Kopien.

### 5.2 `pak_config.yaml` — die maßgebliche Quelle

Liegt in `<game>/client_pc/root/mods/` und gehört der Engine. Der Loader liest und schreibt sie, erfindet aber kein eigenes Format.

Die Liste ist geordnet; die Reihenfolge *ist* die Ladereihenfolge. Beim Schreiben werden **alle** vorhandenen `.pak`-Dateien aufgeführt — auch deaktivierte (siehe 2.1).

Geschrieben wird **atomar**: temporäre Datei im selben Verzeichnis, `fsync`, dann `rename`. Ein Absturz mitten im Schreiben kann so keine unvollständige Konfiguration hinterlassen.

### 5.3 `library.json`

Metadaten zu jedem bekannten Mod, verknüpft über den Dateinamen des Paks:

- Anzeigename, Autor, Version, Nexus-ID, Notizen — soweit ermittelbar oder vom Nutzer gepflegt
- blake3-Hash und Größe des Paks, Zeitpunkt des Imports
- Herkunft: Pfad des Quellarchivs

Der Hash dient der Deduplizierung beim Import und erkennt Mods, die außerhalb des Loaders ersetzt wurden.

### 5.4 Profile

Ein Profil ist eine geordnete Liste von Pak-Dateinamen mit Aktivierungszustand — die exakte Information, die `pak_config.yaml` benötigt. Ein Profil anzuwenden heißt: Datei schreiben. Dadurch ersetzt ein Profil die Backup-Ordner-Sammlung von Mod 386 zum Preis von wenigen hundert Bytes.

Fehlt ein im Profil genannter Mod im Verzeichnis, wird gewarnt und der Eintrag übersprungen; das Profil bleibt unverändert erhalten.

---

## 6. Kernabläufe

### 6.1 Mod importieren

1. Nutzer wählt eine Datei (`.pak`, `.zip`, `.7z`, `.rar`).
2. Archive werden in ein temporäres Verzeichnis entpackt. Bei `.rar`: Suche nach `unar` oder `7z` im `PATH`; fehlen beide, erscheint eine Meldung mit dem Paketnamen für gängige Distributionen — kein stilles Scheitern.
3. Enthaltene `.pak`-Dateien werden ermittelt. Enthält ein Archiv mehrere, wählt der Nutzer aus.
4. Grundprüfung der Pak-Struktur; blake3-Hash gegen `library.json` zur Dublettenerkennung.
5. Verschieben nach `<game>/client_pc/root/mods/`, Eintrag in `library.json`.
6. Ergänzung in `pak_config.yaml` als **deaktiviert** ans Ende. Ein Import verändert damit nie ein laufendes Setup.

### 6.2 Aktivieren, deaktivieren, umsortieren

Alle drei Vorgänge sind derselbe: Zustand im Speicher ändern, `pak_config.yaml` atomar neu schreiben. Keine Dateibewegung, keine Größenabhängigkeit.

### 6.3 Abgleich beim Start

Bei jedem Start vergleicht der Loader Verzeichnisinhalt und Konfiguration:

- Pak vorhanden, nicht in der Konfiguration → wird ergänzt (aktiv, ans Ende), da es ohnehin geladen würde und andernfalls die Reihenfolge unterliefe (2.1)
- In der Konfiguration, aber nicht vorhanden → Eintrag entfällt, Hinweis an den Nutzer
- Hash weicht von `library.json` ab → als „außerhalb verändert" markiert

Das macht den Loader verträglich mit manuellen Eingriffen und mit Mod 386, falls jemand beide benutzt.

### 6.4 Spiel starten

**Modded:** Falls automatisches Backup aktiv (Standard), zuerst Save-Backup. Dann `pak_config.yaml` schreiben, dann Start über Steam.

**Vanilla:** Identisch, nur mit einer Konfiguration, in der alles deaktiviert ist. Der zuvor aktive Zustand wird als impliziter Wiederherstellungspunkt gemerkt.

**EAC-Bypass:** Direktstart der Executable im vorhandenen Proton-Prefix über `umu-launcher`. Fehlt es, erklärt der Loader, was es ist und wie es installiert wird, statt einen unklaren Fehler zu zeigen. Vor dem Start ein deutlicher Hinweis auf die Multiplayer-Einschränkung.

### 6.5 Savegames sichern und wiederherstellen

**Sichern:** Save-Verzeichnis wird ermittelt (2.3), als ZIP mit Zeitstempel und optionalem Etikett abgelegt. Parallel entsteht ein Manifest mit blake3-Hash je Datei.

**Wiederherstellen:** Manifest wird gegen das Archiv geprüft. Vor dem Überschreiben legt der Loader **immer** ein Sicherungs-Backup des aktuellen Stands an — auch ohne Rückfrage, da es nichts kostet. Erst danach wird entpackt.

**Steam-Cloud:** Läuft Steam während der Wiederherstellung, kann die Cloud-Synchronisation den zurückgespielten Stand überschreiben. Der Loader prüft, ob ein Steam-Prozess läuft, und verlangt in diesem Fall eine ausdrückliche Bestätigung mit Erklärung. Dies ist der wahrscheinlichste Weg zu Datenverlust und wird entsprechend behandelt.

---

## 7. Fehlerbehandlung

Leitlinie: **Kein Vorgang, der Nutzerdaten berührt, darf teilweise ausgeführt bleiben.**

- Schreibvorgänge an `pak_config.yaml` und an Konfigurationsdateien sind atomar (temp + `rename`).
- Vor jeder Wiederherstellung entsteht ein Sicherungs-Backup (6.5).
- Fehlende Schreibrechte im Spielverzeichnis werden beim Start geprüft und gemeldet, nicht erst beim Speichern.
- `core` liefert typisierte Fehler (`thiserror`); die Anwendungsschicht reichert sie mit `anyhow`-Kontext an. Jede Meldung an den Nutzer nennt, **was** nicht ging, **warum**, und **was zu tun ist**.
- Alle Vorgänge werden protokolliert. Der Pfad zur Logdatei ist in der GUI sichtbar, damit Bug-Reports brauchbar sind.

---

## 8. Teststrategie

| Ebene | Gegenstand |
|---|---|
| Unit | Pfadauflösung gegen synthetische Steam-Verzeichnisbäume in `tempfile`-Fixtures |
| Golden | `pak_config.yaml`: Round-Trip echter Beispiele; Ausgabe byte-genau gegen erwartete Dateien |
| Unit | Reihenfolge-Semantik: nicht aufgeführte Paks laden zuerst (2.1) — als Eigenschaft der Abgleichslogik |
| Integration | Import → Aktivierung → Profilwechsel → Backup → Wiederherstellung auf einem Fixture-Verzeichnisbaum |
| Integration | Backup/Restore-Round-Trip mit Hash-Vergleich; absichtlich beschädigtes Archiv wird abgewiesen |
| Manuell | **T0** (siehe 9, R1) und ein Spielstart je Startvariante |

`core` ist ohne Spielinstallation vollständig testbar. Die GUI wird nicht automatisiert getestet; sie enthält keine Logik, die nicht in `core` liegt.

---

## 9. Risiken

**R1 — Die Engine könnte `disabled: true` ignorieren.** Das Verhalten ist dokumentiert, aber nicht verifiziert. Trifft es nicht zu, bricht das gesamte Aktivierungsmodell.
*Gegenmaßnahme:* **T0, vor jeder Implementierung.** Ein Spielstart mit auf `disabled: true` gesetztem Astartes-Mod. Schlägt der Test fehl, greift ein Umschalten per Umbenennung innerhalb desselben Dateisystems (`mods/` ↔ `mods_disabled/` auf demselben Gerät) — `rename` statt Kopie, ebenfalls sofort. Nur `pak_config.rs` und `library.rs` wären betroffen; die übrige Architektur bleibt gültig.

**R2 — Steam-Cloud überschreibt wiederhergestellte Saves.** Behandelt in 6.5.

**R3 — Spiel-Updates setzen den Mods-Ordner zurück.** Steam kann bei Updates unbekannte Dateien entfernen. Der Loader erkennt fehlende Paks beim Abgleich (6.3) und meldet sie namentlich, damit der Nutzer weiß, was neu zu installieren ist.

**R4 — `.rar`-Archive ohne externes Werkzeug.** Behandelt in 6.1.

**R5 — `umu-launcher` nicht installiert.** Betrifft nur den EAC-Bypass. Behandelt in 6.4.

---

## 10. Verteilung

- **Primär:** einzelne Binary, `x86_64-unknown-linux-gnu`. Gebaut mit `cargo-zigbuild` gegen glibc 2.31, damit sie auf allem ab Ubuntu 20.04 und Debian 11 läuft — einschließlich SteamOS. Veröffentlicht über GitHub Releases und Nexus Mods.
- **CI:** GitHub Actions — Tests bei jedem Push, Release-Artefakt bei Tags.
- **Später:** AppImage und Flatpak, sobald v1 stabil ist. Beide ändern nichts am Aufbau.

---

## 11. Reihenfolge der Umsetzung

Die Details entstehen im Implementierungsplan. Die Abfolge steht durch die Risiken bereits fest:

1. **T0** — R1 verifizieren. Alles Weitere hängt daran.
2. `core`: Pfadauflösung und Plattform-Trait — ohne diese Grundlage funktioniert nichts.
3. `core`: `pak_config` lesen, schreiben, abgleichen — das Herzstück.
4. `core`: Bibliothek und Import.
5. `core`: Saves — Sicherung, Wiederherstellung, Integrität.
6. `core`: Profile und Spielstart.
7. CLI — macht den Kern nutzbar und dient als Testoberfläche.
8. GUI.
9. CI und Release.

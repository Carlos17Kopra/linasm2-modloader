# Space Marine 2 Native Linux Mod Loader — Spezifikation

Basierend auf dem Funktionsumfang des Windows-Mod-Loaders ([Mod 386](https://www.nexusmods.com/warhammer40000spacemarine2/mods/386)) erstellt.

---

## 1. Übersicht

Der Loader ist eine native Linux-Anwendung (GUI + CLI), die den vollen Funktionsumfang des Windows-Mod-Loaders nachbildet: Mod-Management, Save-Backup/Restore, Custom Stratagems-Integration und EAC-Screen-Anpassung – alles ohne Proton/Wine.

---

## 2. Kernfunktionen

### 2.1 Mod-Management

**Install Mod:**
- Öffnet Dateidialog zur Auswahl von `.pak`-Dateien (oder `.zip`/`.rar` mit extrahierten `.pak`-Dateien)
- Kopiert/verschiebt die `.pak`-Datei in das Loader-eigene Mods-Verzeichnis (`~/.local/share/sm2-mod-loader/mods/`)
- Extrahiert automatisch komprimierte Archive (ZIP/RAR) vor dem Import
- Validiert `.pak`-Struktur (prüft auf gültige SM2-Mod-Struktur)

**Mod-Liste & Aktivierung:**
- Zeigt alle importierten Mods mit Checkbox zur Aktivierung/Deaktivierung
- Speichert Aktivierungsstatus in Konfigurationsdatei (`mods_config.json`)
- Mods bleiben im Loader-Verzeichnis, wenn inaktiv

**Play Modded:**
- Verschiebt/kopiert aktivierte `.pak`-Dateien nach `<game_dir>/client_pc/root/mods/`
- Startet das Spiel mit korrekten Launch-Optionen (für EAC-Bypass bei Bedarf)
- Deaktivierte Mods werden aus dem `mods`-Ordner entfernt

**Play Vanilla:**
- Entfernt alle `.pak`-Dateien aus `<game_dir>/client_pc/root/mods/`
- Startet das Spiel ohne Mods

### 2.2 Save-Management

**Backup Save:**
- Erstellt Backup der Save-Dateien aus:
  - Steam: `~/.local/share/Steam/steamapps/compatdata/2183900/pfx/drive_c/users/steamuser/AppData/Local/SpaceMarine2/Saved/SaveGames/`
  - Epic: `~/.wine/drive_c/users/$USER/AppData/Local/SpaceMarine2/Saved/SaveGames/`
- Speichert Backups im Loader-Verzeichnis unter `backups/saves/` mit Timestamp
- Komprimiert Backup als `.zip` für Platzersparnis

**Restore Save:**
- Stellt das letzte Backup (oder ausgewähltes Backup) wieder her
- Validiert Backup-Integrität vor Wiederherstellung
- Warnt vor Überschreiben aktueller Saves

### 2.3 Custom Stratagems

**Integration:**
- Bietet direkten Zugriff auf Custom Stratagems-Mod (Mod 375)
- Öffnet Stratagems-Konfigurator im Spiel über Quick-Launch
- Speichert bevorzugte Stratagem-Presets im Loader

**Stratagem-Manager:**
- Listet verfügbare Custom Stratagems auf
- Ermöglicht Erstellen/Laden von Stratagem-Profilen (Mission + Buffs/Debuffs)
- Startet Spiel mit vordefiniertem Stratagem-Profil

### 2.4 EAC-Screen-Anpassung

**Change EAC Screen:**
- Ermöglicht Austauschen des EAC-Ladebildschirms (Custom Images)
- Zielverzeichnis: `<game_dir>/client_pc/root/bin/pc/`
- Unterstützte Formate: `.png`, `.jpg`, `.dds` (konvertiert bei Bedarf)
- Erstellt Backup des Originals vor Änderung

**EAC-Bypass-Option:**
- Optional: Startet Spiel ohne EAC für Offline-Modding (via `.bat`-Äquivalent als Shell-Skript)
- Warnt vor Multiplayer-Einschränkungen

### 2.5 Quick-Access zu Modding-Tools

**Tool-Shortcuts:**
- Öffnet relevante Verzeichnisse:
  - Game Root: `<game_dir>/`
  - Mods Folder: `<game_dir>/client_pc/root/mods/`
  - Local Folder: `<game_dir>/client_pc/root/local/`
  - Save Games: (siehe 2.2)
- Startet externe Tools:
  - Integration Studio (falls installiert)
  - 7-Zip/Archive Manager
  - Text-Editor für Konfigs

---

## 3. Technische Architektur

### 3.1 Verzeichnisstruktur

```
~/.local/share/sm2-mod-loader/
├── mods/                    # Importierte .pak-Dateien
├── backups/
│   └── saves/              # Save-Backups (timestamped .zip)
├── config/
│   ├── mods_config.json    # Mod-Aktivierungsstatus
│   └── settings.json       # Loader-Einstellungen
├── eac_screens/            # Custom EAC-Bilder
└── logs/                   # Log-Dateien
```

### 3.2 Game-Directory-Erkennung

**Automatische Erkennung:**
- Steam: `~/.local/share/Steam/steamapps/common/Space Marine 2/`
- Steam (Custom): Durchsucht `~/.steam/` und `~/.local/share/Steam/`
- Epic: `~/.wine/drive_c/Program Files (x86)/Epic Games/SpaceMarine2/`
- Manuelle Konfiguration über Settings

### 3.3 Launch-Optionen

**Für Modded-Start:**
```bash
# Standard mit EAC
steam -applaunch 2183900

# Ohne EAC (Offline-Modding)
cd "<game_dir>/client_pc/root/bin/pc/"
SteamAppId=2183900 SteamGameId=2183900 "./Warhammer 40000 Space Marine 2 - Retail.exe"
```

**Proton-Kompatibilität (falls nötig):**
- Empfohlen: GE-Proton10-29 oder Proton Experimental
- Optional: `SteamDeck=1 %command%` für Linux-Workaround

---

## 4. GUI-Spezifikation

**Hauptfenster:**
- Mod-Liste (Tabelle mit Checkbox, Name, Version, Autor)
- Buttons: `Install Mod`, `Play Modded`, `Play Vanilla`
- Sidebar: `Backup Save`, `Restore Save`, `EAC Screen`, `Stratagems`, `Tools`

**Dialoge:**
- Dateiauswahl für Mod-Installation
- Backup-Auswahl für Restore
- EAC-Screen-Browser mit Vorschau
- Stratagem-Profil-Editor

**Status-Anzeige:**
- Aktiver Mod-Status (Anzahl aktiver Mods)
- Letztes Backup-Datum
- Game-Directory-Pfad

---

## 5. Implementierungs-Empfehlungen

**Sprache/Framework:**
- Python + PyQt6/PySide6 (plattformunabhängig, gute Linux-Unterstützung)
- Alternativ: Rust + GTK4/Relm4 für native Performance
- CLI-Modus für Skript-Automatisierung

**Abhängigkeiten:**
- `python3`, `python3-pip`
- `PyQt6` oder `PySide6`
- `zipfile` (Standardbibliothek für Backups)
- `requests` (für Mod-Updates, optional)

**Packaging:**
- Flatpak für einfache Distribution
- AppImage für portable Nutzung
- AUR-Paket für Arch/Manjaro

---

## 6. Besondere Linux-Hinweise

**EAC-Problematik:**
- EAC funktioniert auf Linux/Proton nicht zuverlässig (Mods Detected-Fehler)
- Loader sollte EAC-Bypass als Standard für Modding anbieten
- Multiplayer nur mit allen Spielern auf gleichen Mods (Private Matches)

**Dateisystem:**
- Linux ist case-sensitive (Pfade genau beachten)
- Wine-Pfade für Saves korrekt auflösen
- Berechtigungen für Game-Directory prüfen

**Proton-Konfiguration:**
- `kernel.split_lock_mitigate=0` für bessere Performance
- DLSS3/FSR3.1 nur mit Proton Experimental/9.0-4+

---

## 7. Beispiel-Code-Skelett (Python)

```python
#!/usr/bin/env python3
"""
Space Marine 2 Native Linux Mod Loader
Spezifikation-Implementierungsskelett
"""

import os
import json
import shutil
from pathlib import Path
from PyQt6.QtWidgets import QApplication, QMainWindow, QPushButton, QListWidget

class SM2ModLoader(QMainWindow):
    def __init__(self):
        super().__init__()
        self.game_dir = self.detect_game_directory()
        self.mods_dir = Path.home() / ".local/share/sm2-mod-loader/mods"
        self.config_file = Path.home() / ".local/share/sm2-mod-loader/config/mods_config.json"

    def detect_game_directory(self) -> Path:
        steam_path = Path.home() / ".local/share/Steam/steamapps/common/Space Marine 2"
        if steam_path.exists():
            return steam_path
        # Fallback: Epic, manuelle Konfiguration
        raise FileNotFoundError("Game directory not found")

    def install_mod(self, pak_file: Path):
        shutil.copy(pak_file, self.mods_dir / pak_file.name)

    def activate_mods(self):
        target_mods = self.game_dir / "client_pc/root/mods"
        target_mods.mkdir(parents=True, exist_ok=True)

        # Lade config
        with open(self.config_file) as f:
            config = json.load(f)

        for mod in config["active_mods"]:
            shutil.copy(self.mods_dir / mod, target_mods / mod)

    def backup_saves(self):
        save_dir = Path.home() / ".local/share/Steam/steamapps/compatdata/2183900/pfx/drive_c/users/steamuser/AppData/Local/SpaceMarine2/Saved/SaveGames"
        backup_dir = Path.home() / ".local/share/sm2-mod-loader/backups/saves"
        backup_dir.mkdir(parents=True, exist_ok=True)

        # ZIP-Backup erstellen
        shutil.make_archive(backup_dir / f"save_backup_{timestamp}", 'zip', save_dir)
```

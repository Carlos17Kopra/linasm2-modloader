# Übergabe: GUI-Entwurf für LiNa SM2 - Mod Launcher

Dieses Dokument ist die Grundlage für den Entwurf der grafischen Oberfläche. Die
Logik darunter ist fertig, getestet und ändert sich nicht mehr — was hier steht,
ist verbindlich, nicht vorläufig.

Wer entwirft, sollte zwei Abschnitte zuerst lesen: **Der harte Rahmen** (was die
Technik erlaubt) und **Fachliche Regeln, die man falsch entwerfen kann**. Der Rest
ist Bestandsaufnahme.

---

## 1. Was die Anwendung ist

Ein nativer Linux-Mod-Loader für *Warhammer 40 000: Space Marine 2*. Das Spiel
läuft unter Proton; die Mods sind einzelne `.pak`-Dateien von teils mehreren
Gigabyte. Zielgruppe: Spieler, die Mods von Nexus laden und auf Linux verwalten
wollen. Eine Veröffentlichung an die Linux-Community ist geplant.

**Die Oberfläche ist auf Deutsch.** Sämtliche Beschriftungen, Meldungen und
Hilfetexte. (Im Quelltext sind nur Bezeichner englisch — das betrifft den Entwurf
nicht.)

---

## 2. Der harte Rahmen

**Technik: Rust mit `egui`/`eframe`, eine einzige Binary ohne Laufzeitabhängigkeiten.**

Das ist keine Web-Oberfläche. `egui` ist ein *Immediate-Mode*-Toolkit, und das
schließt eine Reihe verbreiteter Entwurfsmuster praktisch aus:

- **Kein freies CSS-Layout.** Es gibt Panels (oben/unten/links/rechts/zentral),
  Fenster, `ScrollArea`, horizontale und vertikale Reihen, Grids. Kein Flexbox,
  kein Grid-Template, keine absolute Positionierung nach Belieben.
- **Animationen und Übergänge sind teuer und unüblich.** Ein Entwurf, der von
  weichen Übergängen, Parallax oder aufwendigem Hover-Verhalten lebt, ist hier
  nicht umsetzbar. Zustandswechsel sind sofort.
- **Drag & Drop ist möglich, aber Handarbeit.** Für die Ladereihenfolge ist es
  die naheliegende Geste — bitte trotzdem eine Tastatur- oder Knopf-Alternative
  (hoch/runter) vorsehen, nicht nur Ziehen.
- **Typografie und Icons sind begrenzt.** Eine Schriftart, wenige Größen. Icons
  brauchen eine eingebettete Icon-Schrift oder mitgelieferte Bilder; ein
  Entwurf, der 40 verschiedene Symbole braucht, kostet spürbar.
- **Es gibt kein natives Modal-System.** Bestätigungen sind `egui`-Fenster, die
  man selbst als modal behandelt.
- **Dateidialoge** laufen über `rfd` (nativer GTK/Portal-Dialog) — die sehen aus
  wie das System, nicht wie die App.

Was gut geht: dichte Listen und Tabellen, sofortiges Filtern, Panels mit fester
Rollenverteilung, Fortschrittsbalken, farbige Zustandsmarker, Tooltips.

**Fenstergröße:** Desktop, frei skalierbar. Ein sinnvoller Startwert ist etwa
1100 × 700. Es gibt keine Mobilansicht.

**Dunkel und hell:** `egui` bringt beides mit. Der Entwurf sollte in beiden
funktionieren und Farbe nie als einzigen Bedeutungsträger verwenden.

---

## 3. Die zentralen Objekte

**Mod (Pak).** Eine `.pak`-Datei im Mods-Verzeichnis des Spiels. Bekannt sind:
Dateiname, Anzeigename, optional Autor/Version/Nexus-ID/Notizen, Hash, Größe,
Importzeitpunkt, Herkunft (Archivpfad). Mods werden nach dem Import **nie mehr
verschoben oder umbenannt**.

**Aktivierung und Ladereihenfolge** stehen ausschließlich in `pak_config.yaml` —
einer Datei, die der Engine gehört und die auch andere Werkzeuge schreiben. Sie
ist die einzige Wahrheit. Die Liste ist geordnet; die Position bestimmt, welcher
Mod bei einem Konflikt gewinnt.

**Profil.** Ein benannter Schnappschuss von Aktivierung *und* Reihenfolge.
Speichern, anwenden, löschen, auflisten.

**Savegame-Backup.** Ein ZIP plus Manifest mit Hash je Datei, im Datenverzeichnis
des Loaders. Hat Zeitstempel und optional ein Etikett.

**Einstellungen.** Spielverzeichnis (überschreibt die Erkennung), automatisches
Backup vor dem Start (Standard: an), Steam-Nutzerprofil (nötig, wenn mehrere im
Prefix liegen).

---

## 4. Fachliche Regeln, die man falsch entwerfen kann

Diese vier Punkte sind der Grund, warum dieses Dokument existiert. Ein Entwurf,
der sie ignoriert, sieht gut aus und ist falsch.

### 4.1 Ein Mod, der im Ordner liegt, lädt — ob er in der Liste steht oder nicht

Die Engine lädt jede vorhandene `.pak`, und zwar **zuerst und ungesteuert**, wenn
sie nicht in `pak_config.yaml` steht. Daraus folgt: Jedes vorhandene Pak *muss*
in der Liste auftauchen. „Nicht in der Liste" ist kein neutraler Zustand, sondern
der gefährlichste.

Für die Oberfläche heißt das: Es gibt kein „unverwaltet" als Anzeigekategorie zum
Danebenstehenlassen. Wenn der Loader beim Start ein fremdes Pak findet, nimmt er
es **aktiviert** auf (es lädt ohnehin — also lieber steuerbar) und sagt das.

### 4.2 Ein Import aktiviert nichts

Genau umgekehrt: Ein selbst importierter Mod landet **deaktiviert** am Ende der
Liste. Ein Import darf ein laufendes, funktionierendes Setup nie verändern.

Diese beiden Regeln widersprechen sich scheinbar und sind beide beabsichtigt.
Der Unterschied: Bei 4.1 ist die Datei schon da und wirkt bereits; bei 4.2
entscheidet der Nutzer gerade selbst.

Der Entwurf sollte den frisch importierten Mod deshalb sichtbar machen — er ist
neu, deaktiviert, ganz unten, und der Nutzer will ihn vermutlich als Nächstes
einschalten.

### 4.3 Wiederherstellen ist gefährlich, und Steam macht es gefährlicher

Ein Savegame-Backup zurückzuspielen überschreibt echte Spielstände. Die Logik
sichert davor **immer** automatisch den aktuellen Stand — das ist keine Option,
die man abschalten kann, und der Nutzer soll erfahren, dass es passiert ist.

Läuft Steam, kann die Cloud-Synchronisation die Wiederherstellung überschreiben.
Der Loader warnt und verlangt eine ausdrückliche Bestätigung; er verweigert nicht
stumpf, weil es Fälle gibt (hängender Prozess, Container), in denen die Erkennung
irrt.

Ein Entwurf, der Wiederherstellen wie eine gewöhnliche Listenaktion behandelt, ist
falsch. Ein Entwurf, der es hinter drei Bestätigungen versteckt, auch — dann
klickt der Nutzer im Ernstfall blind durch.

### 4.4 Vanilla-Start räumt die Aktivierung ab

„Ohne Mods starten" schaltet alle Mods aus. Damit die Auswahl nicht verloren ist,
legt der Loader vorher automatisch ein Profil mit Zeitstempel an und nennt dessen
Namen. Die Oberfläche muss diesen Namen zeigen und den Rückweg (Profil anwenden)
nahelegen — sonst ist die Sicherung zwar da, aber unauffindbar.

---

## 5. Funktionsumfang, der abgedeckt sein muss

Alles hier existiert und ist in der Kommandozeile bereits nutzbar. Die GUI muss
es erreichbar machen — nicht zwingend eins zu eins.

### Mods
- Liste aller Mods **in Ladereihenfolge**, mit Aktivierungszustand
- Einzelne aktivieren / deaktivieren
- Reihenfolge ändern (Ziehen und/oder hoch/runter)
- Importieren aus `.pak`, `.zip`, `.7z`, `.rar` — mehrere Dateien auf einmal,
  Dateidialog **und** Ablegen per Drag & Drop aufs Fenster
- Dubletten werden am Inhalt erkannt und gemeldet, statt doppelt zu importieren
- Metadaten je Mod: Name, Autor, Version, Nexus-ID, Notizen, Größe, importiert am,
  Herkunft
- Hinweis, wenn ein Mod **außerhalb des Loaders verändert** wurde (der Hash weicht ab)
- Hinweis, wenn Einträge **fehlen** (Datei weg — etwa nach einem Steam-Update) und
  wenn ein verschwundener Mod **zurückgekehrt** ist und in seinen alten Zustand
  zurückgestellt wurde

### Profile
- Auflisten, aktuellen Zustand als Profil speichern, anwenden, löschen
- Beim Anwenden: Mods, die das Profil kennt, die aber fehlen, werden gemeldet und
  übersprungen — das Profil bleibt unverändert
- Automatisch angelegte Vanilla-Schnappschüsse erscheinen in derselben Liste

### Savegames
- Backup anlegen, optional mit Etikett
- Backups auflisten (neueste zuerst) mit Zeitstempel, Etikett, Größe
- Backup prüfen (Hash je Datei gegen das Manifest)
- Wiederherstellen — mit dem automatischen Sicherungs-Backup davor, siehe 4.3

### Spielstart
- Regulär über Steam
- Ohne Mods (Vanilla), siehe 4.4
- Ohne EAC (kein Multiplayer) — **nur anbieten, wenn verfügbar**: die Option hängt
  an einem externen Werkzeug (`umu-run`), das oft fehlt. Fehlt es, blenden wir die
  Option aus, statt sie mit einem Fehler quittieren zu lassen.
- Optional automatisches Savegame-Backup vor dem Start (Einstellung, Standard an)

### Verzeichnisse und Einstellungen
- Erkannte Pfade zeigen: Spiel, Mods, Savegames, Backups
- Jedes davon im Dateimanager öffnen
- Spielverzeichnis manuell setzen, wenn die Erkennung scheitert
- Steam-Nutzerprofil wählen, wenn mehrere im Prefix liegen
- Automatisches Backup an/aus

---

## 6. Zustände, die der Entwurf abfangen muss

Das sind keine Randfälle — jeder davon tritt bei echten Nutzern auf, und jeder
braucht eine eigene Antwort in der Oberfläche.

| Zustand | Was der Nutzer sehen soll |
|---|---|
| Spiel nicht gefunden | Die Anwendung startet trotzdem. Aufforderung, das Verzeichnis zu wählen — nicht eine leere Liste. |
| Keine Mods installiert | Erstbenutzung. Erklären, wie man importiert; nicht nur eine leere Tabelle. |
| Mods-Verzeichnis schreibgeschützt | Lesen geht, Ändern nicht. Die Oberfläche darf nicht so tun, als ginge alles, und erst beim Speichern scheitern. |
| Mehrere Steam-Nutzerprofile | Auswahl anbieten. Ohne Auswahl sind alle Savegame-Funktionen blockiert. |
| `umu-run` fehlt | „Ohne EAC" ausblenden. Kein Fehler. |
| Import läuft | Paks sind mehrere GB groß, Entpacken dauert spürbar. Fortschritt und ein arbeitsfähiges Fenster, kein eingefrorenes. |
| Backup läuft / wird geprüft | Ebenso. |
| Steam läuft beim Wiederherstellen | Warnung mit Begründung und ausdrücklicher Bestätigung, siehe 4.3. |
| Archiv enthält kein Pak / ist beschädigt | Klare Meldung, welche Datei und warum. |
| `.rar` ohne passendes Werkzeug | Sagen, welches Programm fehlt. |
| Konfiguration von außen verändert | Ein anderes Werkzeug oder der Nutzer hat `pak_config.yaml` angefasst. Der Loader gleicht ab und meldet, was sich geändert hat. |

---

## 7. Was der Entwurf **nicht** enthalten soll

- **Keine Nexus-Anbindung**, kein Download aus der Anwendung, kein Login. Der
  Nutzer lädt selbst und importiert die Datei.
- **Keine Konfliktanalyse** zwischen Mods (welcher überschreibt welche Datei) —
  ausdrücklich nicht in diesem Umfang.
- **Kein Werkzeug-Starter**, keine Stratagems-Integration, kein Austausch des
  EAC-Bildschirms. Bewusst gestrichen.
- **Keine Mehrsprachigkeit.** Nur Deutsch.
- **Keine Kontoverwaltung, keine Telemetrie, kein Update-Check.**

---

## 8. Was ich vom Entwurf brauche

1. **Grundaufteilung des Fensters.** Welche Panels, welche Rolle, was ist immer
   sichtbar. Der Mod-Liste gehört der meiste Platz.
2. **Die Mod-Liste im Detail.** Sie ist die Anwendung. Wie sehen Zeile,
   Aktivierungsschalter, Position, Statusmarker (verändert / fehlt / neu) und die
   Auswahl aus? Wie ändert man die Reihenfolge?
3. **Wie Profile und Savegames erreichbar sind** — eigener Bereich, Reiter,
   Seitenleiste? Beides wird selten benutzt, muss aber im Ernstfall sofort
   auffindbar sein.
4. **Der Startknopf und seine Varianten.** Regulär, Vanilla, ohne EAC — wobei die
   dritte manchmal fehlt.
5. **Wie Warnungen und Bestätigungen aussehen**, besonders die aus 4.3.
6. **Der Erstlauf**, wenn nichts erkannt wurde und nichts installiert ist.

Skizzen, Wireframes oder eine Beschreibung in Worten sind alle recht — nur bitte
gegen den Rahmen aus Abschnitt 2 geprüft.

---

## 9. Verweise

- Entwurfsspezifikation: `docs/superpowers/specs/2026-09-12-sm2-modloader-design.md`
- Umsetzungsplan Kern und Kommandozeile: `docs/superpowers/plans/2026-09-12-core-und-cli.md`
- Kommandozeile (zeigt den vollen Funktionsumfang in Benutzung): `crates/app/src/cli.rs`
- Fachlogik: `crates/core/src/` — besonders `pak_config.rs` (Aktivierungsmodell),
  `import.rs`, `saves.rs`, `profile.rs`

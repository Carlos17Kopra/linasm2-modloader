# Die Oberfläche, wie sie gebaut wurde

Vorlage ist `docs/design/SM2 Mod Loader GUI v2 modern.dc.html` – der Entwurf
aus Claude Design, der zusammen mit `docs/GUI-HANDOVER.md` die Grundlage
bildet. Dieses Dokument hält fest, wo die Umsetzung dem Entwurf folgt, wo sie
ihn ergänzen musste und wo sie bewusst abweicht.

## Wo der Code steht

| Datei | Inhalt |
|---|---|
| `crates/app/src/gui/theme.rs` | Farben, Schriften, Abstände; IBM Plex in drei Schnitten |
| `crates/app/src/gui/icons.rs` | Alle Symbole, mit dem Painter gezeichnet |
| `crates/app/src/gui/widgets.rs` | Schalter, Statuspillen, Schaltflächen, Eingabefelder, Tabellenspalten |
| `crates/app/src/gui/mod.rs` | Zustand, Aktionen, Fensteraufteilung |
| `crates/app/src/gui/top_bar.rs` | Kopfleiste mit Startauswahl |
| `crates/app/src/gui/side_bar.rs` | Navigation und Zustandskarte |
| `crates/app/src/gui/mods_page.rs` | Hinweisleiste, Mod-Liste, Detailkarte, Erstlauf-Bildschirm |
| `crates/app/src/gui/profiles_page.rs` | Profile |
| `crates/app/src/gui/saves_page.rs` | Savegame-Backups |
| `crates/app/src/gui/settings_page.rs` | Verzeichnisse und Verhalten |
| `crates/app/src/gui/dialogs.rs` | Die vier modalen Fenster |
| `crates/app/src/gui/tasks.rs` | Import, Backup, Prüfen, Wiederherstellen im Hintergrund |
| `crates/app/src/gui/commands.rs` | Starten, Profile, Einstellungen |
| `crates/app/src/gui/format.rs` | Größen, Zeitstempel, gekürzte Hashes |

Eine einzige Binary: ohne Argumente startet `lina-sm2` die Oberfläche,
mit Argumenten die Kommandozeile.

## Was eins zu eins übernommen ist

Fenstermaß 1100 × 700, Kopfleiste 58 px, Seitenleiste 208 px, Statusleiste
30 px. Sämtliche Farbwerte, Schriftgrade, Rundungen, Zeilenhöhen und
Spaltenbreiten stehen als benannte Konstanten im Quelltext und tragen
dieselben Zahlen wie der Entwurf. Alle vier Bereiche, alle vier Dialoge, die
Hinweisleiste, die Detailkarte, der Erstlauf-Bildschirm und der leere
Zustand sind vorhanden.

**Texte kommen aus dem Katalog, nicht mehr aus dem Quelltext.** Was der
Entwurf als feste Beschriftung zeigt – Statusmerker wie „ÜBERNOMMEN" und
„GEÄNDERT", die Zustandskarte, jede Meldung in der Hinweisleiste – steht
seit der Mehrsprachigkeit in `crates/core/i18n/{en,de}.toml` und wird über
`t!` nachgeschlagen. Die Oberfläche zeigt den Text in der eingestellten
Sprache; die oben genannten Spaltenbreiten sind deshalb nicht auf eine der
beiden Fassungen allein zugeschnitten, sondern auf die jeweils längere von
beiden.

## Ergänzungen, die der Entwurf offengelassen hat

**Symbole sind gezeichnet, nicht gesetzt.** Der Entwurf benutzt Zeichen wie
`⠿`, `⏷`, `▣`, `⚙`. Keines davon steht in IBM Plex; welche Rückfallschrift
einspringt, entscheidet das jeweilige System. Auf einem fremden Rechner
hinge das Aussehen also vom Zufall ab. Alle Symbole zeichnet deshalb
`icons.rs` mit `ui.painter()` – der Weg, den der Entwurf in seinen eigenen
Anmerkungen ohnehin für Schalter und Pillen vorschlägt.

**Die eigene Fenstertitelleiste fehlt.** Der Entwurf hält in seiner
Schlussbemerkung selbst fest, dass sie Dekoration der Skizze ist und das
echte Fenster Systemdekoration bekommt.

**Kein heller Anstrich.** Der Entwurf ist durchgehend dunkel; es gibt keine
helle Vorlage, die sich übernehmen ließe. Beide Erscheinungsbilder von
`egui` bekommen denselben Stil, damit das Aussehen nicht davon abhängt, was
der Desktop gerade meldet.

**Fortschritt zählt Dateien, nicht Bytes.** Der Entwurf zeigt im Beispiel
„1,4 GB von 2,1 GB“. Weder `zip` noch `sevenz-rust2` melden beim Entpacken
einen Byte-Fortschritt, und dafür einen eigenen Leser dazwischenzuschieben
wäre eine große Änderung an der Fachlogik für eine Anzeige. Die Leiste
zählt stattdessen Dateien und Paks („Entpacken: X — Datei 2 von 3“).

**Zeitangaben und Größen kommen aus dem Dateisystem.** Ein `Profile` trägt
keinen Zeitstempel und ein `BackupEntry` keine Größe – beides steht in der
Spalte des Entwurfs. Genommen wird die Änderungszeit der Profildatei
beziehungsweise die Größe des Backup-Archivs.

## Bewusste Abweichungen

**Nur zwei der fünf Statusmerker stehen in der Zeile.** Der Entwurf kennt
`ÜBERNOMMEN`, `GEÄNDERT`, `FEHLT`, `NEU` und `ZURÜCK`. Die letzten drei
beschreiben, was der letzte Abgleich getan hat – nach einem Neustart wären
sie schlicht nicht mehr wahr, ein `FEHLT`-Eintrag existiert danach gar
nicht mehr. Sie erscheinen deshalb als Meldung in der Hinweisleiste. In der
Zeile bleiben die beiden Zustände, die sich jederzeit aus Bibliothek und
Verzeichnis ablesen lassen: ein Pak ohne Bibliothekseintrag
(`ÜBERNOMMEN`) und eines, dessen Inhalt vom importierten abweicht
(`GEÄNDERT`).

**Die Detailkarte erscheint auch für ein von Hand kopiertes Pak.** Der
Entwurf sieht das so vor (Szenario „Statusmarker“), und es ist die Zeile,
die als Einzige einen Warnmerker trägt – ausgerechnet ihr die Erklärung zu
verweigern wäre verkehrt. Unbekannte Felder stehen als „—“, die Herkunft
als „von Hand ins Mods-Verzeichnis gelegt“.

**Wiederherstellen fragt nur nach, wenn Steam läuft.** So legt es der
Entwurf an: der Dialog heißt „Wiederherstellen, obwohl Steam läuft?“, und
der Text über der Tabelle sagt zu, dass der Loader den aktuellen Stand
vorher immer automatisch sichert. Ohne laufendes Steam genügt diese Zusage
– dieselbe Grenze wie `--force` auf der Kommandozeile.

**Änderungen sind gesperrt, solange ein Auftrag läuft.** Ein
Hintergrundthread arbeitet auf Kopien von Bibliothek und Konfiguration und
schreibt sie am Ende zurück; eine zwischenzeitliche Umsortierung ginge
dabei verloren. Navigieren, Filtern und Auswählen bleiben frei – „das
Fenster bleibt bedienbar“, wie der Entwurf verlangt.

**Das Schreibrecht wird beim Laden einmal geprüft**, indem eine leere
Probedatei im Mods-Verzeichnis angelegt und sofort wieder gelöscht wird.
Ohne das ließe sich die Zeile „pak_config.yaml beschreibbar“ in der
Zustandskarte nicht füllen.

## Was noch nicht sichtgeprüft ist

Der Import-Fortschritt mit Abbruch und die Wiederherstellung wurden im
Ablauf gebaut, aber nicht am echten Vorgang beobachtet – beides verändert
Dateien und gehört in einen Durchlauf zusammen mit dem Nutzer.

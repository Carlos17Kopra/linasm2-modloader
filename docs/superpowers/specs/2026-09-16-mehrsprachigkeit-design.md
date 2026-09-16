# SM2 Mod Loader — Mehrsprachigkeit

**Datum:** 2026-09-16
**Status:** Entwurf zur Freigabe
**Betrifft:** `crates/core` und `crates/app`, beide vollständig

---

## 1. Zweck

Heute spricht das Programm ausschließlich Deutsch: GUI-Beschriftungen, CLI-Ausgaben, `--help`-Texte und Fehlermeldungen. Das schließt den größten Teil der Linux-Spielerschaft aus, für die dieser Loader gebaut ist.

Nach diesem Umbau gilt:

- **Englisch ist die Standardsprache.** Jede frische Installation startet auf Englisch, unabhängig von der Systemumgebung.
- **Deutsch ist umschaltbar** — in der GUI über eine Auswahlliste, auf der Kommandozeile über `--lang` oder den Unterbefehl `lang`.
- **Eine neue Sprache hinzuzufügen ist eine Fleißaufgabe, keine Programmieraufgabe:** Datei kopieren, übersetzen, eine Zeile eintragen. Die Testsuite sagt anschließend, ob die Übersetzung vollständig ist.

**Erfolgskriterium:** Im Code steht kein nutzersichtbarer Text mehr. Jeder Satz, den ein Mensch zu sehen bekommt, kommt aus einer Sprachdatei.

---

## 2. Ausgangslage

Was umgestellt werden muss, gezählt am Stand von Commit `2c01cb4`:

| Ort | Umfang | Besonderheit |
|---|---|---|
| GUI (16 Dateien) | ~150–200 Texte | Beschriftungen, Dialoge, Toasts, Statusmeldungen |
| `crates/core/src/error.rs` | 20 Varianten | `#[error("…")]` ist `&'static str` |
| `crates/app/src/cli.rs` | ~60 `clap`-Hilfetexte | `///`-Doc-Kommentare sind `&'static str` |
| `crates/app/src/cli.rs` | ~58 Ausgaben | `println!`, `bail!`, `anyhow!` |
| `crates/app/src/gui/format.rs` | 2 Funktionen | „3,8 GB" und „14.09.2026" sind sprachabhängig |
| `crates/core/src/settings.rs` | — | braucht ein Feld für die Sprachwahl |

Drei Fehlervarianten tragen ihre Detailmeldung als deutschen `String`: `CorruptBackup`, `UnusableArchive`, `PakConfig`. Sie sind damit doppelt betroffen — Rahmen *und* Inhalt sind Text.

---

## 3. Getroffene Entscheidungen

| Frage | Entscheidung | Begründung |
|---|---|---|
| Umfang | Alles: GUI, CLI-Ausgaben, `--help`, Fehlertexte | Sonst bleibt eine Ecke stur einsprachig, und gerade Fehlermeldungen liest man im Zweifel am dringendsten |
| Ablage | Fest eingebaut, eine Datei pro Sprache | Nichts kann zur Laufzeit fehlen oder beschädigt sein; für „neue Sprache leicht hinzufügen" reicht es aus |
| Startsprache | Immer Englisch, bis umgestellt wird | Vorhersagbar; keine Abhängigkeit von `LANG` beim ersten Eindruck |
| Mechanik | Eigener Katalog (kein `rust-i18n`, kein Fluent) | Die Vollständigkeitsprüfungen aus Abschnitt 9 sind der eigentliche Wert, und die schreibt man sich bei einem eigenen Katalog genau passend |

---

## 4. Katalog und Schlüssel

### 4.1 Aufbau

Neues Modul `crates/core/src/i18n.rs`. Die Sprachdateien liegen unter `crates/core/i18n/` und werden per `include_str!` beim Bauen eingebettet:

```
crates/core/i18n/en.toml      Quelle der Wahrheit
crates/core/i18n/de.toml      Übersetzung
```

Die Dateien sind nach Bereichen in TOML-Tabellen gegliedert und werden beim ersten Zugriff zu Punkt-Schlüsseln eingeebnet:

```toml
[error]
steam_not_found = "Steam installation not found"
game_not_found  = "Space Marine 2 (AppID {app_id}) is not installed in any Steam library"

[gui.saves]
import_button = "Import backup"
```

ergibt `error.steam_not_found`, `error.game_not_found`, `gui.saves.import_button`.

Namensräume: `error.*` für Fehlertexte, `cli.*` für Kommandozeile (Ausgaben und Hilfe), `app.*` für Meldungen aus der gemeinsamen `AppState`-Logik (Reconciliation-Hinweise, die GUI und CLI beide anzeigen), `gui.*` für die Oberfläche, `format.*` für Zahlen- und Datumsmuster.

### 4.2 Sprachen

```rust
pub enum Language { English, German }
```

mit `code()` (`"en"`, `"de"`), `native_name()` (`"English"`, `"Deutsch"` — für die Auswahlliste, jede Sprache nennt sich selbst) und `ALL`. Eine neue Sprache erfordert genau drei Handgriffe: Datei anlegen, Variante ergänzen, in `ALL` eintragen.

### 4.3 Nachschlagen

Die aktive Sprache steht in einem globalen Schalter (`RwLock<Language>`), gesetzt einmal beim Start und bei jeder Umstellung. Das ist bewusst global: die Hintergrund-Threads der GUI rufen in den Kern hinein, und einen Katalogverweis durch alle Aufrufstellen zu fädeln wäre Rauschen ohne Gegenwert. Die Alternative — ein `&Catalog` als Parameter — wurde verworfen, weil sie ~300 Signaturen anfasst, um eine Einstellung zu transportieren, die sich pro Programmlauf höchstens ein paar Mal ändert.

Zugriff über ein Makro, das `crates/core` per `#[macro_export]` bereitstellt und beide Crates benutzen:

```rust
t!("gui.saves.import_button")
t!("saves.verified", created_at = entry.created_at)
```

Platzhalter sind benannt (`{created_at}`) und werden textuell ersetzt. Ein unbekannter Schlüssel liefert den Schlüssel selbst zurück — sichtbar, aber ohne Absturz und ohne leere Fläche. Fehlt ein Platzhalterwert, bleibt die Klammer stehen. Beides darf im ausgelieferten Zustand nicht vorkommen; dafür sorgen die Tests aus Abschnitt 9, nicht die Laufzeit.

---

## 5. Fehler werden Daten

### 5.1 `Display` von Hand

`#[error("…")]` kann keine Sprache umschalten. `Error` bekommt deshalb ein handgeschriebenes `Display`, das je Variante einen Schlüssel nachschlägt und die Felder als Platzhalter einsetzt, sowie ein handgeschriebenes `source()` für die Fehlerkette. `thiserror` fällt damit aus `crates/core` heraus; `#[from] std::io::Error` wird zu einem dreizeiligen `impl From`.

Der Gewinn rechtfertigt die Handarbeit: es gibt **eine** Stelle, an der eine Variante auf einen Text abgebildet wird, und alle rund 60 Aufrufer, die heute `{e}` ausgeben, sprechen ohne jede Änderung die gewählte Sprache.

### 5.2 Detailmeldungen

Zwei Varianten bauen sich ihre Detailsätze heute selbst zusammen und bekommen je ein Grund-Enum, damit auch das Detail übersetzbar wird:

```rust
pub enum BackupDefect {
    NotAZip { path: PathBuf },
    UnknownEntry { name: String },
    DuplicateEntry { name: String },
    SizeMismatch { name: String },
    HashMismatch { name: String },
    CountMismatch { found: usize, expected: usize },
    InvalidPath { name: String },
}

pub enum ArchiveDefect {
    NotAZip { path: PathBuf },
    Symlink { name: String },
    InvalidPath { name: String },
    DuplicateName { name: String },
    TooLarge { limit: u64 },
}
```

`Error::CorruptBackup(BackupDefect)` und `Error::UnusableArchive(ArchiveDefect)` ersetzen die heutigen `String`-Nutzlasten. Jeder Grund bekommt einen eigenen Schlüssel unter `error.backup_defect.*` beziehungsweise `error.archive_defect.*`.

`PakConfig(String)` behält eine Durchreiche: Der Inhalt stammt aus der YAML-Bibliothek und wird nicht übersetzt, sondern in einen übersetzten Rahmen gesetzt (`error.pak_config` = `"pak_config.yaml is malformed: {detail}"`). Fremdtext übersetzen wir nicht — das gilt genauso für die Meldungen aus `describe_toml_error`, deren Rahmen übersetzt wird und deren Zeilen- und Spaltenangabe unverändert bleibt.

### 5.3 Größenangaben in Fehlern

`describe_size` in `saves.rs` formuliert heute „512 MiB" beziehungsweise „1024 Byte". Die Einheit bleibt, das Wort „Byte" wird ein Schlüssel.

---

## 6. `--help` zur Laufzeit

Die `///`-Kommentare in `cli.rs` hören auf, Programmausgabe zu sein: sie werden gewöhnliche englische Entwicklerkommentare und dienen nur noch als Notnagel, falls ein Schlüssel fehlt.

Vor dem Parsen läuft der von `clap` gebaute `Command`-Baum einmal durch eine Lokalisierung, die für jeden Befehl `about` und für jedes Argument `help` aus dem Katalog setzt (`Command::mut_subcommand`, `Command::mut_arg`). Die Schlüssel folgen dem Befehlspfad:

```
cli.save.about                Unterbefehl `save`
cli.save.import.about         Unterbefehl `save import`
cli.save.import.arg.tag       dessen Option `--tag`
cli.save.import.arg.archive   dessen Positionsargument
```

Ein Test läuft denselben Baum ab und besteht darauf, dass für jeden Befehl und jedes Argument ein Schlüssel existiert. Das ist der Riegel dagegen, dass ein später ergänztes Argument stumm auf Englisch bleibt.

---

## 7. Sprache wählen

### 7.1 Einstellung

`Settings` bekommt `language: Option<String>` (leer = Englisch), gespeichert in `settings.toml`. Ein unbekannter Code in der Datei ist kein Fehler: das Programm fällt auf Englisch zurück und meldet es einmal als Hinweis — eine von Hand editierte Einstellung darf den Start nicht verhindern.

### 7.2 Oberfläche

Auf der Einstellungsseite eine Auswahlliste mit `Language::ALL`, jede Sprache mit ihrem eigenen Namen. Ein Wechsel setzt den globalen Schalter, speichert die Einstellung und wirkt sofort — alle Texte laufen durch das Makro und werden im nächsten Frame neu geholt.

### 7.3 Kommandozeile

- `--lang <code>` als globale Option: gilt für genau diesen Aufruf, überstimmt die Einstellung, ändert sie nicht.
- `lang` als Unterbefehl: ohne Argument zeigt er die aktive und alle verfügbaren Sprachen, mit Argument (`lang de`) stellt er dauerhaft um. Ohne ihn käme ein reiner Kommandozeilen-Nutzer nur über das Editieren der `settings.toml` an die Einstellung.

---

## 8. Zahlen und Datum

Beide Formate wandern in den Katalog, statt in `format.rs` einprogrammiert zu bleiben:

```toml
[format]
decimal_separator = "."                             # de: ","
datetime = "{year}-{month}-{day} {clock}"           # de: "{day}.{month}.{year} {clock}"
byte_unit = "byte"                                  # de: "Byte"
```

Damit bringt eine neue Sprache ihre Schreibweise selbst mit, ohne dass Code angefasst wird. Die Einheiten GB/MB/kB bleiben, wie sie sind — sie sind international.

Die bestehenden Tests auf „3,8 GB" und „14.09.2026" werden zu Tests pro Sprache: derselbe Wert, einmal auf Englisch, einmal auf Deutsch geprüft.

---

## 9. Tests

Diese Prüfungen machen den Umbau verantwortbar und die spätere Pflege billig:

1. **Vollständigkeit je Sprache.** Jede Sprachdatei hat exakt die Schlüsselmenge der englischen — fehlende *und* überzählige Schlüssel schlagen fehl. Das ist der Test, der eine neue Sprache abnimmt.
2. **Jeder benutzte Schlüssel existiert.** Ein Test liest die Quelldateien beider Crates, sammelt alle `t!("…")`-Vorkommen ein und prüft sie gegen `en.toml`. Fängt Tippfehler, die sonst erst im Fenster auffallen.
3. **`clap` vollständig.** Der Durchlauf über den Befehlsbaum verlangt für jeden Befehl und jedes Argument einen Schlüssel.
4. **Platzhalter passen.** Die `{…}`-Namen einer Übersetzung sind eine Teilmenge der englischen Fassung. Ein `{naem}` in `de.toml` stünde sonst roh auf dem Bildschirm.
5. **Umschalten wirkt.** Ein Wechsel der Sprache ändert einen konkreten Text; ohne Einstellung ist Englisch aktiv.
6. **Formate pro Sprache.** Größen und Zeitstempel in beiden Sprachen.
7. **Sweep gegen Reste.** Ein Test durchsucht alle `src/**/*.rs` zeilenweise nach deutschen Zeichenketten (Umlaute, `„`, typische Wörter wie „nicht", „wird", „kein") und meldet jeden Treffer mit Datei und Zeile. Übersprungen werden Kommentarzeilen (`//`, `///`, `//!`) und alles ab dem `#[cfg(test)]`-Modul einer Datei — Kommentare sind laut Hausregel Englisch, und Testdaten dürfen bewusst deutsch sein. Der Test ist heuristisch und genau dafür gut: er findet, was bei 300 Texten übersehen wurde.

---

## 10. Umsetzung in fünf Schritten

Jeder Schritt endet mit grüner Testsuite und sauberem Clippy und ist für sich committebar.

1. **Fundament.** `i18n`-Modul mit `Language`, Katalog, `t!`-Makro, beide Sprachdateien (zunächst nur mit den Schlüsseln, die der Schritt selbst braucht), `Settings.language`, Tests 1, 2, 4 und 5.
2. **Fehler.** `Display`/`source` von Hand, `thiserror` aus dem Kern entfernt, `BackupDefect` und `ArchiveDefect`, Schlüssel für alle Varianten.
3. **Kommandozeile.** Ausgaben über den Katalog, Lokalisierung des `clap`-Baums, `--lang`, Unterbefehl `lang`, Test 3.
4. **Oberfläche.** Beschriftungen, Dialoge, Toasts, Statusmeldungen, Auswahlliste in den Einstellungen, `format.rs` mit Test 6.
5. **Abschluss.** Konvention in `CLAUDE.md` umgeschrieben, Sweep-Test 7, `docs/GUI-UMSETZUNG.md` nachgezogen.

Die heutigen deutschen Sätze gehen dabei nicht verloren: `de.toml` wird aus ihnen befüllt, Wort für Wort wie sie jetzt dastehen. Die englische Fassung ist die Neuschöpfung.

---

## 11. Was bewusst nicht dazugehört

- **Keine Sprachdateien zur Laufzeit.** Kein Ordner, aus dem zusätzliche Übersetzungen nachgeladen werden. Eine neue Sprache geht über einen Beitrag ins Repository, nicht über eine Datei im Home-Verzeichnis. Falls das später gewünscht ist, liegt der Katalog bereits richtig dafür.
- **Keine Pluralregeln.** Wo eine Anzahl im Satz steht, wird neutral formuliert (`"{count} file(s)"`, `"{count} Datei(en)"`). Für Englisch und Deutsch trägt das; eine Sprache mit mehreren Pluralformen bräuchte mehr, und dann ist der richtige Zeitpunkt, es einzubauen.
- **Keine Übersetzung von Fremdtext.** Meldungen aus Bibliotheken (YAML, TOML, `zip`) bleiben, wie sie kommen, und bekommen nur einen übersetzten Rahmen.
- **Keine Erkennung der Systemsprache.** Bewusst entschieden (Abschnitt 3); später eine Einzeile, falls gewünscht.
- **Keine Übersetzung der Doku.** `docs/` und `README` bleiben Deutsch.

---

## 12. Risiken

- **Menge.** Rund 300 Schlüssel sind überwiegend mechanische Arbeit. Das Risiko ist nicht Schwierigkeit, sondern Unaufmerksamkeit — dagegen stehen die Tests 1, 2 und 7.
- **Sicherheitsnahe Pfade.** Schritt 2 fasst `error.rs` an, Schritt 3 und 4 die Meldungen rund um `restore`. Verhaltensänderung ist dabei ausdrücklich nicht beabsichtigt: Die bestehenden Tests prüfen Verhalten, nicht Wortlaut, und müssen durchgehend grün bleiben. Wo ein Test heute auf einen deutschen Wortlaut prüft, wird er auf den Schlüssel oder auf das Verhalten umgestellt, nicht gelöscht.
- **`thiserror`-Ausbau.** Handgeschriebenes `Display`/`source` ist mehr Code als ein Attribut. Dafür entfällt die Doppelpflege von Attributtext und Katalogtext, die sonst zwangsläufig auseinanderliefe.

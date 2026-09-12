use crate::atomic::write_atomic;
use crate::error::{Error, Result};
use std::path::Path;
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
        // Windows-Editoren (z. B. Notepad) schreiben gerne ein UTF-8-BOM voran.
        // Ohne das zu entfernen, liest yaml-rust2 das Dokument als Hash statt
        // als Array und die Wurzel-Prüfung unten schlägt mit einer
        // irreführenden Meldung fehl.
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);

        if text.trim().is_empty() {
            return Ok(Self::default());
        }

        let docs = YamlLoader::load_from_str(text).map_err(|e| {
            let marker = e.marker();
            Error::PakConfig(format!(
                "die YAML-Syntax ist ungültig (Zeile {}, Spalte {})",
                marker.line(),
                marker.col() + 1,
            ))
        })?;

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

            let disabled = match map.get(&Yaml::String("disabled".into())) {
                None => false,
                Some(value) => value.as_bool().ok_or_else(|| {
                    Error::PakConfig(format!(
                        "Eintrag {} hat einen ungültigen Wert für 'disabled': {} \
                         (erwartet: true oder false)",
                        i + 1,
                        describe_yaml_scalar(value),
                    ))
                })?,
            };

            entries.push(PakEntry { pak: pak.to_string(), disabled });
        }

        Ok(Self { entries })
    }

    /// Die aktiven Einträge in Ladereihenfolge.
    pub fn enabled(&self) -> impl Iterator<Item = &PakEntry> {
        self.entries.iter().filter(|e| !e.disabled)
    }

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

    /// Lädt die Konfiguration von `path`. Eine fehlende Datei ergibt eine
    /// leere Konfiguration (frische Installation ohne Mods).
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    /// Schreibt die Konfiguration atomar nach `path`.
    pub fn save(&self, path: &Path) -> Result<()> {
        write_atomic(path, &self.to_yaml())
    }

    /// Bringt die Konfiguration mit dem tatsächlichen Verzeichnisinhalt in
    /// Einklang. Reihenfolge und Aktivierungszustand bestehender Einträge
    /// bleiben unangetastet.
    pub fn reconcile(&mut self, present: &[String]) -> Reconciliation {
        let present_set: std::collections::HashSet<&str> =
            present.iter().map(String::as_str).collect();

        let mut removed = Vec::new();
        self.entries.retain(|e| {
            if present_set.contains(e.pak.as_str()) {
                true
            } else {
                removed.push(e.pak.clone());
                false
            }
        });

        // `present` ist eine `&[String]`, kein Set: ein Verzeichnis kann
        // denselben Namen zwar nicht doppelt enthalten, ein Aufrufer könnte
        // ihn aber doppelt melden. `known` wird deshalb beim Aufbau von
        // `added` laufend erweitert (nicht nur einmal vorab berechnet), damit
        // ein wiederholter Name nur einmal aufgenommen wird.
        let mut known: std::collections::HashSet<&str> =
            self.entries.iter().map(|e| e.pak.as_str()).collect();

        let mut added: Vec<String> = Vec::new();
        for pak in present {
            if known.insert(pak.as_str()) {
                added.push(pak.clone());
            }
        }
        added.sort();

        for pak in &added {
            self.entries.push(PakEntry { pak: pak.clone(), disabled: false });
        }

        Reconciliation { added, removed }
    }
}

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

/// Setzt einen Dateinamen in Anführungszeichen, wenn er sonst als anderes
/// YAML-Konstrukt gelesen würde.
///
/// Die offensichtlichen Sonderzeichen (Doppelpunkt, Raute, Anführungszeichen,
/// führende Strukturzeichen, Rand-Whitespace) werden per Zeichen-Check
/// erkannt. Das reicht aber nicht: ein Name wie "true", "null" oder "123"
/// enthält keines dieser Zeichen, würde aber unquotiert als Bool/Null/Zahl
/// statt als String gelesen – `parse` bekäme dann keinen gültigen
/// `pak`-Schlüssel mehr und schlägt fehl. Deshalb wird zusätzlich mit dem
/// echten YAML-Parser geprüft, ob der unquotierte Name als Skalar zu
/// irgendetwas anderem als exakt sich selbst als String aufgelöst würde
/// (das erfasst auch eingebettete Zeilenumbrüche, die als eigenständiges
/// Plain-Scalar zu einem Leerzeichen gefaltet würden).
fn quote_if_needed(name: &str) -> String {
    let needs_quotes = name.is_empty()
        || name.contains(':')
        || name.contains('#')
        || name.contains('"')
        || name.contains('\'')
        || name.starts_with(['-', '?', '&', '*', '!', '|', '>', '%', '@', '`', '[', '{'])
        || name.trim() != name
        || resolves_to_non_string_scalar(name);

    if needs_quotes {
        format!(
            "\"{}\"",
            name.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\r', "\\r")
                .replace('\n', "\\n")
        )
    } else {
        name.to_string()
    }
}

/// Prüft, ob `name` als eigenständiges, unquotiertes YAML-Plain-Scalar zu
/// etwas anderem als der Zeichenkette `name` selbst aufgelöst würde (z. B.
/// zu einem Bool, einer Zahl, `null` oder einer gefalteten Zeile). Nutzt
/// denselben Parser wie `parse`, damit die Entscheidung garantiert mit dem
/// tatsächlichen Leseverhalten übereinstimmt.
fn resolves_to_non_string_scalar(name: &str) -> bool {
    match YamlLoader::load_from_str(name) {
        Ok(docs) => !matches!(docs.first(), Some(Yaml::String(s)) if s == name),
        Err(_) => true,
    }
}

/// Stellt einen YAML-Skalar für eine Fehlermeldung dar. `yaml-rust2` hat
/// kein `Display` für `Yaml`, daher hier eine kleine, für Nutzer lesbare
/// Übersetzung der gängigen Fälle mit einem `Debug`-Fallback für den Rest.
fn describe_yaml_scalar(value: &Yaml) -> String {
    match value {
        Yaml::String(s) => format!("\"{s}\""),
        Yaml::Integer(n) => n.to_string(),
        Yaml::Real(s) => s.clone(),
        Yaml::Boolean(b) => b.to_string(),
        Yaml::Null => "null".to_string(),
        Yaml::Array(_) => "eine Liste".to_string(),
        Yaml::Hash(_) => "ein Objekt".to_string(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(pak: &str, disabled: bool) -> PakEntry {
        PakEntry { pak: pak.to_string(), disabled }
    }

    fn names(cfg: &PakConfig) -> Vec<&str> {
        cfg.entries.iter().map(|e| e.pak.as_str()).collect()
    }

    #[test]
    fn adds_unknown_paks_as_enabled_at_end() {
        // Nicht aufgeführte Paks lädt die Engine ohnehin – also aktiv aufnehmen,
        // damit sie steuerbar werden.
        let mut cfg = PakConfig { entries: vec![entry("a.pak", false)] };
        let result = cfg.reconcile(&["a.pak".into(), "neu.pak".into()]);

        assert_eq!(names(&cfg), vec!["a.pak", "neu.pak"]);
        assert!(!cfg.entries[1].disabled);
        assert_eq!(result.added, vec!["neu.pak"]);
        assert!(result.removed.is_empty());
    }

    #[test]
    fn removes_entries_without_file() {
        let mut cfg = PakConfig {
            entries: vec![entry("a.pak", false), entry("weg.pak", true)],
        };
        let result = cfg.reconcile(&["a.pak".into()]);

        assert_eq!(names(&cfg), vec!["a.pak"]);
        assert_eq!(result.removed, vec!["weg.pak"]);
    }

    #[test]
    fn keeps_order_and_state_of_existing_entries() {
        let mut cfg = PakConfig {
            entries: vec![entry("z.pak", true), entry("a.pak", false)],
        };
        cfg.reconcile(&["a.pak".into(), "z.pak".into()]);

        assert_eq!(names(&cfg), vec!["z.pak", "a.pak"], "Reihenfolge darf sich nicht ändern");
        assert!(cfg.entries[0].disabled, "Deaktivierung darf nicht verloren gehen");
    }

    #[test]
    fn adds_multiple_new_paks_alphabetically() {
        let mut cfg = PakConfig::default();
        let result = cfg.reconcile(&["b.pak".into(), "a.pak".into()]);

        assert_eq!(names(&cfg), vec!["a.pak", "b.pak"]);
        assert_eq!(result.added, vec!["a.pak", "b.pak"]);
    }

    #[test]
    fn reconcile_without_change_reports_nothing() {
        let mut cfg = PakConfig { entries: vec![entry("a.pak", false)] };
        let result = cfg.reconcile(&["a.pak".into()]);

        assert!(result.is_empty());
    }

    /// Ein Verzeichnis kann denselben Dateinamen nicht doppelt enthalten,
    /// aber `present` ist ein `&[String]`, kein Set – ein Aufrufer könnte
    /// (versehentlich, z. B. durch doppeltes Einlesen) denselben Namen
    /// zweimal übergeben. Ohne Deduplizierung würde `reconcile` daraus zwei
    /// identische Einträge in der Konfiguration machen – stille
    /// Datenkorruption.
    #[test]
    fn deduplicates_repeatedly_reported_filenames() {
        let mut cfg = PakConfig::default();
        let result = cfg.reconcile(&["a.pak".into(), "a.pak".into()]);

        assert_eq!(names(&cfg), vec!["a.pak"], "darf keinen doppelten Eintrag erzeugen");
        assert_eq!(result.added, vec!["a.pak"]);
    }

    /// Eine von Hand bearbeitete Konfiguration kann bereits einen Pak-Namen
    /// doppelt enthalten. `reconcile` darf bestehende Einträge nicht
    /// zusammenführen oder umordnen (siehe `keeps_order_and_state_of_existing_entries`) – ein
    /// bereits vorhandenes Duplikat bleibt also unangetastet bestehen, statt
    /// dass reconcile es „repariert“ oder ein weiteres Duplikat hinzufügt.
    #[test]
    fn leaves_existing_duplicates_in_config_untouched() {
        let mut cfg = PakConfig {
            entries: vec![entry("a.pak", false), entry("a.pak", true)],
        };
        let result = cfg.reconcile(&["a.pak".into()]);

        assert_eq!(
            cfg.entries,
            vec![entry("a.pak", false), entry("a.pak", true)],
            "bestehende Duplikate werden weder entfernt noch verändert"
        );
        assert!(result.is_empty(), "ein bereits bekannter Name ist kein neuer Fund");
    }

    #[test]
    fn reads_the_example_from_the_engine_readme() {
        let text = "- pak: mod_a.pak\n- pak: mod_b.pak\n  disabled: true\n";
        let cfg = PakConfig::parse(text).unwrap();
        assert_eq!(cfg.entries, vec![entry("mod_a.pak", false), entry("mod_b.pak", true)]);
    }

    #[test]
    fn reads_the_real_config_from_the_reference_system() {
        let cfg = PakConfig::parse("- pak: wa_astartes_14_1.pak\n").unwrap();
        assert_eq!(cfg.entries, vec![entry("wa_astartes_14_1.pak", false)]);
    }

    #[test]
    fn keeps_the_order() {
        let text = "- pak: z.pak\n- pak: a.pak\n- pak: m.pak\n";
        let cfg = PakConfig::parse(text).unwrap();
        let names: Vec<&str> = cfg.entries.iter().map(|e| e.pak.as_str()).collect();
        assert_eq!(names, vec!["z.pak", "a.pak", "m.pak"], "Reihenfolge ist die Ladereihenfolge");
    }

    #[test]
    fn empty_file_yields_empty_config() {
        assert_eq!(PakConfig::parse("").unwrap(), PakConfig::default());
        assert_eq!(PakConfig::parse("\n\n").unwrap(), PakConfig::default());
        assert_eq!(PakConfig::parse("[]\n").unwrap(), PakConfig::default());
    }

    #[test]
    fn tolerates_comments_and_quotes() {
        let text = "# von Hand bearbeitet\n- pak: \"mit leerzeichen.pak\"\n  disabled: false\n";
        let cfg = PakConfig::parse(text).unwrap();
        assert_eq!(cfg.entries, vec![entry("mit leerzeichen.pak", false)]);
    }

    #[test]
    fn rejects_non_list_root() {
        let error = PakConfig::parse("pak: a.pak\n").unwrap_err();
        assert!(matches!(error, Error::PakConfig(_)));
    }

    #[test]
    fn rejects_entry_without_pak_key() {
        let error = PakConfig::parse("- disabled: true\n").unwrap_err();
        assert!(matches!(error, Error::PakConfig(_)));
    }

    #[test]
    fn enabled_filters_out_disabled() {
        let cfg = PakConfig::parse("- pak: a.pak\n- pak: b.pak\n  disabled: true\n- pak: c.pak\n").unwrap();
        let names: Vec<&str> = cfg.enabled().map(|e| e.pak.as_str()).collect();
        assert_eq!(names, vec!["a.pak", "c.pak"]);
    }

    #[test]
    fn reports_yaml_syntax_error_in_german_with_position() {
        // doppelter Schlüssel in derselben Zuordnung ist laut YAML-Spezifikation
        // ein Scanner-Fehler in yaml-rust2, nicht nur ein Überschreiben.
        let text = "- pak: a.pak\n  pak: b.pak\n";
        let error = PakConfig::parse(text).unwrap_err();
        let Error::PakConfig(message) = error else {
            panic!("erwartete Error::PakConfig, bekam {error:?}");
        };
        for english_fragment in ["duplicated key", "mapping", "byte", "at byte"] {
            assert!(
                !message.contains(english_fragment),
                "Meldung darf keinen rohen englischen Scanner-Text enthalten \
                 (gefunden: {english_fragment:?}): {message:?}"
            );
        }
        assert!(
            message.contains("Zeile") && message.contains("Spalte"),
            "Meldung soll die Position benennen: {message:?}"
        );
    }

    #[test]
    fn rejects_non_boolean_disabled_value() {
        for text in [
            "- pak: a.pak\n  disabled: yes\n",
            "- pak: a.pak\n  disabled: on\n",
            "- pak: a.pak\n  disabled: 1\n",
            "- pak: a.pak\n  disabled: \"true\"\n",
        ] {
            let error = PakConfig::parse(text).unwrap_err();
            let Error::PakConfig(message) = error else {
                panic!("erwartete Error::PakConfig für {text:?}, bekam {error:?}");
            };
            assert!(
                message.contains("Eintrag 1") && message.contains("disabled"),
                "Meldung soll den Eintrag benennen: {message:?} (Eingabe: {text:?})"
            );
        }
    }

    #[test]
    fn ignores_utf8_bom_at_start_of_file() {
        let text = "\u{feff}- pak: mod_a.pak\n- pak: mod_b.pak\n  disabled: true\n";
        let cfg = PakConfig::parse(text).unwrap();
        assert_eq!(cfg.entries, vec![entry("mod_a.pak", false), entry("mod_b.pak", true)]);
    }

    #[test]
    fn bom_alone_yields_empty_config() {
        assert_eq!(PakConfig::parse("\u{feff}").unwrap(), PakConfig::default());
    }

    #[test]
    fn writes_the_format_from_the_engine_readme() {
        let cfg = PakConfig {
            entries: vec![entry("mod_a.pak", false), entry("mod_b.pak", true)],
        };
        assert_eq!(cfg.to_yaml(), "- pak: mod_a.pak\n- pak: mod_b.pak\n  disabled: true\n");
    }

    #[test]
    fn omits_disabled_for_active_entries() {
        let cfg = PakConfig { entries: vec![entry("a.pak", false)] };
        assert_eq!(cfg.to_yaml(), "- pak: a.pak\n");
    }

    #[test]
    fn writes_empty_list_as_valid_yaml() {
        // Eine leere Datei wäre YAML-Null, keine Liste. "[]" ist eindeutig.
        assert_eq!(PakConfig::default().to_yaml(), "[]\n");
    }

    #[test]
    fn quotes_names_with_special_characters() {
        let cfg = PakConfig { entries: vec![entry("mit: doppelpunkt.pak", false)] };
        assert_eq!(cfg.to_yaml(), "- pak: \"mit: doppelpunkt.pak\"\n");
    }

    #[test]
    fn round_trip_preserves_the_config() {
        let original = PakConfig {
            entries: vec![
                entry("z.pak", false),
                entry("mit: doppelpunkt.pak", true),
                entry("a.pak", false),
            ],
        };
        let roundtripped = PakConfig::parse(&original.to_yaml()).unwrap();
        assert_eq!(roundtripped, original);
    }

    #[test]
    fn load_with_missing_file_yields_empty_config() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = PakConfig::load(&dir.path().join("gibt_es_nicht.yaml")).unwrap();
        assert_eq!(cfg, PakConfig::default());
    }

    #[test]
    fn save_and_load_are_inverses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pak_config.yaml");
        let cfg = PakConfig { entries: vec![entry("a.pak", true), entry("b.pak", false)] };

        cfg.save(&path).unwrap();

        assert_eq!(PakConfig::load(&path).unwrap(), cfg);
    }

    /// Namen, die keines der "offensichtlichen" Sonderzeichen enthalten,
    /// aber unquotiert als YAML-Bool/Zahl/Null statt als String gelesen
    /// würden. Ohne die zusätzliche Skalar-Prüfung in `quote_if_needed`
    /// würde `parse` diese Einträge als fehlend am 'pak'-Schlüssel ablehnen.
    #[test]
    fn round_trips_names_that_would_be_read_as_yaml_scalars() {
        for name in [
            "true", "True", "TRUE", "false", "False", "FALSE", "null", "~", "123", "-123", "0",
            "0x1F", "0o17", "1.5", "-1.5", ".inf", "-.inf", ".nan",
        ] {
            let cfg = PakConfig { entries: vec![entry(name, false)] };
            let roundtripped = PakConfig::parse(&cfg.to_yaml()).unwrap();
            assert_eq!(roundtripped, cfg, "Name {name:?} übersteht den Rundtrip nicht");
        }
    }

    /// Eingebettete Steuerzeichen und Sonderzeichen, die in der
    /// Escape-Logik von `quote_if_needed` gesondert behandelt werden
    /// müssen, damit sie beim Wiedereinlesen nicht die YAML-Struktur
    /// sprengen oder zu einem Leerzeichen gefaltet werden.
    #[test]
    fn round_trips_names_with_embedded_special_characters() {
        for name in ["a\nb.pak", "a\rb.pak", "a\\b.pak", "a'b.pak", "  a.pak  "] {
            let cfg = PakConfig { entries: vec![entry(name, false)] };
            let roundtripped = PakConfig::parse(&cfg.to_yaml()).unwrap();
            assert_eq!(roundtripped, cfg, "Name {name:?} übersteht den Rundtrip nicht");
        }
    }
}

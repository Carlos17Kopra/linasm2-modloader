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
    let braucht_quotes = name.is_empty()
        || name.contains(':')
        || name.contains('#')
        || name.contains('"')
        || name.contains('\'')
        || name.starts_with(['-', '?', '&', '*', '!', '|', '>', '%', '@', '`', '[', '{'])
        || name.trim() != name
        || resolves_to_non_string_scalar(name);

    if braucht_quotes {
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

    #[test]
    fn meldet_yaml_syntaxfehler_auf_deutsch_mit_position() {
        // doppelter Schlüssel in derselben Zuordnung ist laut YAML-Spezifikation
        // ein Scanner-Fehler in yaml-rust2, nicht nur ein Überschreiben.
        let text = "- pak: a.pak\n  pak: b.pak\n";
        let fehler = PakConfig::parse(text).unwrap_err();
        let Error::PakConfig(meldung) = fehler else {
            panic!("erwartete Error::PakConfig, bekam {fehler:?}");
        };
        for englisches_fragment in ["duplicated key", "mapping", "byte", "at byte"] {
            assert!(
                !meldung.contains(englisches_fragment),
                "Meldung darf keinen rohen englischen Scanner-Text enthalten \
                 (gefunden: {englisches_fragment:?}): {meldung:?}"
            );
        }
        assert!(
            meldung.contains("Zeile") && meldung.contains("Spalte"),
            "Meldung soll die Position benennen: {meldung:?}"
        );
    }

    #[test]
    fn weist_nicht_booleschen_disabled_wert_zurueck() {
        for text in [
            "- pak: a.pak\n  disabled: yes\n",
            "- pak: a.pak\n  disabled: on\n",
            "- pak: a.pak\n  disabled: 1\n",
            "- pak: a.pak\n  disabled: \"true\"\n",
        ] {
            let fehler = PakConfig::parse(text).unwrap_err();
            let Error::PakConfig(meldung) = fehler else {
                panic!("erwartete Error::PakConfig für {text:?}, bekam {fehler:?}");
            };
            assert!(
                meldung.contains("Eintrag 1") && meldung.contains("disabled"),
                "Meldung soll den Eintrag benennen: {meldung:?} (Eingabe: {text:?})"
            );
        }
    }

    #[test]
    fn ignoriert_utf8_bom_am_dateianfang() {
        let text = "\u{feff}- pak: mod_a.pak\n- pak: mod_b.pak\n  disabled: true\n";
        let cfg = PakConfig::parse(text).unwrap();
        assert_eq!(cfg.entries, vec![eintrag("mod_a.pak", false), eintrag("mod_b.pak", true)]);
    }

    #[test]
    fn bom_allein_ergibt_leere_konfiguration() {
        assert_eq!(PakConfig::parse("\u{feff}").unwrap(), PakConfig::default());
    }

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

    /// Namen, die keines der "offensichtlichen" Sonderzeichen enthalten,
    /// aber unquotiert als YAML-Bool/Zahl/Null statt als String gelesen
    /// würden. Ohne die zusätzliche Skalar-Prüfung in `quote_if_needed`
    /// würde `parse` diese Einträge als fehlend am 'pak'-Schlüssel ablehnen.
    #[test]
    fn rundet_namen_die_als_yaml_skalare_gelesen_wuerden() {
        for name in [
            "true", "True", "TRUE", "false", "False", "FALSE", "null", "~", "123", "-123", "0",
            "0x1F", "0o17", "1.5", "-1.5", ".inf", "-.inf", ".nan",
        ] {
            let cfg = PakConfig { entries: vec![eintrag(name, false)] };
            let wieder = PakConfig::parse(&cfg.to_yaml()).unwrap();
            assert_eq!(wieder, cfg, "Name {name:?} übersteht den Rundtrip nicht");
        }
    }

    /// Eingebettete Steuerzeichen und Sonderzeichen, die in der
    /// Escape-Logik von `quote_if_needed` gesondert behandelt werden
    /// müssen, damit sie beim Wiedereinlesen nicht die YAML-Struktur
    /// sprengen oder zu einem Leerzeichen gefaltet werden.
    #[test]
    fn rundet_namen_mit_eingebetteten_sonderzeichen() {
        for name in ["a\nb.pak", "a\rb.pak", "a\\b.pak", "a'b.pak", "  a.pak  "] {
            let cfg = PakConfig { entries: vec![eintrag(name, false)] };
            let wieder = PakConfig::parse(&cfg.to_yaml()).unwrap();
            assert_eq!(wieder, cfg, "Name {name:?} übersteht den Rundtrip nicht");
        }
    }
}

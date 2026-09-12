use crate::error::{Error, Result};
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

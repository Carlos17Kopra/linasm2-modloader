//! Farben, Schriften und Abstände des Entwurfs, eins zu eins übernommen.
//!
//! Alle Werte stammen aus `SM2 Mod Loader GUI v2 modern.dc.html`. Der Entwurf
//! ist bewusst nur dunkel gehalten; es gibt keine helle Variante, deshalb
//! setzt `install` `Visuals::dark()` als Ausgangspunkt und überschreibt
//! anschließend jede Fläche, die im Entwurf vorkommt.

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke, Style,
    TextStyle, Visuals,
};
use std::sync::Arc;

/// Wandelt einen Hex-Literal wie `0x3ba6a0` in eine Farbe – damit die Werte
/// im Quelltext genauso dastehen wie im Entwurf.
const fn hex(value: u32) -> Color32 {
    Color32::from_rgb((value >> 16) as u8, ((value >> 8) & 0xff) as u8, (value & 0xff) as u8)
}

/// Sämtliche Farben des Entwurfs. Die Namen beschreiben die Rolle, nicht den
/// Farbton – der Entwurf benutzt dieselbe Fläche an mehreren Stellen.
pub mod color {
    use super::hex;
    use egui::Color32;

    /// Fläche des Anwendungsfensters (CentralPanel).
    pub const WINDOW: Color32 = hex(0x12141a);
    /// Seitenleiste und Statusleiste.
    pub const PANEL: Color32 = hex(0x101218);
    /// Kopfleiste über dem Inhalt.
    pub const TOP_BAR: Color32 = hex(0x14161d);
    /// Karten (Mod-Liste, Profile, Backups, Einstellungen).
    pub const CARD: Color32 = hex(0x161921);
    /// Aufklappmenü der Startauswahl.
    pub const MENU: Color32 = hex(0x191c24);
    /// Titelzeile eines modalen Fensters.
    pub const DIALOG_HEAD: Color32 = hex(0x1a1d25);
    /// Kopfzeile einer Tabelle.
    pub const TABLE_HEAD: Color32 = hex(0x13161d);
    /// Schaltflächen, Schalterspur im Aus-Zustand.
    pub const CONTROL: Color32 = hex(0x1b1f27);
    /// Eingabefelder und eingelassene Flächen.
    pub const INPUT: Color32 = hex(0x12141a);
    /// Zeile unter dem Zeiger.
    pub const HOVER: Color32 = hex(0x1a1e26);
    /// Ausgewählte Zeile beziehungsweise aktiver Navigationspunkt.
    pub const SELECTED: Color32 = hex(0x18262a);
    /// Ausgewählte Zeile unter dem Zeiger.
    pub const SELECTED_HOVER: Color32 = hex(0x1c2d32);
    /// Gesperrte Schaltfläche (Import bei schreibgeschütztem Verzeichnis).
    pub const CONTROL_DISABLED: Color32 = hex(0x171a20);

    /// Rahmen von Eingaben und Schaltflächen.
    pub const BORDER: Color32 = hex(0x262b36);
    /// Trennlinien zwischen Panels und Kartenabschnitten.
    pub const BORDER_SOFT: Color32 = hex(0x1e222b);
    /// Trennlinien zwischen Tabellenzeilen.
    pub const BORDER_ROW: Color32 = hex(0x1b1f27);
    /// Rahmen, der sich vom Untergrund abhebt (Schalterspur, Abbrechen).
    pub const BORDER_STRONG: Color32 = hex(0x2b313d);
    /// Rahmen unter dem Zeiger, wenn nicht der Akzent verwendet wird.
    pub const BORDER_HOVER: Color32 = hex(0x3a4250);
    /// Rahmen einer gesperrten Schaltfläche.
    pub const BORDER_DISABLED: Color32 = hex(0x20242c);

    /// Überschriften und hervorgehobener Text.
    pub const TEXT_STRONG: Color32 = hex(0xe6e9ef);
    /// Fließtext.
    pub const TEXT: Color32 = hex(0xc8cedb);
    /// Text zweiter Ordnung.
    pub const TEXT_DIM: Color32 = hex(0x9aa3b2);
    /// Tabelleninhalt zweiter Ordnung.
    pub const TEXT_DIM2: Color32 = hex(0x8d96a5);
    /// Beschriftungen und Erläuterungen.
    pub const TEXT_MUTED: Color32 = hex(0x8a94a6);
    /// Spaltenköpfe, Platzhalter, Symbole ohne Zustand.
    pub const TEXT_FAINT: Color32 = hex(0x7c8698);

    /// Akzent: Auswahl, Fokus, Fortschritt.
    pub const ACCENT: Color32 = hex(0x3ba6a0);
    /// Ruhezustand der Hauptschaltfläche.
    pub const ACCENT_DEEP: Color32 = hex(0x23706c);
    /// Schalterspur im Ein-Zustand.
    pub const ACCENT_MID: Color32 = hex(0x2f8c87);
    /// Schrift auf Akzentflächen.
    pub const ACCENT_FG: Color32 = hex(0xe9fbf9);
    /// Hinterlegung einer Zählerpille im aktiven Navigationspunkt.
    pub const ACCENT_SOFT: Color32 = hex(0x13262a);

    /// Warnung (übernommen, geändert, schreibgeschützt, Wiederherstellen).
    pub const WARN: Color32 = hex(0xe0a341);
    /// Fläche hinter einer Warnung.
    pub const WARN_BG: Color32 = hex(0x231d14);
    /// Rahmen einer Warnung.
    pub const WARN_BORDER: Color32 = hex(0x4a3a1c);
    /// Warnung unter dem Zeiger.
    pub const WARN_BRIGHT: Color32 = hex(0xf0c579);

    /// Fehler (fehlende Datei, Löschen).
    pub const DANGER: Color32 = hex(0xe0645c);
    /// Fläche einer zerstörenden Schaltfläche.
    pub const DANGER_BG: Color32 = hex(0x2a1a19);
    /// Rahmen einer zerstörenden Schaltfläche.
    pub const DANGER_BORDER: Color32 = hex(0x7a3a36);
    /// Zerstörende Schaltfläche unter dem Zeiger.
    pub const DANGER_HOVER_BG: Color32 = hex(0x3a201e);
    /// Schrift einer zerstörenden Schaltfläche unter dem Zeiger.
    pub const DANGER_BRIGHT: Color32 = hex(0xf08a83);
    /// Knopf des Bestätigungsschalters im Wiederherstellen-Dialog.
    pub const DANGER_KNOB: Color32 = hex(0xffd9d6);

    /// Hinweis (neu importiert, zurückgestellt).
    pub const INFO: Color32 = hex(0x6ea8fe);
    /// Fläche hinter einem Hinweismerker.
    pub const INFO_BG: Color32 = hex(0x1c2530);

    /// Bestätigung (Spiel erkannt, Backup geprüft).
    pub const OK: Color32 = hex(0x5fb98a);
    /// Fläche hinter einer Bestätigung.
    pub const OK_BG: Color32 = hex(0x16251f);

    /// Abdunklung hinter einem modalen Fenster.
    pub const OVERLAY: Color32 = Color32::from_rgba_premultiplied(4, 5, 7, 174);
}

/// Höhen und Abstände, die der Entwurf mehrfach verwendet.
pub mod metric {
    /// Kopfleiste über dem Inhalt.
    pub const TOP_BAR_HEIGHT: f32 = 58.0;
    /// Statusleiste am unteren Rand.
    pub const STATUS_BAR_HEIGHT: f32 = 30.0;
    /// Breite der Seitenleiste.
    pub const SIDE_BAR_WIDTH: f32 = 208.0;
    /// Innenabstand des Inhaltsbereichs.
    pub const CONTENT_PADDING: f32 = 14.0;
    /// Abstand zwischen den Karten im Inhaltsbereich.
    pub const CONTENT_GAP: f32 = 12.0;
    /// Innenabstand einer Karte, waagerecht.
    pub const CARD_PADDING: f32 = 14.0;
    /// Werkzeugleiste am Kopf einer Karte.
    pub const CARD_TOOLBAR_HEIGHT: f32 = 52.0;
    /// Spaltenkopf einer Tabelle.
    pub const TABLE_HEAD_HEIGHT: f32 = 30.0;
    /// Zeilenhöhe einer Tabelle.
    pub const ROW_HEIGHT: f32 = 36.0;
    /// Zeilenhöhe in Profil- und Backup-Tabellen.
    pub const LIST_ROW_HEIGHT: f32 = 38.0;
    /// Abstand zwischen zwei Spalten einer Tabellenzeile.
    pub const COLUMN_GAP: f32 = 10.0;
    /// Höhe einer Schaltfläche in einer Werkzeugleiste.
    pub const BUTTON_HEIGHT: f32 = 30.0;
    /// Höhe einer Schaltfläche in Kopfleiste und Dialogfuß.
    pub const BUTTON_HEIGHT_LARGE: f32 = 32.0;
    /// Höhe einer Schaltfläche in einer Tabellenzeile.
    pub const BUTTON_HEIGHT_SMALL: f32 = 26.0;
}

/// Schriftgrad in der Proportionalschrift (IBM Plex Sans Regular).
pub fn sans(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// Schriftgrad in der halbfetten Proportionalschrift (IBM Plex Sans Medium).
///
/// Eigene Familie statt eines Fettschnitts: `egui` kennt kein
/// `font-weight`, jede Strichstärke ist eine eigene Schriftdatei.
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM_FAMILY.into()))
}

/// Schriftgrad in der dicktengleichen Schrift (IBM Plex Mono).
pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// Name der halbfetten Familie, wie er in `FontDefinitions` eingetragen wird.
const MEDIUM_FAMILY: &str = "medium";

const SANS_REGULAR: &[u8] = include_bytes!("../../assets/fonts/IBMPlexSans-Regular.ttf");
const SANS_MEDIUM: &[u8] = include_bytes!("../../assets/fonts/IBMPlexSans-Medium.ttf");
const MONO_REGULAR: &[u8] = include_bytes!("../../assets/fonts/IBMPlexMono-Regular.ttf");

/// Trägt IBM Plex ein und setzt den Stil des Entwurfs.
pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    // `egui` hält je Erscheinungsbild einen eigenen Stil. Der Entwurf ist
    // nur dunkel gedacht, deshalb bekommen beide denselben – sonst hinge das
    // Aussehen davon ab, was der Desktop gerade meldet.
    let style = style();
    ctx.all_styles_mut(|target| *target = style.clone());
}

/// IBM Plex in drei Schnitten, jeweils mit den mitgelieferten Schriften von
/// `egui` als Rückfall – IBM Plex deckt die Zeichensätze nicht ab, die
/// `egui` intern für Menü- und Fehlerdarstellung benutzt.
fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(font_definitions());
}

/// Die Schriftdefinitionen als reiner Wert – so lässt sich prüfen, was
/// eingetragen wird, ohne einen Zeichenkontext aufzubauen.
fn font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert("plex_sans".into(), Arc::new(FontData::from_static(SANS_REGULAR)));
    fonts.font_data.insert("plex_sans_medium".into(), Arc::new(FontData::from_static(SANS_MEDIUM)));
    fonts.font_data.insert("plex_mono".into(), Arc::new(FontData::from_static(MONO_REGULAR)));

    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "plex_sans".into());
    fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "plex_mono".into());

    // Die halbfette Familie erbt dieselben Rückfallschriften wie die
    // Proportionalschrift, damit ein in IBM Plex fehlendes Zeichen auch in
    // einer Überschrift nicht als leeres Kästchen erscheint.
    let mut medium_family = vec![String::from("plex_sans_medium")];
    medium_family.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
    fonts.families.insert(FontFamily::Name(MEDIUM_FAMILY.into()), medium_family);

    fonts
}

/// Der Stil des Entwurfs: dunkle Flächen, 10-px-Rundungen, kein
/// Standardrahmen um Schaltflächen – alle Flächen zeichnen die Widgets in
/// `super::widgets` selbst.
fn style() -> Style {
    let mut visuals = Visuals::dark();

    visuals.panel_fill = color::WINDOW;
    visuals.window_fill = color::CARD;
    visuals.window_stroke = Stroke::new(1.0, color::BORDER);
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.extreme_bg_color = color::INPUT;
    visuals.faint_bg_color = color::HOVER;
    visuals.override_text_color = Some(color::TEXT_DIM);
    visuals.selection.bg_fill = color::ACCENT.gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, color::ACCENT);
    visuals.hyperlink_color = color::ACCENT;
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 26],
        blur: 70,
        spread: 0,
        color: Color32::from_black_alpha(190),
    };
    visuals.popup_shadow = egui::epaint::Shadow {
        offset: [0, 16],
        blur: 36,
        spread: 0,
        color: Color32::from_black_alpha(166),
    };

    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(7);
        widget.bg_fill = color::CONTROL;
        widget.weak_bg_fill = color::CONTROL;
        widget.bg_stroke = Stroke::new(1.0, color::BORDER);
        widget.fg_stroke = Stroke::new(1.0, color::TEXT);
        widget.expansion = 0.0;
    }
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, color::BORDER_SOFT);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, color::BORDER_HOVER);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, color::TEXT_STRONG);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, color::ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, color::TEXT_STRONG);

    let mut style = Style { visuals, ..Default::default() };
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(11.0, 6.0);
    style.spacing.window_margin = Margin::same(0);
    style.spacing.menu_margin = Margin::same(5);
    style.spacing.scroll.bar_width = 10.0;
    style.spacing.scroll.bar_inner_margin = 2.0;
    style.spacing.scroll.bar_outer_margin = 0.0;
    style.spacing.interact_size = egui::vec2(0.0, 20.0);

    style.text_styles = [
        (TextStyle::Small, sans(11.0)),
        (TextStyle::Body, sans(12.5)),
        (TextStyle::Button, sans(12.5)),
        (TextStyle::Heading, medium(15.0)),
        (TextStyle::Monospace, mono(11.5)),
    ]
    .into();

    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_wandelt_wie_im_entwurf_notiert() {
        assert_eq!(hex(0x3ba6a0), Color32::from_rgb(0x3b, 0xa6, 0xa0), "Akzentfarbe des Entwurfs");
        assert_eq!(hex(0x000000), Color32::BLACK, "Nullwert ergibt Schwarz");
        assert_eq!(hex(0xffffff), Color32::WHITE, "Höchstwert ergibt Weiß");
    }

    #[test]
    fn ibm_plex_steht_in_jeder_familie_an_erster_stelle() {
        let fonts = font_definitions();
        assert_eq!(
            fonts.families[&FontFamily::Proportional].first().map(String::as_str),
            Some("plex_sans"),
            "die Proportionalschrift muss IBM Plex Sans sein"
        );
        assert_eq!(
            fonts.families[&FontFamily::Monospace].first().map(String::as_str),
            Some("plex_mono"),
            "die dicktengleiche Schrift muss IBM Plex Mono sein"
        );
    }

    #[test]
    fn die_halbfette_familie_faellt_auf_dieselben_schriften_zurueck_wie_die_proportionale() {
        // Ein in IBM Plex fehlendes Zeichen darf auch halbfett nicht als
        // leeres Kästchen enden – die Rückfallkette muss dieselbe sein.
        let fonts = font_definitions();
        let medium = &fonts.families[&FontFamily::Name(MEDIUM_FAMILY.into())];
        let proportional = &fonts.families[&FontFamily::Proportional];

        assert_eq!(medium.first().map(String::as_str), Some("plex_sans_medium"));
        assert_eq!(
            &medium[1..],
            proportional.as_slice(),
            "hinter dem halbfetten Schnitt muss dieselbe Rückfallkette stehen"
        );
    }
}

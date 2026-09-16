//! The design's colours, fonts and spacing, taken over one to one.
//!
//! Every value comes from `SM2 Mod Loader GUI v2 modern.dc.html`. The design
//! is deliberately dark only; there is no light variant. That is why
//! `install` starts out from `Visuals::dark()` and then overrides every
//! surface the design uses.

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke, Style,
    TextStyle, Visuals,
};
use std::sync::Arc;

/// Turns a hex literal such as `0x3ba6a0` into a colour, so that the values
/// read the same in the source as they do in the design.
const fn hex(value: u32) -> Color32 {
    Color32::from_rgb((value >> 16) as u8, ((value >> 8) & 0xff) as u8, (value & 0xff) as u8)
}

/// Every colour of the design. The names describe the role, not the hue —
/// the design reuses the same fill in several places.
pub mod color {
    use super::hex;
    use egui::Color32;

    /// Fill of the application window (CentralPanel).
    pub const WINDOW: Color32 = hex(0x12141a);
    /// Sidebar and status bar.
    pub const PANEL: Color32 = hex(0x101218);
    /// Top bar above the content.
    pub const TOP_BAR: Color32 = hex(0x14161d);
    /// Cards (mod list, profiles, backups, settings).
    pub const CARD: Color32 = hex(0x161921);
    /// Drop-down menu of the launch choice.
    pub const MENU: Color32 = hex(0x191c24);
    /// Title row of a modal window.
    pub const DIALOG_HEAD: Color32 = hex(0x1a1d25);
    /// Header row of a table.
    pub const TABLE_HEAD: Color32 = hex(0x13161d);
    /// Buttons, and the toggle track in its off state.
    pub const CONTROL: Color32 = hex(0x1b1f27);
    /// Input fields and inset surfaces.
    pub const INPUT: Color32 = hex(0x12141a);
    /// Row under the pointer.
    pub const HOVER: Color32 = hex(0x1a1e26);
    /// Selected row, and the active navigation item.
    pub const SELECTED: Color32 = hex(0x18262a);
    /// Selected row under the pointer.
    pub const SELECTED_HOVER: Color32 = hex(0x1c2d32);
    /// Disabled button (import while the directory is read-only).
    pub const CONTROL_DISABLED: Color32 = hex(0x171a20);

    /// Border of inputs and buttons.
    pub const BORDER: Color32 = hex(0x262b36);
    /// Dividers between panels and card sections.
    pub const BORDER_SOFT: Color32 = hex(0x1e222b);
    /// Dividers between table rows.
    pub const BORDER_ROW: Color32 = hex(0x1b1f27);
    /// Border that stands out from its ground (toggle track, cancel).
    pub const BORDER_STRONG: Color32 = hex(0x2b313d);
    /// Border under the pointer wherever the accent is not used.
    pub const BORDER_HOVER: Color32 = hex(0x3a4250);
    /// Border of a disabled button.
    pub const BORDER_DISABLED: Color32 = hex(0x20242c);

    /// Headings and emphasised text.
    pub const TEXT_STRONG: Color32 = hex(0xe6e9ef);
    /// Body text.
    pub const TEXT: Color32 = hex(0xc8cedb);
    /// Secondary text.
    pub const TEXT_DIM: Color32 = hex(0x9aa3b2);
    /// Secondary text inside tables.
    pub const TEXT_DIM2: Color32 = hex(0x8d96a5);
    /// Labels and explanatory text.
    pub const TEXT_MUTED: Color32 = hex(0x8a94a6);
    /// Column headers, placeholders, icons without a state.
    pub const TEXT_FAINT: Color32 = hex(0x7c8698);

    /// Accent: selection, focus, progress.
    pub const ACCENT: Color32 = hex(0x3ba6a0);
    /// Resting state of the primary button.
    pub const ACCENT_DEEP: Color32 = hex(0x23706c);
    /// Toggle track in its on state.
    pub const ACCENT_MID: Color32 = hex(0x2f8c87);
    /// Text on accent surfaces.
    pub const ACCENT_FG: Color32 = hex(0xe9fbf9);
    /// Backing of a counter pill in the active navigation item.
    pub const ACCENT_SOFT: Color32 = hex(0x13262a);

    /// Warning (adopted, changed, read-only, restore).
    pub const WARN: Color32 = hex(0xe0a341);
    /// Fill behind a warning.
    pub const WARN_BG: Color32 = hex(0x231d14);
    /// Border of a warning.
    pub const WARN_BORDER: Color32 = hex(0x4a3a1c);
    /// Warning under the pointer.
    pub const WARN_BRIGHT: Color32 = hex(0xf0c579);

    /// Error (missing file, delete).
    pub const DANGER: Color32 = hex(0xe0645c);
    /// Fill of a destructive button.
    pub const DANGER_BG: Color32 = hex(0x2a1a19);
    /// Border of a destructive button.
    pub const DANGER_BORDER: Color32 = hex(0x7a3a36);
    /// Destructive button under the pointer.
    pub const DANGER_HOVER_BG: Color32 = hex(0x3a201e);
    /// Text of a destructive button under the pointer.
    pub const DANGER_BRIGHT: Color32 = hex(0xf08a83);
    /// Knob of the confirmation toggle in the restore dialog.
    pub const DANGER_KNOB: Color32 = hex(0xffd9d6);

    /// Notice (newly imported, deferred).
    pub const INFO: Color32 = hex(0x6ea8fe);
    /// Fill behind a notice marker.
    pub const INFO_BG: Color32 = hex(0x1c2530);

    /// Confirmation (game detected, backup verified).
    pub const OK: Color32 = hex(0x5fb98a);
    /// Fill behind a confirmation.
    pub const OK_BG: Color32 = hex(0x16251f);

    /// Dimming behind a modal window.
    pub const OVERLAY: Color32 = Color32::from_rgba_premultiplied(4, 5, 7, 174);
}

/// Heights and spacings the design uses more than once.
pub mod metric {
    /// Top bar above the content.
    pub const TOP_BAR_HEIGHT: f32 = 58.0;
    /// Status bar along the bottom edge.
    pub const STATUS_BAR_HEIGHT: f32 = 30.0;
    /// Width of the sidebar.
    pub const SIDE_BAR_WIDTH: f32 = 208.0;
    /// Inner padding of the content area.
    pub const CONTENT_PADDING: f32 = 14.0;
    /// Gap between the cards in the content area.
    pub const CONTENT_GAP: f32 = 12.0;
    /// Inner padding of a card, horizontally.
    pub const CARD_PADDING: f32 = 14.0;
    /// Toolbar at the head of a card.
    pub const CARD_TOOLBAR_HEIGHT: f32 = 52.0;
    /// Column header of a table.
    pub const TABLE_HEAD_HEIGHT: f32 = 30.0;
    /// Row height of a table.
    pub const ROW_HEIGHT: f32 = 36.0;
    /// Row height in the profile and backup tables.
    pub const LIST_ROW_HEIGHT: f32 = 38.0;
    /// Gap between two columns of a table row.
    pub const COLUMN_GAP: f32 = 10.0;
    /// Height of a button in a toolbar.
    pub const BUTTON_HEIGHT: f32 = 30.0;
    /// Height of a button in the top bar and in a dialog footer.
    pub const BUTTON_HEIGHT_LARGE: f32 = 32.0;
    /// Height of a button in a table row.
    pub const BUTTON_HEIGHT_SMALL: f32 = 26.0;
}

/// A font size in the proportional font (IBM Plex Sans Regular).
pub fn sans(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// A font size in the semi-bold proportional font (IBM Plex Sans Medium).
///
/// Its own family rather than a bold style: `egui` knows no `font-weight`,
/// so every stroke weight is a font file of its own.
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM_FAMILY.into()))
}

/// A font size in the monospaced font (IBM Plex Mono).
pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// Name of the semi-bold family as it is entered into `FontDefinitions`.
const MEDIUM_FAMILY: &str = "medium";

const SANS_REGULAR: &[u8] = include_bytes!("../../assets/fonts/IBMPlexSans-Regular.ttf");
const SANS_MEDIUM: &[u8] = include_bytes!("../../assets/fonts/IBMPlexSans-Medium.ttf");
const MONO_REGULAR: &[u8] = include_bytes!("../../assets/fonts/IBMPlexMono-Regular.ttf");

/// Registers IBM Plex and installs the design's style.
pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    // `egui` keeps a separate style per appearance. The design is meant to
    // be dark only, so both get the same one — otherwise the look would
    // depend on whatever the desktop happens to report.
    let style = style();
    ctx.all_styles_mut(|target| *target = style.clone());
}

/// IBM Plex in three styles, each with the fonts `egui` ships as a
/// fallback — IBM Plex does not cover the character sets `egui` uses
/// internally to draw menus and error messages.
fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(font_definitions());
}

/// The font definitions as a plain value, so what gets registered can be
/// checked without building up a drawing context.
fn font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert("plex_sans".into(), Arc::new(FontData::from_static(SANS_REGULAR)));
    fonts.font_data.insert("plex_sans_medium".into(), Arc::new(FontData::from_static(SANS_MEDIUM)));
    fonts.font_data.insert("plex_mono".into(), Arc::new(FontData::from_static(MONO_REGULAR)));

    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "plex_sans".into());
    fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "plex_mono".into());

    // The semi-bold family inherits the same fallback fonts as the
    // proportional one, so that a character missing from IBM Plex does not
    // show up as an empty box in a heading either.
    let mut medium_family = vec![String::from("plex_sans_medium")];
    medium_family.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
    fonts.families.insert(FontFamily::Name(MEDIUM_FAMILY.into()), medium_family);

    fonts
}

/// The design's style: dark fills, 10 px corner radii, no default border
/// around buttons — the widgets in `super::widgets` paint every surface
/// themselves.
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
    fn hex_converts_as_noted_in_the_design() {
        assert_eq!(hex(0x3ba6a0), Color32::from_rgb(0x3b, 0xa6, 0xa0), "the design's accent color");
        assert_eq!(hex(0x000000), Color32::BLACK, "the zero value yields black");
        assert_eq!(hex(0xffffff), Color32::WHITE, "the maximum value yields white");
    }

    #[test]
    fn ibm_plex_comes_first_in_every_font_family() {
        let fonts = font_definitions();
        assert_eq!(
            fonts.families[&FontFamily::Proportional].first().map(String::as_str),
            Some("plex_sans"),
            "the proportional font must be IBM Plex Sans"
        );
        assert_eq!(
            fonts.families[&FontFamily::Monospace].first().map(String::as_str),
            Some("plex_mono"),
            "the monospaced font must be IBM Plex Mono"
        );
    }

    #[test]
    fn the_medium_family_falls_back_to_the_same_fonts_as_the_proportional_one() {
        // A character missing from IBM Plex must not end up as an empty box
        // in semi-bold either — the fallback chain has to be the same.
        let fonts = font_definitions();
        let medium = &fonts.families[&FontFamily::Name(MEDIUM_FAMILY.into())];
        let proportional = &fonts.families[&FontFamily::Proportional];

        assert_eq!(medium.first().map(String::as_str), Some("plex_sans_medium"));
        assert_eq!(
            &medium[1..],
            proportional.as_slice(),
            "the same fallback chain must sit behind the semi-bold cut"
        );
    }
}

//! Die Bausteine des Entwurfs: Schalter, Statuspillen, Schaltflächen,
//! Eingabefelder, Karten und Tabellenspalten.
//!
//! `egui` bringt für all das eigene Widgets mit, deren Aussehen sich aber
//! nur über `Visuals` global steuern lässt – der Entwurf benutzt pro Rolle
//! andere Flächen, Rahmen und Höhen. Deshalb zeichnet dieses Modul die
//! Flächen selbst und benutzt von `egui` nur die Flächenvergabe
//! (`allocate_exact_size`) und die Ereignisbehandlung (`Response`).

use super::icons;
use super::theme::{color, medium, metric, sans};
use egui::{
    Color32, CornerRadius, CursorIcon, FontId, Frame, Margin, Pos2, Rect, Response, Sense,
    Stroke, StrokeKind, TextFormat, Ui, Vec2,
};

/// Ein Symbol des Entwurfs, damit Schaltflächen und Zeilen dasselbe
/// Symbol über einen Wert statt über einen Funktionszeiger benennen können.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Plus,
    Play,
    Check,
    Cross,
    Warning,
    Dot,
    Ring,
    ChevronDown,
    Search,
    Grip,
    TriangleUp,
    TriangleDown,
    NavMods,
    NavProfiles,
    NavSaves,
    NavSettings,
}

impl Icon {
    pub fn paint(self, painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
        match self {
            Self::Plus => icons::plus(painter, center, size, color),
            Self::Play => icons::play(painter, center, size, color),
            Self::Check => icons::check(painter, center, size, color),
            Self::Cross => icons::cross(painter, center, size, color),
            Self::Warning => icons::warning(painter, center, size, color),
            Self::Dot => icons::dot(painter, center, size, color),
            Self::Ring => icons::ring(painter, center, size, color),
            Self::ChevronDown => icons::chevron_down(painter, center, size, color),
            Self::Search => icons::search(painter, center, size, color),
            Self::Grip => icons::grip(painter, center, size, color),
            Self::TriangleUp => icons::triangle(painter, center, size, color, true),
            Self::TriangleDown => icons::triangle(painter, center, size, color, false),
            Self::NavMods => icons::nav_mods(painter, center, size, color),
            Self::NavProfiles => icons::nav_profiles(painter, center, size, color),
            Self::NavSaves => icons::nav_saves(painter, center, size, color),
            Self::NavSettings => icons::nav_settings(painter, center, size, color),
        }
    }
}

/// Aussehen einer Schaltfläche. Der Entwurf kennt fünf Ausprägungen, die
/// sich nur in Fläche, Rahmen und Schriftfarbe unterscheiden – Höhe,
/// Innenabstand und Schriftgrad kommen je Einsatzort dazu.
#[derive(Debug, Clone)]
pub struct ButtonStyle {
    pub background: Color32,
    pub background_hovered: Color32,
    pub border: Option<Color32>,
    pub border_hovered: Option<Color32>,
    pub foreground: Color32,
    pub foreground_hovered: Color32,
    pub height: f32,
    pub padding_x: f32,
    pub font: FontId,
    pub corner_radius: u8,
}

impl ButtonStyle {
    /// Hauptschaltfläche („Starten“, „Dateien wählen“, „Übernehmen“).
    pub fn primary() -> Self {
        Self {
            background: color::ACCENT_DEEP,
            background_hovered: color::ACCENT,
            border: None,
            border_hovered: None,
            foreground: color::ACCENT_FG,
            foreground_hovered: color::ACCENT_FG,
            height: metric::BUTTON_HEIGHT_LARGE,
            padding_x: 15.0,
            font: medium(12.5),
            corner_radius: 8,
        }
    }

    /// Nebenschaltfläche mit Rahmen, die unter dem Zeiger den Akzent annimmt.
    pub fn ghost() -> Self {
        Self {
            background: color::CONTROL,
            background_hovered: color::CONTROL,
            border: Some(color::BORDER),
            border_hovered: Some(color::ACCENT),
            foreground: color::TEXT,
            foreground_hovered: color::TEXT_STRONG,
            height: metric::BUTTON_HEIGHT,
            padding_x: 13.0,
            font: sans(12.5),
            corner_radius: 7,
        }
    }

    /// „Abbrechen“ in einem modalen Fenster: hellerer Rahmen, heller Text.
    pub fn neutral() -> Self {
        Self {
            background: color::CONTROL,
            background_hovered: color::CONTROL,
            border: Some(color::BORDER_HOVER),
            border_hovered: Some(color::TEXT_FAINT),
            foreground: color::TEXT_STRONG,
            foreground_hovered: color::TEXT_STRONG,
            height: metric::BUTTON_HEIGHT_LARGE,
            padding_x: 15.0,
            font: medium(12.5),
            corner_radius: 8,
        }
    }

    /// Zerstörende Schaltfläche („Löschen“, „Trotzdem wiederherstellen“).
    pub fn danger() -> Self {
        Self {
            background: color::DANGER_BG,
            background_hovered: color::DANGER_HOVER_BG,
            border: Some(color::DANGER_BORDER),
            border_hovered: Some(color::DANGER_BORDER),
            foreground: color::DANGER,
            foreground_hovered: color::DANGER_BRIGHT,
            height: metric::BUTTON_HEIGHT_LARGE,
            padding_x: 15.0,
            font: medium(12.5),
            corner_radius: 8,
        }
    }

    /// Warnende Schaltfläche („⚠ Wiederherstellen“ in der Backup-Liste).
    pub fn warning() -> Self {
        Self {
            background: color::WARN_BG,
            background_hovered: color::WARN_BG,
            border: Some(color::WARN_BORDER),
            border_hovered: Some(color::WARN),
            foreground: color::WARN,
            foreground_hovered: color::WARN_BRIGHT,
            height: metric::BUTTON_HEIGHT_SMALL,
            padding_x: 11.0,
            font: medium(11.5),
            corner_radius: 6,
        }
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    pub fn padding_x(mut self, padding: f32) -> Self {
        self.padding_x = padding;
        self
    }

    pub fn font(mut self, font: FontId) -> Self {
        self.font = font;
        self
    }

    pub fn corner_radius(mut self, radius: u8) -> Self {
        self.corner_radius = radius;
        self
    }

    /// Kleine Schaltfläche in einer Tabellenzeile (26 px, Radius 6).
    pub fn small(self) -> Self {
        self.height(metric::BUTTON_HEIGHT_SMALL).padding_x(11.0).font(sans(11.5)).corner_radius(6)
    }
}

/// Zeichnet eine Schaltfläche des Entwurfs und meldet den Klick zurück.
///
/// Eine gesperrte Schaltfläche bleibt sichtbar (der Entwurf blendet sie nicht
/// aus, sondern nimmt ihr Fläche, Rahmen und Schriftfarbe) und zeigt unter
/// dem Zeiger „nicht erlaubt“ statt der Hand.
pub fn button(
    ui: &mut Ui,
    style: &ButtonStyle,
    icon: Option<Icon>,
    label: &str,
    enabled: bool,
) -> Response {
    let icon_size = style.font.size * 0.85;
    let icon_gap = 7.0;
    let galley =
        ui.painter().layout_no_wrap(label.to_owned(), style.font.clone(), Color32::PLACEHOLDER);
    let icon_width = if icon.is_some() { icon_size + icon_gap } else { 0.0 };
    let width = style.padding_x * 2.0 + icon_width + galley.size().x;

    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, style.height), sense);

    let hovered = enabled && response.hovered();
    let (background, foreground, border) = if enabled {
        if hovered {
            (style.background_hovered, style.foreground_hovered, style.border_hovered)
        } else {
            (style.background, style.foreground, style.border)
        }
    } else {
        (color::CONTROL_DISABLED, color::TEXT_FAINT, Some(color::BORDER_DISABLED))
    };

    let painter = ui.painter();
    let radius = CornerRadius::same(style.corner_radius);
    painter.rect_filled(rect, radius, background);
    if let Some(border) = border {
        painter.rect_stroke(rect, radius, Stroke::new(1.0, border), StrokeKind::Inside);
    }

    let mut cursor = rect.left() + style.padding_x;
    if let Some(icon) = icon {
        icon.paint(
            painter,
            Pos2::new(cursor + icon_size / 2.0, rect.center().y),
            icon_size,
            foreground,
        );
        cursor += icon_size + icon_gap;
    }
    painter.galley(
        Pos2::new(cursor, rect.center().y - galley.size().y / 2.0),
        galley,
        foreground,
    );

    if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response.on_hover_cursor(CursorIcon::NotAllowed)
    }
}

/// Der Schalter, mit dem der Entwurf Haken ersetzt: 30 × 17 px, Knopf 12 px.
///
/// Die Bewegung des Knopfes läuft über `animate_bool_with_time` – laut
/// Anmerkung des Entwurfs ausdrücklich erlaubt, weil es nur eine Interpolation
/// zwischen zwei Positionen ist.
pub fn toggle(ui: &mut Ui, on: bool, enabled: bool) -> Response {
    toggle_colored(ui, on, enabled, color::ACCENT_MID, color::ACCENT, color::ACCENT_FG)
}

/// Schalter in abweichenden Farben – der Wiederherstellen-Dialog benutzt für
/// die Risikobestätigung Rot statt des Akzents.
pub fn toggle_colored(
    ui: &mut Ui,
    on: bool,
    enabled: bool,
    track_on: Color32,
    border_on: Color32,
    knob_on: Color32,
) -> Response {
    let size = Vec2::new(30.0, 17.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);

    let progress = ui.ctx().animate_bool_with_time(response.id, on, 0.12);
    let (track, border, knob) = if on {
        (track_on, border_on, knob_on)
    } else {
        (color::CONTROL, color::BORDER_STRONG, color::TEXT_FAINT)
    };

    let painter = ui.painter();
    let radius = CornerRadius::same(9);
    painter.rect_filled(rect, radius, track);
    painter.rect_stroke(rect, radius, Stroke::new(1.0, border), StrokeKind::Inside);

    let knob_radius = 6.0;
    let left = rect.left() + 3.0 + knob_radius;
    let right = rect.right() - 3.0 - knob_radius;
    painter.circle_filled(
        Pos2::new(left + (right - left) * progress, rect.center().y),
        knob_radius,
        knob,
    );

    if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response.on_hover_cursor(CursorIcon::NotAllowed)
    }
}

/// Eine Statuspille wie „⚠ GEÄNDERT“ oder „✓ geprüft, 41 Dateien“.
pub fn badge(ui: &mut Ui, icon: Option<Icon>, label: &str, foreground: Color32, background: Color32) {
    let font = medium(10.0);
    let galley = ui.painter().layout_job(tracked_text(label, font.clone(), foreground, 0.6));
    let icon_size = 8.0;
    let icon_width = if icon.is_some() { icon_size + 5.0 } else { 0.0 };
    let width = 16.0 + icon_width + galley.size().x;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 18.0), Sense::hover());

    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(9), background);
    let mut cursor = rect.left() + 8.0;
    if let Some(icon) = icon {
        icon.paint(
            painter,
            Pos2::new(cursor + icon_size / 2.0, rect.center().y),
            icon_size,
            foreground,
        );
        cursor += icon_size + 5.0;
    }
    painter.galley(
        Pos2::new(cursor, rect.center().y - galley.size().y / 2.0),
        galley,
        foreground,
    );
}

/// Text mit Sperrung, wie ihn der Entwurf für Spaltenköpfe und Pillen
/// benutzt (`letter-spacing`). `egui` kennt keine Sperrung als Stilangabe,
/// wohl aber `TextFormat::extra_letter_spacing` in einem `LayoutJob`.
pub fn tracked_text(
    text: &str,
    font: FontId,
    color: Color32,
    extra_letter_spacing: f32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        0.0,
        TextFormat { font_id: font, color, extra_letter_spacing, ..Default::default() },
    );
    job
}

/// Rahmen einer Karte: Fläche, 1-px-Rahmen, 12-px-Rundung – als `Frame`,
/// wenn die Höhe aus dem Inhalt folgen soll statt vorab festzustehen.
pub fn card() -> Frame {
    Frame::new()
        .fill(color::CARD)
        .stroke(Stroke::new(1.0, color::BORDER_SOFT))
        .corner_radius(CornerRadius::same(12))
}

/// Rahmen eines eingelassenen Kastens (Pfadlisten in Dialogen und im
/// Erstlauf-Bildschirm): dunklere Fläche, 9-px-Rundung.
pub fn inset() -> Frame {
    Frame::new()
        .fill(color::INPUT)
        .stroke(Stroke::new(1.0, color::BORDER))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin { left: 13, right: 13, top: 11, bottom: 11 })
}

/// Ein einzeiliges Eingabefeld in der Form des Entwurfs: 30 px hoch,
/// eingelassene Fläche, 7-px-Rundung, optionales Symbol davor.
pub fn text_field(
    ui: &mut Ui,
    value: &mut String,
    placeholder: &str,
    width: f32,
    icon: Option<Icon>,
) -> Response {
    let height = metric::BUTTON_HEIGHT;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());

    let painter = ui.painter();
    let radius = CornerRadius::same(7);
    painter.rect_filled(rect, radius, color::INPUT);
    painter.rect_stroke(rect, radius, Stroke::new(1.0, color::BORDER), StrokeKind::Inside);

    let mut content = rect.shrink2(Vec2::new(11.0, 0.0));
    if let Some(icon) = icon {
        icon.paint(ui.painter(), Pos2::new(content.left() + 5.5, rect.center().y), 11.0, color::TEXT_FAINT);
        content.set_left(content.left() + 19.0);
    }

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.add(
        egui::TextEdit::singleline(value)
            .frame(Frame::NONE)
            .desired_width(content.width())
            .font(sans(12.5))
            .text_color(color::TEXT_STRONG)
            .hint_text(egui::RichText::new(placeholder).font(sans(12.5)).color(color::TEXT_FAINT)),
    )
}

/// Breitenangabe einer Tabellenspalte.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Column {
    /// Feste Breite in Punkten.
    Fixed(f32),
    /// Nimmt den Rest der Zeile ein (`1fr` im Entwurf).
    Flexible,
}

/// Rechnet die Spaltenangaben einer Zeile in Rechtecke um – das Gegenstück
/// zu `grid-template-columns` samt `gap` aus dem Entwurf.
///
/// Mehrere flexible Spalten teilen sich den Rest zu gleichen Teilen. Bleibt
/// nichts übrig (sehr schmales Fenster), bekommen sie Breite 0 statt einer
/// negativen Breite, damit die Zeile nicht über ihren Rand hinauswächst.
pub fn columns(row: Rect, spec: &[Column], gap: f32) -> Vec<Rect> {
    let fixed: f32 = spec
        .iter()
        .map(|c| match c {
            Column::Fixed(w) => *w,
            Column::Flexible => 0.0,
        })
        .sum();
    let flexible_count = spec.iter().filter(|c| **c == Column::Flexible).count();
    let gaps = gap * spec.len().saturating_sub(1) as f32;
    let remaining = (row.width() - fixed - gaps).max(0.0);
    let per_flexible = if flexible_count > 0 { remaining / flexible_count as f32 } else { 0.0 };

    let mut rects = Vec::with_capacity(spec.len());
    let mut x = row.left();
    for column in spec {
        let width = match column {
            Column::Fixed(w) => *w,
            Column::Flexible => per_flexible,
        };
        rects.push(Rect::from_min_size(Pos2::new(x, row.top()), Vec2::new(width, row.height())));
        x += width + gap;
    }
    rects
}

/// Setzt Text einzeilig und kürzt ihn mit einem Auslassungszeichen, wenn er
/// nicht passt – das Gegenstück zu `white-space: nowrap` plus
/// `text-overflow: ellipsis` aus dem Entwurf.
///
/// Der Unterschied zu einem gewöhnlichen Umbruch ist in einer Tabelle nicht
/// kosmetisch: bricht ein langer Dateiname um, wächst die Zeile und schiebt
/// ihren Inhalt in die Nachbarspalte.
pub fn truncated(
    ui: &Ui,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        TextFormat { font_id: font, color, ..Default::default() },
    );
    job.wrap = egui::text::TextWrapping {
        max_width: max_width.max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.painter().layout_job(job)
}

/// Schreibt gekürzten Text linksbündig und senkrecht zentriert in eine
/// Spalte. Eine Spalte ohne nutzbare Breite bleibt leer, statt ihren Text
/// über den Nachbarn zu legen.
pub fn column_text(ui: &Ui, rect: Rect, text: &str, font: FontId, color: Color32) {
    if rect.width() < 8.0 {
        return;
    }
    let galley = truncated(ui, text, font, color, rect.width());
    ui.painter().galley(
        Pos2::new(rect.left(), rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(width: f32) -> Rect {
        Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(width, 36.0))
    }

    #[test]
    fn eine_flexible_spalte_bekommt_den_rest_der_zeile() {
        // Der Kopf der Mod-Liste: 22 44 1fr 72 124 56 bei 10 px Abstand.
        let spec = [
            Column::Fixed(22.0),
            Column::Fixed(44.0),
            Column::Flexible,
            Column::Fixed(72.0),
            Column::Fixed(124.0),
            Column::Fixed(56.0),
        ];
        let rects = columns(row(800.0), &spec, 10.0);
        assert_eq!(rects.len(), 6, "jede Spalte bekommt ein Rechteck");
        assert_eq!(rects[2].width(), 800.0 - 318.0 - 50.0, "die flexible Spalte nimmt den Rest");
        assert_eq!(rects[0].left(), 0.0, "die erste Spalte beginnt am linken Rand");
        assert_eq!(
            rects[5].right(),
            800.0,
            "die letzte Spalte endet genau am rechten Rand der Zeile"
        );
    }

    #[test]
    fn eine_zu_schmale_zeile_erzeugt_keine_negative_breite() {
        let spec = [Column::Fixed(200.0), Column::Flexible, Column::Fixed(200.0)];
        let rects = columns(row(120.0), &spec, 10.0);
        assert_eq!(rects[1].width(), 0.0, "die flexible Spalte schrumpft auf null statt ins Minus");
    }

    #[test]
    fn mehrere_flexible_spalten_teilen_sich_den_rest_zu_gleichen_teilen() {
        let spec = [Column::Flexible, Column::Fixed(100.0), Column::Flexible];
        let rects = columns(row(400.0), &spec, 10.0);
        assert_eq!(rects[0].width(), 140.0, "erste flexible Spalte");
        assert_eq!(rects[2].width(), 140.0, "zweite flexible Spalte, gleich breit");
    }
}

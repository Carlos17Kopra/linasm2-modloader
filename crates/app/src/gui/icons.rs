//! Die Symbole des Entwurfs, selbst gezeichnet statt als Schriftzeichen.
//!
//! Der Entwurf benutzt Zeichen wie `⠿`, `⏷`, `▣` oder `⚙`. Keines davon
//! steht in IBM Plex, und die Rückfallschriften von `egui` decken sie nur
//! lückenhaft ab – auf einem fremden System entschiede also der Zufall der
//! installierten Schriften darüber, ob an diesen Stellen ein Symbol oder ein
//! leeres Kästchen erscheint. Alle Symbole werden deshalb über
//! `ui.painter()` gezeichnet; das ist zugleich der Weg, den der Entwurf in
//! seinen eigenen Anmerkungen vorschlägt.
//!
//! `size` ist jeweils die Kantenlänge des gedachten Schriftkegels, damit die
//! Aufrufe dieselben Zahlen tragen wie die `font-size`-Angaben im Entwurf.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2};

/// Strichstärke, die zu einem Symbol dieser Größe passt.
fn line_width(size: f32) -> f32 {
    (size * 0.11).clamp(1.0, 1.8)
}

fn stroke(size: f32, color: Color32) -> Stroke {
    Stroke::new(line_width(size), color)
}

/// `⠿` – Anfasser zum Ziehen einer Zeile: zwei Spalten zu je drei Punkten.
pub fn grip(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    let radius = size * 0.085;
    let dx = size * 0.19;
    let dy = size * 0.26;
    for row in -1..=1 {
        for column in [-1.0_f32, 1.0] {
            let position =
                Pos2::new(center.x + column * dx, center.y + row as f32 * dy);
            painter.circle_filled(position, radius, color);
        }
    }
}

/// `⏷` – Pfeil nach unten an einem Aufklappfeld.
pub fn chevron_down(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    let half = size * 0.38;
    let depth = size * 0.24;
    painter.add(Shape::line(
        vec![
            Pos2::new(center.x - half, center.y - depth),
            Pos2::new(center.x, center.y + depth),
            Pos2::new(center.x + half, center.y - depth),
        ],
        stroke(size, color),
    ));
}

/// `▶` – Wiedergabedreieck auf der Schaltfläche „Starten“.
pub fn play(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    let half_height = size * 0.44;
    let half_width = size * 0.38;
    painter.add(Shape::convex_polygon(
        vec![
            Pos2::new(center.x - half_width, center.y - half_height),
            Pos2::new(center.x + half_width, center.y),
            Pos2::new(center.x - half_width, center.y + half_height),
        ],
        color,
        Stroke::NONE,
    ));
}

/// `▲` und `▼` – gefüllte Dreiecke der Rang-Schaltflächen.
pub fn triangle(painter: &Painter, center: Pos2, size: f32, color: Color32, up: bool) {
    let half_width = size * 0.5;
    let half_height = size * 0.34;
    let tip = if up { -half_height } else { half_height };
    let base = -tip;
    painter.add(Shape::convex_polygon(
        vec![
            Pos2::new(center.x, center.y + tip),
            Pos2::new(center.x - half_width, center.y + base),
            Pos2::new(center.x + half_width, center.y + base),
        ],
        color,
        Stroke::NONE,
    ));
}

/// `＋` – Pluszeichen der Import-Schaltfläche.
pub fn plus(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    let half = size * 0.42;
    let line = stroke(size, color);
    painter.line_segment(
        [Pos2::new(center.x - half, center.y), Pos2::new(center.x + half, center.y)],
        line,
    );
    painter.line_segment(
        [Pos2::new(center.x, center.y - half), Pos2::new(center.x, center.y + half)],
        line,
    );
}

/// `⌕` – Lupe im Filterfeld.
pub fn search(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    let radius = size * 0.31;
    let lens = Pos2::new(center.x - size * 0.08, center.y - size * 0.08);
    let line = stroke(size, color);
    painter.circle_stroke(lens, radius, line);
    let start = Pos2::new(lens.x + radius * 0.72, lens.y + radius * 0.72);
    painter.line_segment(
        [start, Pos2::new(center.x + size * 0.42, center.y + size * 0.42)],
        line,
    );
}

/// `✓` – Haken.
pub fn check(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    painter.add(Shape::line(
        vec![
            Pos2::new(center.x - size * 0.40, center.y + size * 0.03),
            Pos2::new(center.x - size * 0.10, center.y + size * 0.32),
            Pos2::new(center.x + size * 0.42, center.y - size * 0.32),
        ],
        stroke(size, color),
    ));
}

/// `✕` – Kreuz zum Schließen und als Fehlermerker.
pub fn cross(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    let half = size * 0.36;
    let line = stroke(size, color);
    painter.line_segment(
        [
            Pos2::new(center.x - half, center.y - half),
            Pos2::new(center.x + half, center.y + half),
        ],
        line,
    );
    painter.line_segment(
        [
            Pos2::new(center.x + half, center.y - half),
            Pos2::new(center.x - half, center.y + half),
        ],
        line,
    );
}

/// `⚠` – Warndreieck mit Ausrufezeichen.
pub fn warning(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    let half_width = size * 0.48;
    let half_height = size * 0.42;
    let top = Pos2::new(center.x, center.y - half_height);
    let left = Pos2::new(center.x - half_width, center.y + half_height);
    let right = Pos2::new(center.x + half_width, center.y + half_height);
    painter.add(Shape::convex_polygon(vec![top, left, right], Color32::TRANSPARENT, stroke(size, color)));

    let bar = Stroke::new(line_width(size), color);
    painter.line_segment(
        [
            Pos2::new(center.x, center.y - size * 0.12),
            Pos2::new(center.x, center.y + size * 0.14),
        ],
        bar,
    );
    painter.circle_filled(
        Pos2::new(center.x, center.y + size * 0.29),
        line_width(size) * 0.55,
        color,
    );
}

/// `●` – gefüllter Punkt.
pub fn dot(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    painter.circle_filled(center, size * 0.27, color);
}

/// `○` – leerer Kreis (Merkmal nicht verfügbar).
pub fn ring(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    painter.circle_stroke(center, size * 0.27, stroke(size, color));
}

/// `▮` – Navigationssymbol „Mods“: ein stehender Balken.
pub fn nav_mods(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    let rect = Rect::from_center_size(center, Vec2::new(size * 0.42, size * 0.78));
    painter.rect_filled(rect, 1.0, color);
}

/// `▤` – Navigationssymbol „Profile“: ein Kasten mit Zeilen.
pub fn nav_profiles(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    let rect = Rect::from_center_size(center, Vec2::new(size * 0.82, size * 0.72));
    let line = stroke(size, color);
    painter.rect_stroke(rect, 1.0, line, egui::StrokeKind::Inside);
    for step in 1..3 {
        let y = rect.top() + rect.height() * step as f32 / 3.0;
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            line,
        );
    }
}

/// `▣` – Navigationssymbol „Savegames“: Kasten im Kasten.
pub fn nav_saves(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    let outer = Rect::from_center_size(center, Vec2::new(size * 0.82, size * 0.78));
    painter.rect_stroke(outer, 1.0, stroke(size, color), egui::StrokeKind::Inside);
    let inner = Rect::from_center_size(center, Vec2::new(size * 0.36, size * 0.34));
    painter.rect_filled(inner, 0.0, color);
}

/// `⚙` – Navigationssymbol „Einstellungen“: Zahnrad mit acht Zähnen.
pub fn nav_settings(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    let line = stroke(size, color);
    let radius = size * 0.30;
    painter.circle_stroke(center, radius, line);
    for index in 0..8 {
        let angle = std::f32::consts::TAU * index as f32 / 8.0;
        let (sin, cos) = angle.sin_cos();
        painter.line_segment(
            [
                Pos2::new(center.x + cos * radius, center.y + sin * radius),
                Pos2::new(center.x + cos * size * 0.48, center.y + sin * size * 0.48),
            ],
            line,
        );
    }
}

/// Auswahlring mit Punkt, wie ihn der Dialog „Steam-Nutzerprofil wählen“
/// zeigt: 14 px Ring, 7 px Punkt.
pub fn radio(painter: &Painter, center: Pos2, ring_color: Color32, dot_color: Color32) {
    painter.circle_stroke(center, 7.0, Stroke::new(1.0, ring_color));
    if dot_color != Color32::TRANSPARENT {
        painter.circle_filled(center, 3.5, dot_color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn die_strichstaerke_bleibt_in_den_groessen_des_entwurfs_sichtbar() {
        // Die kleinsten Symbole des Entwurfs sind 9 px, die größten 15 px.
        assert!(line_width(9.0) >= 1.0, "9-px-Symbol wäre unsichtbar dünn");
        assert!(line_width(15.0) <= 1.8, "15-px-Symbol wäre zu fett für den Entwurf");
    }
}

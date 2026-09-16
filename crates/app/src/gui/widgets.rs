//! The building blocks of the design: toggles, status pills, buttons, input
//! fields, cards and table columns.
//!
//! `egui` ships widgets for all of this, but their look can only be steered
//! globally through `Visuals` — and the design uses different fills, borders
//! and heights for each role. So this module paints the surfaces itself and
//! takes only the space allocation (`allocate_exact_size`) and the event
//! handling (`Response`) from `egui`.

use super::icons;
use super::theme::{color, medium, metric, sans};
use egui::{
    Color32, CornerRadius, CursorIcon, FontId, Frame, Margin, Pos2, Rect, Response, Sense,
    Stroke, StrokeKind, TextFormat, Ui, Vec2,
};

/// One of the design's icons, so that buttons and rows can name the same
/// icon through a value instead of through a function pointer.
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
    Pencil,
    Folder,
    Trash,
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
            Self::Pencil => icons::pencil(painter, center, size, color),
            Self::Folder => icons::folder(painter, center, size, color),
            Self::Trash => icons::trash(painter, center, size, color),
        }
    }
}

/// The look of a button. The design has five variants that differ only in
/// fill, border and text colour — height, padding and font size are added
/// per place of use.
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
    /// The primary button ("Starten", "Dateien wählen", "Übernehmen").
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

    /// A secondary button with a border, which takes on the accent colour
    /// under the pointer.
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

    /// "Abbrechen" in a modal window: lighter border, lighter text.
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

    /// A destructive button ("Löschen", "Trotzdem wiederherstellen").
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

    /// A warning button ("⚠ Wiederherstellen" in the backup list).
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

    /// A small button in a table row (26 px, radius 6).
    pub fn small(self) -> Self {
        self.height(metric::BUTTON_HEIGHT_SMALL).padding_x(11.0).font(sans(11.5)).corner_radius(6)
    }
}

/// Draws one of the design's buttons and reports back whether it was
/// clicked.
///
/// A disabled button stays visible — the design does not hide it, it drains
/// its fill, border and text colour — and shows a "not allowed" cursor
/// instead of the pointing hand.
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

/// The toggle the design uses in place of check marks: 30 × 17 px, with a
/// 12 px knob.
///
/// The knob's movement runs through `animate_bool_with_time`. The design's
/// own notes explicitly allow this, because it is nothing more than an
/// interpolation between two positions.
pub fn toggle(ui: &mut Ui, on: bool, enabled: bool) -> Response {
    toggle_colored(ui, on, enabled, color::ACCENT_MID, color::ACCENT, color::ACCENT_FG)
}

/// A toggle in different colours — the restore dialog uses red instead of
/// the accent for its risk confirmation.
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

/// A status pill such as "⚠ GEÄNDERT" or "✓ geprüft, 41 Dateien".
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

/// Letter-spaced text, the way the design uses it for column headers and
/// pills (`letter-spacing`). `egui` has no letter spacing as a style
/// setting, but it does have `TextFormat::extra_letter_spacing` inside a
/// `LayoutJob`.
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

/// The frame of a card: fill, 1 px border, 12 px corner radius — as a
/// `Frame`, for when the height should follow from the content instead of
/// standing fixed up front.
pub fn card() -> Frame {
    Frame::new()
        .fill(color::CARD)
        .stroke(Stroke::new(1.0, color::BORDER_SOFT))
        .corner_radius(CornerRadius::same(12))
}

/// The frame of an inset box (path lists in dialogs and on the first-run
/// screen): darker fill, 9 px corner radius.
pub fn inset() -> Frame {
    Frame::new()
        .fill(color::INPUT)
        .stroke(Stroke::new(1.0, color::BORDER))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin { left: 13, right: 13, top: 11, bottom: 11 })
}

/// A single-line input field in the shape of the design: 30 px tall, inset
/// fill, 7 px corner radius, optional icon in front of it.
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

/// The width specification of a table column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Column {
    /// A fixed width in points.
    Fixed(f32),
    /// Takes up the rest of the row (`1fr` in the design).
    Flexible,
}

/// Turns a row's column specification into rectangles — the counterpart to
/// `grid-template-columns` and its `gap` from the design.
///
/// Several flexible columns share what is left in equal parts. If nothing
/// is left over (a very narrow window) they get width 0 rather than a
/// negative width, so the row does not grow past its own edge.
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

/// Lays text out on a single line and shortens it with an ellipsis when it
/// does not fit — the counterpart to `white-space: nowrap` plus
/// `text-overflow: ellipsis` from the design.
///
/// In a table the difference from ordinary wrapping is not cosmetic: if a
/// long file name wraps, the row grows and pushes its content into the
/// neighbouring column.
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

/// Width of a context menu. Fixed, so that every entry gets the same hit
/// area — with a width derived from the longest entry, the clickable area
/// would change with the labels.
pub const MENU_WIDTH: f32 = 204.0;

/// One entry of a context menu; returns whether it was clicked.
///
/// `egui` provides the shell — fill, border, shadow, opening and closing —
/// through `Response::context_menu`; the row, the text and the colours this
/// function paints itself, for the same reason as in `button`: `egui`'s own
/// menu entries can only be styled globally through `Visuals`, and would
/// not match the design.
pub fn menu_item(ui: &mut Ui, icon: Option<Icon>, label: &str, enabled: bool, danger: bool) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());

    let base = if danger { color::DANGER } else { color::TEXT };
    let foreground = if enabled { base } else { base.gamma_multiply(0.4) };
    let highlight = enabled && response.hovered();
    if highlight {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(6),
            if danger { color::DANGER_BG } else { color::HOVER },
        );
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let mut x = rect.left() + 9.0;
    if let Some(icon) = icon {
        icon.paint(ui.painter(), Pos2::new(x + 5.5, rect.center().y), 11.0, foreground);
    }
    x += 20.0;
    ui.painter().text(
        Pos2::new(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        sans(12.5),
        if highlight && !danger { color::TEXT_STRONG } else { foreground },
    );

    // A disabled entry does not close the menu: the click should stay
    // without consequence, not take the menu away and do nothing else.
    let clicked = enabled && response.clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// The divider between two groups of menu entries.
pub fn menu_separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 7.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, color::BORDER_SOFT));
}

/// Writes shortened text into a column, left-aligned and vertically
/// centred. A column without usable width stays empty instead of laying its
/// text over its neighbour.
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
    fn a_flexible_column_gets_the_rest_of_the_row() {
        // The header of the mod list: 22 44 1fr 72 124 56 with a 10 px gap.
        let spec = [
            Column::Fixed(22.0),
            Column::Fixed(44.0),
            Column::Flexible,
            Column::Fixed(72.0),
            Column::Fixed(124.0),
            Column::Fixed(56.0),
        ];
        let rects = columns(row(800.0), &spec, 10.0);
        assert_eq!(rects.len(), 6, "every column gets a rectangle");
        assert_eq!(rects[2].width(), 800.0 - 318.0 - 50.0, "the flexible column takes the rest");
        assert_eq!(rects[0].left(), 0.0, "the first column starts at the left edge");
        assert_eq!(
            rects[5].right(),
            800.0,
            "the last column ends exactly at the right edge of the row"
        );
    }

    #[test]
    fn a_row_that_is_too_narrow_produces_no_negative_width() {
        let spec = [Column::Fixed(200.0), Column::Flexible, Column::Fixed(200.0)];
        let rects = columns(row(120.0), &spec, 10.0);
        assert_eq!(
            rects[1].width(),
            0.0,
            "the flexible column shrinks to zero rather than going negative"
        );
    }

    #[test]
    fn several_flexible_columns_share_the_rest_in_equal_parts() {
        let spec = [Column::Flexible, Column::Fixed(100.0), Column::Flexible];
        let rects = columns(row(400.0), &spec, 10.0);
        assert_eq!(rects[0].width(), 140.0, "first flexible column");
        assert_eq!(rects[2].width(), 140.0, "second flexible column, same width");
    }
}

//! The sidebar: navigation at the top, status card at the bottom.

use super::theme::{color, medium, mono, sans};
use super::widgets::Icon;
use super::{Action, App, Section};
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use sm2_core::t;

pub fn show(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let items = [
        (Section::Mods, Icon::NavMods, t!("gui.side_bar.nav_mods"), mods_count(app)),
        (
            Section::Profiles,
            Icon::NavProfiles,
            t!("gui.side_bar.nav_profiles"),
            app.profiles.len().to_string(),
        ),
        (Section::Saves, Icon::NavSaves, t!("gui.side_bar.nav_saves"), saves_count(app)),
        (Section::Settings, Icon::NavSettings, t!("gui.side_bar.nav_settings"), String::new()),
    ];

    // The status card sits at the bottom edge and is given its space
    // first: its height depends on how often the text wraps, and a gap
    // inserted afterwards would push it out of the window as soon as three
    // rows wrap.
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        status_card(app, ui);
        ui.add_space(12.0);

        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for (section, icon, label, count) in items {
                if nav_item(app, ui, section, icon, &label, &count) {
                    actions.push(Action::ShowSection(section));
                }
            }
        });
    });
}

fn mods_count(app: &App) -> String {
    match &app.state {
        Some(state) => state.config.entries.len().to_string(),
        None => t!("gui.side_bar.mods_count_unknown"),
    }
}

/// When the savegame functions are locked, the counter shows a warning
/// sign instead of a number — drawn, not set as text (see `icons`).
fn saves_count(app: &App) -> String {
    if app.saves_blocked.is_some() {
        String::new()
    } else {
        app.backups.len().to_string()
    }
}

fn nav_item(
    app: &App,
    ui: &mut Ui,
    section: Section,
    icon: Icon,
    label: &str,
    count: &str,
) -> bool {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 36.0), Sense::click());

    let active = app.section == section;
    let background = if active {
        if response.hovered() { color::SELECTED_HOVER } else { color::SELECTED }
    } else if response.hovered() {
        color::HOVER
    } else {
        Color32::TRANSPARENT
    };

    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(8), background);
    if active {
        painter.rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(2.0, rect.height())),
            CornerRadius::same(1),
            color::ACCENT,
        );
    }

    let icon_color = if active { color::ACCENT } else { color::TEXT_FAINT };
    icon.paint(painter, Pos2::new(rect.left() + 10.0 + 7.0, rect.center().y), 12.0, icon_color);

    let (font, text_color) = if active {
        (medium(13.0), color::TEXT_STRONG)
    } else {
        (sans(13.0), color::TEXT_DIM)
    };
    painter.text(
        Pos2::new(rect.left() + 10.0 + 14.0 + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        text_color,
    );

    // Counter pill at the right edge.
    let blocked = section == Section::Saves && app.saves_blocked.is_some();
    if blocked || !count.is_empty() {
        let (pill_fg, pill_bg) = if active {
            (color::ACCENT, color::ACCENT_SOFT)
        } else {
            (color::TEXT_MUTED, color::CONTROL)
        };
        let content_width = if blocked {
            11.0
        } else {
            painter.layout_no_wrap(count.to_owned(), mono(10.5), pill_fg).size().x
        };
        let pill = Rect::from_center_size(
            Pos2::new(rect.right() - 10.0 - content_width / 2.0 - 7.0, rect.center().y),
            Vec2::new(content_width + 14.0, 16.0),
        );
        painter.rect_filled(pill, CornerRadius::same(8), pill_bg);
        if blocked {
            super::icons::warning(painter, pill.center(), 11.0, color::WARN);
        } else {
            painter.text(pill.center(), Align2::CENTER_CENTER, count, mono(10.5), pill_fg);
        }
    }

    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// The three status rows at the bottom: game, write access, EAC.
fn status_card(app: &App, ui: &mut Ui) {
    let rows = [
        game_row(app),
        write_row(app),
        eac_row(app),
    ];

    let width = ui.available_width();
    let painter_rows: Vec<_> = rows
        .iter()
        .map(|(_, _, text)| ui.painter().layout(
            (*text).to_owned(),
            sans(11.0),
            color::TEXT_DIM2,
            width - 20.0 - 8.0 - 12.0,
        ))
        .collect();
    let content_height: f32 =
        painter_rows.iter().map(|g| g.size().y).sum::<f32>() + 7.0 * (rows.len() - 1) as f32;
    let height = content_height + 20.0;

    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter();
    let radius = CornerRadius::same(10);
    painter.rect_filled(rect, radius, color::CARD);
    painter.rect_stroke(rect, radius, Stroke::new(1.0, color::BORDER_SOFT), StrokeKind::Inside);

    let mut y = rect.top() + 10.0;
    for ((icon, icon_color, _), galley) in rows.iter().zip(painter_rows) {
        icon.paint(painter, Pos2::new(rect.left() + 10.0 + 6.0, y + 7.0), 11.0, *icon_color);
        let text_height = galley.size().y;
        painter.galley(Pos2::new(rect.left() + 10.0 + 12.0 + 8.0, y), galley, color::TEXT_DIM2);
        y += text_height + 7.0;
    }
}

fn game_row(app: &App) -> (Icon, Color32, String) {
    if app.state.is_some() {
        (Icon::Check, color::OK, t!("gui.side_bar.game_detected"))
    } else {
        (Icon::Warning, color::WARN, t!("gui.side_bar.game_not_found"))
    }
}

fn write_row(app: &App) -> (Icon, Color32, String) {
    if app.state.is_none() {
        (Icon::Ring, color::TEXT_FAINT, t!("gui.side_bar.write_not_checked"))
    } else if app.writable {
        (Icon::Check, color::OK, t!("gui.side_bar.write_ok"))
    } else {
        (Icon::Warning, color::WARN, t!("gui.side_bar.write_denied"))
    }
}

fn eac_row(app: &App) -> (Icon, Color32, String) {
    if app.no_eac_available {
        (Icon::Check, color::OK, t!("gui.side_bar.eac_available"))
    } else {
        (Icon::Ring, color::TEXT_FAINT, t!("gui.side_bar.eac_unavailable"))
    }
}

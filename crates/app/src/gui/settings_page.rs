//! The "Settings" section: detected directories and behaviour.

use super::theme::{color, medium, metric, mono, sans};
use super::widgets::{self, ButtonStyle, Icon};
use super::{Action, App};
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};
use sm2_core::t;
use sm2_core::update::Version;
use std::path::PathBuf;

pub fn show(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        directories(app, ui, actions);
        ui.add_space(metric::CONTENT_GAP);
        behaviour(app, ui, actions);
    });
}

/// One row of the directory table.
struct PathRow {
    label: String,
    value: String,
    /// Target path to open in the file manager, if there is one.
    open: Option<PathBuf>,
    /// Only the game directory can be changed.
    changeable: bool,
    /// Coloured as a warning when the value is not a real path.
    unresolved: bool,
}

fn directories(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let rows = path_rows(app);
    let height = 44.0 + rows.len() as f32 * 38.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    super::draw_card(ui, rect);
    card_title(ui, rect, &t!("gui.settings.directories_title"));

    for (index, row) in rows.iter().enumerate() {
        let line = Rect::from_min_size(
            Pos2::new(rect.left(), rect.top() + 44.0 + index as f32 * 38.0),
            Vec2::new(rect.width(), 38.0),
        );
        if index + 1 < rows.len() {
            ui.painter().hline(
                line.x_range(),
                line.bottom(),
                Stroke::new(1.0, color::BORDER_ROW),
            );
        }

        let inner = line.shrink2(Vec2::new(metric::CARD_PADDING, 0.0));
        ui.painter().text(
            Pos2::new(inner.left(), inner.center().y),
            Align2::LEFT_CENTER,
            &row.label,
            sans(12.0),
            color::TEXT_DIM2,
        );

        let mut buttons = ui.new_child(
            UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        buttons.spacing_mut().item_spacing.x = 10.0;
        if row.changeable
            && widgets::button(
                &mut buttons,
                &ButtonStyle::ghost().small(),
                None,
                &t!("gui.settings.change_button"),
                true,
            )
            .clicked()
        {
            actions.push(Action::PickGameDir);
        }
        if widgets::button(
            &mut buttons,
            &ButtonStyle::ghost().small(),
            None,
            &t!("gui.settings.open_button"),
            row.open.is_some(),
        )
        .clicked()
        {
            if let Some(path) = row.open.clone() {
                actions.push(Action::OpenFolder(path));
            }
        }

        let value_rect = Rect::from_min_max(
            Pos2::new(inner.left() + 150.0 + 10.0, inner.top()),
            Pos2::new(buttons.min_rect().left() - 10.0, inner.bottom()),
        );
        if value_rect.width() > 20.0 {
            let color = if row.unresolved { color::WARN } else { color::TEXT };
            widgets::column_text(ui, value_rect, &row.value, mono(11.0), color);
        }
    }
}

fn path_rows(app: &App) -> Vec<PathRow> {
    let mut rows = Vec::new();

    match &app.state {
        Some(state) => {
            rows.push(PathRow {
                label: t!("gui.settings.row_game"),
                value: state.paths.game_dir.display().to_string(),
                open: Some(state.paths.game_dir.clone()),
                changeable: true,
                unresolved: false,
            });
            rows.push(PathRow {
                label: t!("gui.settings.row_mods"),
                value: state.paths.mods_dir().display().to_string(),
                open: Some(state.paths.mods_dir()),
                changeable: false,
                unresolved: false,
            });
            rows.push(PathRow {
                label: t!("gui.settings.row_pak_config"),
                value: state.paths.pak_config_path().display().to_string(),
                open: Some(state.paths.mods_dir()),
                changeable: false,
                unresolved: false,
            });
            match state.paths.save_dir(state.settings.steam_user.as_deref()) {
                Ok(dir) => rows.push(PathRow {
                    label: t!("gui.settings.row_saves"),
                    value: dir.display().to_string(),
                    open: Some(dir),
                    changeable: false,
                    unresolved: false,
                }),
                Err(e) => rows.push(PathRow {
                    label: t!("gui.settings.row_saves"),
                    value: e.to_string(),
                    open: None,
                    changeable: false,
                    unresolved: true,
                }),
            }
        }
        None => {
            rows.push(PathRow {
                label: t!("gui.settings.row_game"),
                value: t!("gui.settings.game_not_detected"),
                open: None,
                changeable: true,
                unresolved: true,
            });
            for label in [
                t!("gui.settings.row_mods"),
                t!("gui.settings.row_pak_config"),
                t!("gui.settings.row_saves"),
            ] {
                rows.push(PathRow {
                    label,
                    value: t!("gui.settings.no_value"),
                    open: None,
                    changeable: false,
                    unresolved: true,
                });
            }
        }
    }

    if let Some(dir) = app.backups_dir() {
        rows.push(PathRow {
            label: t!("gui.settings.row_backups"),
            value: dir.display().to_string(),
            open: Some(dir),
            changeable: false,
            unresolved: false,
        });
    }
    if let Some(dir) = app.profiles_dir() {
        rows.push(PathRow {
            label: t!("gui.settings.row_profiles"),
            value: dir.display().to_string(),
            open: Some(dir),
            changeable: false,
            unresolved: false,
        });
    }
    rows
}

fn behaviour(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let height = 44.0 + 56.0 + UPDATE_BLOCK_HEIGHT + 56.0 + 56.0 + 62.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    super::draw_card(ui, rect);
    card_title(ui, rect, &t!("gui.settings.behaviour_title"));

    // Automatic backup before launching.
    let auto = Rect::from_min_size(
        Pos2::new(rect.left(), rect.top() + 44.0),
        Vec2::new(rect.width(), 56.0),
    );
    ui.painter().hline(auto.x_range(), auto.bottom(), Stroke::new(1.0, color::BORDER_ROW));
    let auto_response = ui.interact(auto, egui::Id::new("auto_backup_row"), Sense::click());
    if auto_response.hovered() {
        ui.painter().rect_filled(auto, CornerRadius::ZERO, color::HOVER);
    }
    let inner = auto.shrink2(Vec2::new(metric::CARD_PADDING, 0.0));
    let mut toggle_ui = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(
        Pos2::new(inner.left(), inner.top() + 13.0),
        Vec2::new(30.0, 17.0),
    )));
    let toggled = widgets::toggle(&mut toggle_ui, app.settings().auto_backup, true).clicked();
    ui.painter().text(
        Pos2::new(inner.left() + 42.0, inner.top() + 20.0),
        Align2::LEFT_CENTER,
        t!("gui.settings.auto_backup_title"),
        sans(12.5),
        color::TEXT_STRONG,
    );
    ui.painter().text(
        Pos2::new(inner.left() + 42.0, inner.top() + 38.0),
        Align2::LEFT_CENTER,
        t!("gui.settings.auto_backup_body"),
        sans(11.0),
        color::TEXT_MUTED,
    );
    if toggled || auto_response.clicked() {
        actions.push(Action::ToggleAutoBackup);
    }

    let update_bottom = update_block(app, ui, rect, auto.bottom(), actions);

    // Steam user profile.
    let user = Rect::from_min_size(
        Pos2::new(rect.left(), update_bottom),
        Vec2::new(rect.width(), 56.0),
    );
    ui.painter().hline(user.x_range(), user.bottom(), Stroke::new(1.0, color::BORDER_ROW));
    let inner = user.shrink2(Vec2::new(metric::CARD_PADDING, 0.0));
    ui.painter().text(
        Pos2::new(inner.left(), inner.center().y),
        Align2::LEFT_CENTER,
        t!("gui.settings.steam_user_title"),
        sans(12.5),
        color::TEXT_STRONG,
    );

    let (chosen, foreground) = match app.settings().steam_user.clone() {
        Some(user) => (user, color::TEXT_STRONG),
        None if app.steam_users.len() > 1 => (
            t!("gui.settings.steam_user_not_chosen", count = app.steam_users.len()),
            color::WARN,
        ),
        None => (t!("gui.settings.steam_user_auto"), color::TEXT_STRONG),
    };
    let field = Rect::from_min_size(
        Pos2::new(inner.left() + 170.0, inner.center().y - 15.0),
        Vec2::new(250.0, metric::BUTTON_HEIGHT),
    );
    let field_response = ui.interact(field, egui::Id::new("steam_user_field"), Sense::click());
    let painter = ui.painter();
    let radius = CornerRadius::same(7);
    painter.rect_filled(field, radius, color::CONTROL);
    painter.rect_stroke(
        field,
        radius,
        Stroke::new(1.0, if field_response.hovered() { color::ACCENT } else { color::BORDER }),
        StrokeKind::Inside,
    );
    painter.text(
        Pos2::new(field.left() + 11.0, field.center().y),
        Align2::LEFT_CENTER,
        chosen,
        mono(11.5),
        foreground,
    );
    Icon::ChevronDown.paint(
        painter,
        Pos2::new(field.right() - 11.0 - 4.5, field.center().y),
        9.0,
        color::TEXT_MUTED,
    );
    painter.text(
        Pos2::new(field.right() + 10.0, field.center().y),
        Align2::LEFT_CENTER,
        t!("gui.settings.steam_user_hint"),
        sans(11.0),
        color::TEXT_MUTED,
    );
    if field_response.clicked() {
        actions.push(Action::OpenSteamUserDialog);
    }

    // Language of the interface.
    let language = Rect::from_min_size(
        Pos2::new(rect.left(), user.bottom()),
        Vec2::new(rect.width(), 56.0),
    );
    ui.painter().hline(language.x_range(), language.bottom(), Stroke::new(1.0, color::BORDER_ROW));
    let inner = language.shrink2(Vec2::new(metric::CARD_PADDING, 0.0));
    ui.painter().text(
        Pos2::new(inner.left(), inner.center().y),
        Align2::LEFT_CENTER,
        t!("gui.settings.language"),
        sans(12.5),
        color::TEXT_STRONG,
    );
    let mut language_button = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    if widgets::button(
        &mut language_button,
        &ButtonStyle::ghost().small(),
        None,
        app.settings().language().native_name(),
        true,
    )
    .clicked()
    {
        actions.push(Action::OpenLanguageDialog);
    }

    // Availability of the start without EAC.
    let eac = Rect::from_min_size(
        Pos2::new(rect.left(), language.bottom()),
        Vec2::new(rect.width(), 62.0),
    );
    let inner = eac.shrink2(Vec2::new(metric::CARD_PADDING, 0.0));
    let (icon, icon_color, title, body): (Icon, Color32, String, String) = if app.no_eac_available {
        (
            Icon::Check,
            color::OK,
            t!("gui.settings.eac_available_title"),
            t!("gui.settings.eac_available_body"),
        )
    } else {
        (
            Icon::Ring,
            color::TEXT_FAINT,
            t!("gui.settings.eac_unavailable_title"),
            t!("gui.settings.eac_unavailable_body"),
        )
    };
    icon.paint(ui.painter(), Pos2::new(inner.left() + 6.0, inner.top() + 20.0), 11.0, icon_color);
    ui.painter().text(
        Pos2::new(inner.left() + 24.0, inner.top() + 20.0),
        Align2::LEFT_CENTER,
        title,
        sans(12.5),
        color::TEXT_STRONG,
    );
    let body_galley =
        ui.painter().layout(body, sans(11.0), color::TEXT_MUTED, 660.0_f32.min(inner.width() - 24.0));
    ui.painter().galley(
        Pos2::new(inner.left() + 24.0, inner.top() + 30.0),
        body_galley,
        color::TEXT_MUTED,
    );
}

/// How tall `update_block` paints: heading, the two version rows, the
/// buttons, the toggle. The card is allocated in one piece before
/// anything is drawn into it, so the sum has to sit next to the rows it
/// adds up — the same arrangement the rest of `behaviour` uses.
const UPDATE_BLOCK_HEIGHT: f32 = 38.0 + 38.0 + 38.0 + 46.0 + 56.0;

/// The update rows, between the backup toggle and the Steam user.
/// Returns the edge it drew down to, which is where the next row starts.
fn update_block(app: &App, ui: &mut Ui, card: Rect, top: f32, actions: &mut Vec<Action>) -> f32 {
    let heading = Rect::from_min_size(Pos2::new(card.left(), top), Vec2::new(card.width(), 38.0));
    ui.painter().hline(heading.x_range(), heading.bottom(), Stroke::new(1.0, color::BORDER_ROW));
    ui.painter().text(
        Pos2::new(heading.left() + metric::CARD_PADDING, heading.center().y),
        Align2::LEFT_CENTER,
        t!("gui.settings.update_title"),
        medium(12.5),
        color::TEXT_STRONG,
    );

    // What the last check found, whatever made it run: pressing the
    // button is a check too, so this row says something even while the
    // automatic one is switched off. Nothing known at all is a dash
    // rather than "up to date" — the launcher has not looked.
    let (available, available_color) = if app.update.is_busy() {
        (t!("gui.settings.update_checking"), color::TEXT_MUTED)
    } else {
        match app.update.known {
            Some(found) => match found.newer() {
                Some(latest) => (latest.to_string(), color::ACCENT),
                None => (t!("gui.settings.update_up_to_date"), color::TEXT),
            },
            None => (t!("gui.settings.no_value"), color::TEXT_MUTED),
        }
    };
    let after_installed = version_row(
        ui,
        card,
        heading.bottom(),
        &t!("gui.settings.update_row_installed"),
        &Version::running().to_string(),
        color::TEXT,
    );
    let after_available = version_row(
        ui,
        card,
        after_installed,
        &t!("gui.settings.update_row_available"),
        &available,
        available_color,
    );

    let buttons = Rect::from_min_size(
        Pos2::new(card.left(), after_available),
        Vec2::new(card.width(), 46.0),
    );
    ui.painter().hline(buttons.x_range(), buttons.bottom(), Stroke::new(1.0, color::BORDER_ROW));
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(buttons.shrink2(Vec2::new(metric::CARD_PADDING, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            // One long job at a time — `App::task` holds exactly one,
            // and the status bar has room for exactly one.
            let installable = app.update.has_news() && app.task.is_none();
            if widgets::button(
                ui,
                &ButtonStyle::primary().small(),
                None,
                &t!("gui.settings.update_button"),
                installable,
            )
            .clicked()
            {
                actions.push(Action::InstallUpdate);
            }
            if widgets::button(
                ui,
                &ButtonStyle::ghost().small(),
                None,
                &t!("gui.settings.update_check_button"),
                !app.update.is_busy(),
            )
            .clicked()
            {
                actions.push(Action::CheckForUpdates);
            }
        },
    );

    let automatic = Rect::from_min_size(
        Pos2::new(card.left(), buttons.bottom()),
        Vec2::new(card.width(), 56.0),
    );
    ui.painter().hline(
        automatic.x_range(),
        automatic.bottom(),
        Stroke::new(1.0, color::BORDER_ROW),
    );
    let row_response = ui.interact(automatic, egui::Id::new("update_check_row"), Sense::click());
    if row_response.hovered() {
        ui.painter().rect_filled(automatic, CornerRadius::ZERO, color::HOVER);
    }
    let inner = automatic.shrink2(Vec2::new(metric::CARD_PADDING, 0.0));
    let mut toggle_ui = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(
        Pos2::new(inner.left(), inner.top() + 13.0),
        Vec2::new(30.0, 17.0),
    )));
    // An unanswered question — the dialog was dismissed instead of
    // answered — shows as off, which is what it is: nothing is asked of
    // github.com until someone says so, here or there.
    let on = app.settings().update_check.unwrap_or(false);
    let toggled = widgets::toggle(&mut toggle_ui, on, true).clicked();
    ui.painter().text(
        Pos2::new(inner.left() + 42.0, inner.top() + 20.0),
        Align2::LEFT_CENTER,
        t!("gui.settings.update_auto_title"),
        sans(12.5),
        color::TEXT_STRONG,
    );
    ui.painter().text(
        Pos2::new(inner.left() + 42.0, inner.top() + 38.0),
        Align2::LEFT_CENTER,
        t!("gui.settings.update_auto_body"),
        sans(11.0),
        color::TEXT_MUTED,
    );
    if toggled || row_response.clicked() {
        actions.push(Action::ToggleUpdateCheck);
    }

    automatic.bottom()
}

/// One "label — value" line of the update block, in the shape the
/// directory table above uses. Returns its bottom edge.
fn version_row(
    ui: &Ui,
    card: Rect,
    top: f32,
    label: &str,
    value: &str,
    value_color: Color32,
) -> f32 {
    let row = Rect::from_min_size(Pos2::new(card.left(), top), Vec2::new(card.width(), 38.0));
    ui.painter().hline(row.x_range(), row.bottom(), Stroke::new(1.0, color::BORDER_ROW));
    let inner = row.shrink2(Vec2::new(metric::CARD_PADDING, 0.0));
    ui.painter().text(
        Pos2::new(inner.left(), inner.center().y),
        Align2::LEFT_CENTER,
        label,
        sans(12.0),
        color::TEXT_DIM2,
    );
    ui.painter().text(
        Pos2::new(inner.left() + 150.0 + 10.0, inner.center().y),
        Align2::LEFT_CENTER,
        value,
        mono(11.0),
        value_color,
    );
    row.bottom()
}

fn card_title(ui: &Ui, card: Rect, title: &str) {
    let head = Rect::from_min_size(card.min, Vec2::new(card.width(), 44.0));
    ui.painter().hline(head.x_range(), head.bottom(), Stroke::new(1.0, color::BORDER_SOFT));
    ui.painter().text(
        Pos2::new(head.left() + metric::CARD_PADDING, head.center().y),
        Align2::LEFT_CENTER,
        title,
        medium(13.5),
        color::TEXT_STRONG,
    );
}

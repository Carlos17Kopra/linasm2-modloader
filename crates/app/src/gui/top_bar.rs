//! The top bar: heading with path on the left, launch choice on the right.

use super::theme::{color, medium, metric, mono, sans};
use super::widgets::{self, ButtonStyle, Icon};
use super::{Action, App, LaunchChoice};
use egui::{Align, CornerRadius, Layout, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use sm2_core::t;

pub fn show(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;

        if widgets::button(
            ui,
            &ButtonStyle::primary().padding_x(18.0),
            Some(Icon::Play),
            &t!("gui.top_bar.start"),
            true,
        )
        .clicked()
        {
            actions.push(Action::Launch);
        }

        let combo = launch_combo(app, ui);
        if combo.clicked() {
            actions.push(Action::ToggleLaunchMenu);
        }
        if app.launch_menu_open {
            launch_menu(app, ui, combo.rect, actions);
        }

        ui.add_space(6.0);
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| headline(app, ui));
    });
}

/// Heading and path, two rows stacked on top of each other.
fn headline(app: &App, ui: &mut Ui) {
    let available = ui.available_rect_before_wrap();

    let (title, path) = match &app.state {
        Some(state) => {
            let total = state.config.entries.len();
            let active = state.config.entries.iter().filter(|e| !e.disabled).count();
            (
                t!("gui.top_bar.mods_active", total = total, active = active),
                state.paths.mods_dir().display().to_string(),
            )
        }
        None => (t!("gui.top_bar.no_game_dir"), t!("gui.top_bar.detection_failed")),
    };

    let title_galley =
        widgets::truncated(ui, &title, medium(15.0), color::TEXT_STRONG, available.width());
    let path_galley = widgets::truncated(ui, &path, mono(11.0), color::TEXT_MUTED, available.width());
    let painter = ui.painter();

    let block_height = title_galley.size().y + 2.0 + path_galley.size().y;
    let top = available.center().y - block_height / 2.0;
    painter.galley(Pos2::new(available.left(), top), title_galley, color::TEXT_STRONG);

    let path_top = top + block_height - path_galley.size().y;
    painter.galley(Pos2::new(available.left(), path_top), path_galley, color::TEXT_MUTED);
}

/// The drop-down field showing the chosen way to launch.
fn launch_combo(app: &App, ui: &mut Ui) -> egui::Response {
    let label = app.launch_choice.label();
    let galley = ui.painter().layout_no_wrap(label.to_owned(), sans(12.5), color::TEXT);
    let width = (galley.size().x + 12.0 + 10.0 + 9.0 + 12.0).max(228.0);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, metric::BUTTON_HEIGHT_LARGE), Sense::click());

    let hovered = response.hovered();
    let border = if hovered { color::BORDER_HOVER } else { color::BORDER };
    let foreground = if hovered { color::TEXT_STRONG } else { color::TEXT };

    let painter = ui.painter();
    let radius = CornerRadius::same(8);
    painter.rect_filled(rect, radius, color::CONTROL);
    painter.rect_stroke(rect, radius, Stroke::new(1.0, border), StrokeKind::Inside);
    painter.galley(
        Pos2::new(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0),
        galley,
        foreground,
    );
    Icon::ChevronDown.paint(
        painter,
        Pos2::new(rect.right() - 12.0 - 4.5, rect.center().y),
        9.0,
        color::TEXT_MUTED,
    );

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The drop-down menu below the field. "Ohne EAC" is missing when
/// `umu-run` is not on the PATH — the design hides the row rather than let
/// the user fail only once they hit launch.
fn launch_menu(app: &App, ui: &Ui, anchor: Rect, actions: &mut Vec<Action>) {
    let mut choices = vec![LaunchChoice::Steam, LaunchChoice::Vanilla];
    if app.no_eac_available {
        choices.push(LaunchChoice::NoEac);
    }

    let width = 252.0_f32.max(anchor.width());
    let item_height = 27.0;
    let padding = 5.0;
    let height = padding * 2.0 + item_height * choices.len() as f32;
    let menu_rect = Rect::from_min_size(
        Pos2::new(anchor.left(), anchor.bottom() + 4.0),
        Vec2::new(width, height),
    );

    egui::Area::new(egui::Id::new("launch_menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(menu_rect.min)
        .show(ui.ctx(), |ui| {
            let painter = ui.painter();
            let radius = CornerRadius::same(10);
            painter.rect_filled(menu_rect, radius, color::MENU);
            painter.rect_stroke(
                menu_rect,
                radius,
                Stroke::new(1.0, color::BORDER),
                StrokeKind::Inside,
            );

            for (index, choice) in choices.iter().enumerate() {
                let item = Rect::from_min_size(
                    Pos2::new(
                        menu_rect.left() + padding,
                        menu_rect.top() + padding + index as f32 * item_height,
                    ),
                    Vec2::new(width - padding * 2.0, item_height),
                );
                let response = ui.interact(
                    item,
                    egui::Id::new(("launch_menu_item", index)),
                    Sense::click(),
                );

                let selected = app.launch_choice == *choice;
                let (background, foreground) = if selected {
                    (color::SELECTED, color::ACCENT)
                } else if response.hovered() {
                    (color::HOVER, color::TEXT_STRONG)
                } else {
                    (egui::Color32::TRANSPARENT, color::TEXT_DIM)
                };
                ui.painter().rect_filled(item, CornerRadius::same(6), background);
                ui.painter().text(
                    Pos2::new(item.left() + 10.0, item.center().y),
                    egui::Align2::LEFT_CENTER,
                    choice.label(),
                    sans(12.5),
                    foreground,
                );

                if response.clicked() {
                    actions.push(Action::SetLaunchChoice(*choice));
                }
            }

            // A click anywhere else closes the menu again.
            if ui.ctx().input(|i| i.pointer.any_click()) && !menu_rect.contains(
                ui.ctx().pointer_interact_pos().unwrap_or(Pos2::new(f32::MIN, f32::MIN)),
            ) && !anchor.contains(
                ui.ctx().pointer_interact_pos().unwrap_or(Pos2::new(f32::MIN, f32::MIN)),
            ) {
                actions.push(Action::ToggleLaunchMenu);
            }
        });
}

//! The status bar: the progress of a running job on the left, the last
//! message in the middle, a metric on the right.

use super::theme::{color, mono, sans};
use super::widgets::{self, ButtonStyle};
use super::{Action, App};
use egui::{Align, Align2, CornerRadius, Layout, Pos2, Rect, Sense, Ui, Vec2};
use sm2_core::t;

pub fn show(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 12.0;

        if let Some(task) = &app.task {
            let progress = task.progress();
            progress_bar(ui, progress.fraction);
            ui.label(egui::RichText::new(progress.label).font(sans(11.0)).color(color::TEXT));

            if task.is_cancellable() {
                let style = ButtonStyle::ghost()
                    .height(20.0)
                    .padding_x(9.0)
                    .font(sans(11.0))
                    .corner_radius(5);
                if widgets::button(ui, &style, None, &t!("gui.status_bar.cancel"), true).clicked() {
                    actions.push(Action::CancelTask);
                }
            }
        }

        // Reserve the metric on the right first, so that the message in
        // the middle gets the rest and is shortened when it has to be.
        let right = right_label(app);
        let right_width = ui.painter().layout_no_wrap(right.clone(), mono(11.0), color::TEXT_FAINT).size().x;

        let available = ui.available_rect_before_wrap();
        let message_rect = Rect::from_min_size(
            available.min,
            Vec2::new((available.width() - right_width - 12.0).max(0.0), available.height()),
        );
        let (_, _) = ui.allocate_exact_size(available.size(), Sense::hover());

        let status_color = if app.status_is_warning { color::WARN } else { color::TEXT_DIM2 };
        let galley = ui.painter().layout_no_wrap(app.status.clone(), sans(11.0), status_color);
        ui.painter().with_clip_rect(message_rect).galley(
            Pos2::new(message_rect.left(), message_rect.center().y - galley.size().y / 2.0),
            galley,
            status_color,
        );

        ui.painter().text(
            Pos2::new(available.right(), available.center().y),
            Align2::RIGHT_CENTER,
            right,
            mono(11.0),
            color::TEXT_FAINT,
        );
    });
}

fn right_label(app: &App) -> String {
    match &app.state {
        Some(state) => t!("gui.status_bar.entries", count = state.config.entries.len()),
        None => t!("gui.status_bar.no_game_dir"),
    }
}

/// The progress bar: 180 × 6 px, fully rounded.
fn progress_bar(ui: &mut Ui, fraction: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(180.0, 6.0), Sense::hover());
    let radius = CornerRadius::same(3);
    ui.painter().rect_filled(rect, radius, color::CONTROL);

    let filled = rect.width() * fraction.clamp(0.0, 1.0);
    if filled > 0.5 {
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(filled, rect.height())),
            radius,
            color::ACCENT,
        );
    }
}

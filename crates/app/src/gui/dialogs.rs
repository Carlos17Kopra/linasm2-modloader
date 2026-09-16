//! The four modal windows from the design.
//!
//! `egui::Modal` brings the dimming backdrop and the locking of the
//! controls underneath; the header, body and footer are drawn by this
//! module itself, following the design.

use super::format::human_time;
use super::theme::{color, medium, mono, sans};
use super::widgets::{self, ButtonStyle};
use super::{Action, App, Dialog};
use egui::{
    Align2, Color32, CornerRadius, Frame, Margin, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2,
};
use sm2_core::i18n::Language;
use sm2_core::t;

pub fn show(app: &App, ctx: &egui::Context, actions: &mut Vec<Action>) {
    let Some(dialog) = app.dialog.clone() else { return };

    let width = match &dialog {
        Dialog::Restore { .. } => 580.0,
        Dialog::Vanilla => 540.0,
        Dialog::SteamUser { .. } => 490.0,
        Dialog::Language { .. } => 490.0,
        Dialog::DeleteProfile { .. } => 450.0,
        Dialog::RenameBackup { .. } => 460.0,
        Dialog::DeleteBackup { .. } => 470.0,
    };
    let border = match &dialog {
        Dialog::Restore { .. } => color::WARN_BORDER,
        Dialog::DeleteBackup { .. } => color::DANGER_BORDER,
        _ => color::BORDER,
    };

    let response = egui::Modal::new(egui::Id::new("dialog"))
        .backdrop_color(color::OVERLAY)
        .frame(
            Frame::new()
                .fill(color::CARD)
                .stroke(Stroke::new(1.0, border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(Margin::same(0)),
        )
        .show(ctx, |ui| {
            ui.set_width(width);
            match &dialog {
                Dialog::Restore { index, force } => restore(app, ui, *index, *force, actions),
                Dialog::Vanilla => vanilla(app, ui, actions),
                Dialog::SteamUser { picked } => steam_user(app, ui, picked.as_deref(), actions),
                Dialog::Language { picked } => language(ui, *picked, actions),
                Dialog::DeleteProfile { name } => delete_profile(ui, name, actions),
                Dialog::RenameBackup { index, label } => {
                    rename_backup(app, ui, *index, label, actions)
                }
                Dialog::DeleteBackup { index } => delete_backup(app, ui, *index, actions),
            }
        });

    // A click outside or Escape closes it — like the cross in the header.
    if response.should_close() {
        actions.push(Action::CloseDialog);
    }
}

/// Header with an optional warning sign, the title and the close cross.
fn header(ui: &mut Ui, title: &str, warning: bool, actions: &mut Vec<Action>) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 46.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        CornerRadius { nw: 12, ne: 12, sw: 0, se: 0 },
        color::DIALOG_HEAD,
    );
    painter.hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, color::BORDER_SOFT));

    let mut x = rect.left() + 14.0;
    if warning {
        super::icons::warning(painter, Pos2::new(x + 6.0, rect.center().y), 12.0, color::WARN);
        x += 22.0;
    }
    painter.text(
        Pos2::new(x, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        medium(13.5),
        color::TEXT_STRONG,
    );

    let close =
        Rect::from_center_size(Pos2::new(rect.right() - 14.0 - 5.5, rect.center().y), Vec2::splat(20.0));
    let response = ui.interact(close, egui::Id::new("dialog_close"), Sense::click());
    super::icons::cross(
        ui.painter(),
        close.center(),
        11.0,
        if response.hovered() { color::TEXT_STRONG } else { color::TEXT_FAINT },
    );
    if response.clicked() {
        actions.push(Action::CloseDialog);
    }
}

/// The content area below the header: 14 px all round, 12 px between the
/// blocks.
fn body<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 12.0;
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn paragraph(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).font(sans(12.5)).color(color::TEXT));
}

/// The "restore despite Steam running?" dialog (`gui.dialog.restore_title`).
fn restore(app: &App, ui: &mut Ui, index: usize, force: bool, actions: &mut Vec<Action>) {
    header(ui, &t!("gui.dialog.restore_title"), true, actions);
    body(ui, |ui| {
        paragraph(ui, &t!("gui.dialog.restore_body"));

        let entry = app.backups.get(index);
        widgets::inset().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            let when = entry.map_or_else(
                || t!("gui.dialog.no_value"),
                |e| {
                    t!(
                        "gui.dialog.restore_when",
                        time = human_time(&e.created_at),
                        label = e.label.clone().unwrap_or_else(|| t!("gui.dialog.no_value"))
                    )
                },
            );
            field(ui, &t!("gui.dialog.restore_field_backup"), &when, mono(11.5), color::TEXT_STRONG);

            let target = app
                .state
                .as_ref()
                .and_then(|s| s.paths.save_dir(s.settings.steam_user.as_deref()).ok())
                .map_or_else(|| t!("gui.dialog.no_value"), |p| p.display().to_string());
            field(ui, &t!("gui.dialog.restore_field_target"), &target, mono(11.5), color::TEXT);
            field(
                ui,
                &t!("gui.dialog.restore_field_backed_up"),
                &t!("gui.dialog.restore_backed_up_value"),
                sans(11.5),
                color::OK,
            );
        });

        // The confirmation toggle — red instead of the accent colour,
        // because it releases a risk rather than setting an ordinary
        // option.
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 11.0;
            let clicked = widgets::toggle_colored(
                ui,
                force,
                true,
                color::DANGER_BORDER,
                color::DANGER,
                color::DANGER_KNOB,
            )
            .clicked();
            ui.label(
                egui::RichText::new(t!("gui.dialog.restore_ack"))
                    .font(sans(12.0))
                    .color(color::TEXT),
            );
            if clicked {
                actions.push(Action::SetRestoreForce(!force));
            }
        });

        footer(ui, |ui| {
            let mut confirm = ButtonStyle::danger();
            if !force {
                confirm.background = color::CONTROL_DISABLED;
                confirm.background_hovered = color::CONTROL_DISABLED;
                confirm.border = Some(color::BORDER_DISABLED);
                confirm.border_hovered = Some(color::BORDER_DISABLED);
                confirm.foreground = color::TEXT_FAINT;
                confirm.foreground_hovered = color::TEXT_FAINT;
            }
            if widgets::button(ui, &confirm, None, &t!("gui.dialog.restore_confirm"), force)
                .clicked()
            {
                actions.push(Action::ConfirmRestore);
            }
            if widgets::button(ui, &ButtonStyle::neutral(), None, &t!("gui.dialog.cancel"), true)
                .clicked()
            {
                actions.push(Action::CloseDialog);
            }
        });
    });
}

/// The "launch without mods?" dialog (`gui.dialog.vanilla_title`).
fn vanilla(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    header(ui, &t!("gui.dialog.vanilla_title"), false, actions);
    body(ui, |ui| {
        let anything_active = app.entries().iter().any(|e| !e.disabled);
        if anything_active {
            paragraph(ui, &t!("gui.dialog.vanilla_body_active"));
            widgets::inset().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    egui::RichText::new(format!(
                        "{} {}",
                        crate::vanilla::VANILLA_SNAPSHOT_PREFIX,
                        crate::vanilla::timestamp_for_snapshot_name()
                    ))
                    .font(mono(11.5))
                    .color(color::TEXT_STRONG),
                );
            });
            ui.label(
                egui::RichText::new(t!("gui.dialog.vanilla_body_active_note"))
                    .font(sans(11.5))
                    .color(color::TEXT_DIM2),
            );
        } else {
            paragraph(ui, &t!("gui.dialog.vanilla_body_inactive"));
        }

        footer(ui, |ui| {
            let label = if anything_active {
                t!("gui.dialog.vanilla_confirm_with_backup")
            } else {
                t!("gui.dialog.vanilla_confirm_plain")
            };
            if widgets::button(ui, &ButtonStyle::primary(), None, &label, true).clicked() {
                actions.push(Action::ConfirmVanilla);
            }
            if widgets::button(ui, &ButtonStyle::neutral(), None, &t!("gui.dialog.cancel"), true)
                .clicked()
            {
                actions.push(Action::CloseDialog);
            }
        });
    });
}

/// The "choose Steam user profile" dialog (`gui.dialog.steam_user_title`).
fn steam_user(app: &App, ui: &mut Ui, picked: Option<&str>, actions: &mut Vec<Action>) {
    header(ui, &t!("gui.dialog.steam_user_title"), false, actions);
    body(ui, |ui| {
        paragraph(ui, &t!("gui.dialog.steam_user_body"));

        if app.steam_users.is_empty() {
            ui.label(
                egui::RichText::new(t!("gui.dialog.steam_user_none_found"))
                    .font(sans(12.0))
                    .color(color::WARN),
            );
        }

        ui.spacing_mut().item_spacing.y = 6.0;
        for user in &app.steam_users {
            let selected = picked == Some(user.as_str());
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 38.0), Sense::click());

            let painter = ui.painter();
            let radius = CornerRadius::same(9);
            painter.rect_filled(rect, radius, if selected { color::SELECTED } else { color::INPUT });
            let border = if selected || response.hovered() { color::ACCENT } else { color::BORDER };
            painter.rect_stroke(rect, radius, Stroke::new(1.0, border), StrokeKind::Inside);

            super::icons::radio(
                painter,
                Pos2::new(rect.left() + 12.0 + 7.0, rect.center().y),
                if selected { color::ACCENT } else { color::BORDER_HOVER },
                if selected { color::ACCENT } else { Color32::TRANSPARENT },
            );
            painter.text(
                Pos2::new(rect.left() + 12.0 + 14.0 + 11.0, rect.center().y),
                Align2::LEFT_CENTER,
                user,
                mono(12.0),
                color::TEXT_STRONG,
            );

            if response.clicked() {
                actions.push(Action::PickSteamUser(user.clone()));
            }
        }

        footer(ui, |ui| {
            if widgets::button(ui, &ButtonStyle::primary(), None, &t!("gui.dialog.apply"), picked.is_some())
                .clicked()
            {
                actions.push(Action::ConfirmSteamUser);
            }
            if widgets::button(ui, &ButtonStyle::neutral(), None, &t!("gui.dialog.cancel"), true)
                .clicked()
            {
                actions.push(Action::CloseDialog);
            }
        });
    });
}

/// The "choose language" dialog (`gui.dialog.language_title`).
fn language(ui: &mut Ui, picked: Option<Language>, actions: &mut Vec<Action>) {
    header(ui, &t!("gui.dialog.language_title"), false, actions);
    body(ui, |ui| {
        paragraph(ui, &t!("gui.dialog.language_body"));

        ui.spacing_mut().item_spacing.y = 6.0;
        for candidate in Language::ALL {
            let selected = picked == Some(candidate);
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 38.0), Sense::click());

            let painter = ui.painter();
            let radius = CornerRadius::same(9);
            painter.rect_filled(rect, radius, if selected { color::SELECTED } else { color::INPUT });
            let border = if selected || response.hovered() { color::ACCENT } else { color::BORDER };
            painter.rect_stroke(rect, radius, Stroke::new(1.0, border), StrokeKind::Inside);

            super::icons::radio(
                painter,
                Pos2::new(rect.left() + 12.0 + 7.0, rect.center().y),
                if selected { color::ACCENT } else { color::BORDER_HOVER },
                if selected { color::ACCENT } else { Color32::TRANSPARENT },
            );
            painter.text(
                Pos2::new(rect.left() + 12.0 + 14.0 + 11.0, rect.center().y),
                Align2::LEFT_CENTER,
                candidate.native_name(),
                sans(12.5),
                color::TEXT_STRONG,
            );

            if response.clicked() {
                actions.push(Action::PickLanguage(candidate));
            }
        }

        footer(ui, |ui| {
            if widgets::button(ui, &ButtonStyle::primary(), None, &t!("gui.dialog.apply"), picked.is_some())
                .clicked()
            {
                actions.push(Action::ConfirmLanguage);
            }
            if widgets::button(ui, &ButtonStyle::neutral(), None, &t!("gui.dialog.cancel"), true)
                .clicked()
            {
                actions.push(Action::CloseDialog);
            }
        });
    });
}

/// The "delete profile?" dialog (`gui.dialog.delete_profile_title`).
fn delete_profile(ui: &mut Ui, name: &str, actions: &mut Vec<Action>) {
    header(ui, &t!("gui.dialog.delete_profile_title"), false, actions);
    body(ui, |ui| {
        paragraph(ui, &t!("gui.dialog.delete_profile_body", name = name));
        footer(ui, |ui| {
            if widgets::button(ui, &ButtonStyle::danger(), None, &t!("gui.dialog.delete"), true)
                .clicked()
            {
                actions.push(Action::ConfirmDeleteProfile);
            }
            if widgets::button(ui, &ButtonStyle::neutral(), None, &t!("gui.dialog.cancel"), true)
                .clicked()
            {
                actions.push(Action::CloseDialog);
            }
        });
    });
}

/// The "rename backup" dialog (`gui.dialog.rename_backup_title`).
fn rename_backup(app: &App, ui: &mut Ui, index: usize, label: &str, actions: &mut Vec<Action>) {
    header(ui, &t!("gui.dialog.rename_backup_title"), false, actions);
    body(ui, |ui| {
        if let Some(entry) = app.backups.get(index) {
            field(
                ui,
                &t!("gui.dialog.field_created"),
                &human_time(&entry.created_at),
                mono(12.0),
                color::TEXT,
            );
        }
        paragraph(ui, &t!("gui.dialog.rename_backup_body"));

        let mut value = label.to_owned();
        let placeholder = t!("gui.dialog.rename_backup_placeholder");
        let response = widgets::text_field(ui, &mut value, &placeholder, ui.available_width(), None);
        if response.changed() {
            actions.push(Action::SetRenameLabel(value));
        }
        // The dialog opens with the caret already in the field — it has
        // exactly one input, and that is what everyone came here to change.
        if ui.memory(|m| m.focused().is_none()) {
            response.request_focus();
        }
        // The order carries weight here: `SetRenameLabel` comes before
        // `ConfirmRenameBackup` in the same list and is applied first, so
        // the confirmation never works on a stale label.
        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            actions.push(Action::ConfirmRenameBackup);
        }

        footer(ui, |ui| {
            if widgets::button(ui, &ButtonStyle::primary(), None, &t!("gui.dialog.apply"), true)
                .clicked()
            {
                actions.push(Action::ConfirmRenameBackup);
            }
            if widgets::button(ui, &ButtonStyle::neutral(), None, &t!("gui.dialog.cancel"), true)
                .clicked()
            {
                actions.push(Action::CloseDialog);
            }
        });
    });
}

/// The "delete backup?" dialog (`gui.dialog.delete_backup_title`).
fn delete_backup(app: &App, ui: &mut Ui, index: usize, actions: &mut Vec<Action>) {
    header(ui, &t!("gui.dialog.delete_backup_title"), true, actions);
    body(ui, |ui| {
        if let Some(entry) = app.backups.get(index) {
            field(
                ui,
                &t!("gui.dialog.field_created"),
                &human_time(&entry.created_at),
                mono(12.0),
                color::TEXT,
            );
            let label = entry.label.clone().unwrap_or_else(|| t!("gui.dialog.no_value"));
            field(ui, &t!("gui.dialog.field_label"), &label, sans(12.5), color::TEXT);
        }
        paragraph(ui, &t!("gui.dialog.delete_backup_body"));
        footer(ui, |ui| {
            if widgets::button(ui, &ButtonStyle::danger(), None, &t!("gui.dialog.delete"), true)
                .clicked()
            {
                actions.push(Action::ConfirmDeleteBackup);
            }
            if widgets::button(ui, &ButtonStyle::neutral(), None, &t!("gui.dialog.cancel"), true)
                .clicked()
            {
                actions.push(Action::CloseDialog);
            }
        });
    });
}

/// A dialog's footer: buttons aligned to the right, the confirming one
/// furthest right.
fn footer(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        add(ui);
    });
}

/// One "label — value" row inside an inset box.
fn field(ui: &mut Ui, label: &str, value: &str, font: egui::FontId, color: egui::Color32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(150.0, 16.0), Sense::hover());
        ui.painter().text(
            Pos2::new(rect.left(), rect.center().y),
            Align2::LEFT_CENTER,
            label,
            sans(11.5),
            super::theme::color::TEXT_FAINT,
        );
        let value_rect = ui.available_rect_before_wrap();
        widgets::column_text(
            ui,
            Rect::from_min_size(value_rect.min, Vec2::new(value_rect.width(), 16.0)),
            value,
            font,
            color,
        );
        ui.allocate_exact_size(Vec2::new(value_rect.width(), 16.0), Sense::hover());
    });
}

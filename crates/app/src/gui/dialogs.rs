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

pub fn show(app: &App, ctx: &egui::Context, actions: &mut Vec<Action>) {
    let Some(dialog) = app.dialog.clone() else { return };

    let width = match &dialog {
        Dialog::Restore { .. } => 580.0,
        Dialog::Vanilla => 540.0,
        Dialog::SteamUser { .. } => 490.0,
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

/// "Wiederherstellen, obwohl Steam läuft?"
fn restore(app: &App, ui: &mut Ui, index: usize, force: bool, actions: &mut Vec<Action>) {
    header(ui, "Wiederherstellen, obwohl Steam läuft?", true, actions);
    body(ui, |ui| {
        paragraph(
            ui,
            "Steam läuft. Die Cloud-Synchronisation kann den wiederhergestellten Stand \
             überschreiben, sobald das Spiel wieder startet. Bitte Steam beenden und erneut \
             versuchen – oder ausdrücklich bestätigen, falls der erkannte Prozess sicher nicht \
             mehr synchronisiert (hängender Client, Container).",
        );

        let entry = app.backups.get(index);
        widgets::inset().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            let when = entry.map_or_else(
                || String::from("—"),
                |e| {
                    format!(
                        "{} · {}",
                        human_time(&e.created_at),
                        e.label.clone().unwrap_or_else(|| String::from("—"))
                    )
                },
            );
            field(ui, "Wiederherstellen", &when, mono(11.5), color::TEXT_STRONG);

            let target = app
                .state
                .as_ref()
                .and_then(|s| s.paths.save_dir(s.settings.steam_user.as_deref()).ok())
                .map_or_else(|| String::from("—"), |p| p.display().to_string());
            field(ui, "Ziel", &target, mono(11.5), color::TEXT);
            field(
                ui,
                "Davor gesichert",
                "automatisch, als „vor Wiederherstellung“",
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
                egui::RichText::new(
                    "Mir ist klar, dass Steams Cloud-Synchronisation den zurückgespielten Stand \
                     überschreiben kann.",
                )
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
            if widgets::button(ui, &confirm, None, "Trotzdem wiederherstellen", force).clicked() {
                actions.push(Action::ConfirmRestore);
            }
            if widgets::button(ui, &ButtonStyle::neutral(), None, "Abbrechen", true).clicked() {
                actions.push(Action::CloseDialog);
            }
        });
    });
}

/// "Ohne Mods starten?"
fn vanilla(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    header(ui, "Ohne Mods starten?", false, actions);
    body(ui, |ui| {
        let anything_active = app.entries().iter().any(|e| !e.disabled);
        if anything_active {
            paragraph(
                ui,
                "Das deaktiviert alle Mods. Damit die Auswahl nicht verloren geht, sichert der \
                 Loader sie vorher als Profil:",
            );
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
                egui::RichText::new(
                    "Zurück geht es über Profile → Anwenden. Scheitert die Sicherung, wird nichts \
                     verändert und nichts gestartet.",
                )
                .font(sans(11.5))
                .color(color::TEXT_DIM2),
            );
        } else {
            paragraph(
                ui,
                "Es ist ohnehin kein Mod aktiv. Der Loader legt deshalb keine neue Sicherung an \
                 und startet das Spiel unverändert ohne Mods.",
            );
        }

        footer(ui, |ui| {
            let label = if anything_active {
                "Sichern und ohne Mods starten"
            } else {
                "Ohne Mods starten"
            };
            if widgets::button(ui, &ButtonStyle::primary(), None, label, true).clicked() {
                actions.push(Action::ConfirmVanilla);
            }
            if widgets::button(ui, &ButtonStyle::neutral(), None, "Abbrechen", true).clicked() {
                actions.push(Action::CloseDialog);
            }
        });
    });
}

/// "Steam-Nutzerprofil wählen"
fn steam_user(app: &App, ui: &mut Ui, picked: Option<&str>, actions: &mut Vec<Action>) {
    header(ui, "Steam-Nutzerprofil wählen", false, actions);
    body(ui, |ui| {
        paragraph(
            ui,
            "Im Proton-Prefix liegen mehrere Profile. Die Auswahl bestimmt, welche Spielstände \
             gesichert und wiederhergestellt werden.",
        );

        if app.steam_users.is_empty() {
            ui.label(
                egui::RichText::new("Es wurde kein Nutzerprofil gefunden.")
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
            if widgets::button(
                ui,
                &ButtonStyle::primary(),
                None,
                "Übernehmen",
                picked.is_some(),
            )
            .clicked()
            {
                actions.push(Action::ConfirmSteamUser);
            }
            if widgets::button(ui, &ButtonStyle::neutral(), None, "Abbrechen", true).clicked() {
                actions.push(Action::CloseDialog);
            }
        });
    });
}

/// "Profil löschen?"
fn delete_profile(ui: &mut Ui, name: &str, actions: &mut Vec<Action>) {
    header(ui, "Profil löschen?", false, actions);
    body(ui, |ui| {
        paragraph(
            ui,
            &format!(
                "„{name}“ wird gelöscht. Aktivierung und Reihenfolge der Mods bleiben, wie sie \
                 sind."
            ),
        );
        footer(ui, |ui| {
            if widgets::button(ui, &ButtonStyle::danger(), None, "Löschen", true).clicked() {
                actions.push(Action::ConfirmDeleteProfile);
            }
            if widgets::button(ui, &ButtonStyle::neutral(), None, "Abbrechen", true).clicked() {
                actions.push(Action::CloseDialog);
            }
        });
    });
}

/// "Backup umbenennen"
fn rename_backup(app: &App, ui: &mut Ui, index: usize, label: &str, actions: &mut Vec<Action>) {
    header(ui, "Backup umbenennen", false, actions);
    body(ui, |ui| {
        if let Some(entry) = app.backups.get(index) {
            field(ui, "Erstellt", &human_time(&entry.created_at), mono(12.0), color::TEXT);
        }
        paragraph(
            ui,
            "Das Etikett steht in der Liste und im Dateinamen des Backups. Ein leeres Feld \
             entfernt es wieder.",
        );

        let mut value = label.to_owned();
        let response = widgets::text_field(ui, &mut value, "Etikett", ui.available_width(), None);
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
            if widgets::button(ui, &ButtonStyle::primary(), None, "Übernehmen", true).clicked() {
                actions.push(Action::ConfirmRenameBackup);
            }
            if widgets::button(ui, &ButtonStyle::neutral(), None, "Abbrechen", true).clicked() {
                actions.push(Action::CloseDialog);
            }
        });
    });
}

/// "Backup löschen?"
fn delete_backup(app: &App, ui: &mut Ui, index: usize, actions: &mut Vec<Action>) {
    header(ui, "Backup löschen?", true, actions);
    body(ui, |ui| {
        if let Some(entry) = app.backups.get(index) {
            field(ui, "Erstellt", &human_time(&entry.created_at), mono(12.0), color::TEXT);
            field(
                ui,
                "Etikett",
                entry.label.as_deref().unwrap_or("—"),
                sans(12.5),
                color::TEXT,
            );
        }
        paragraph(
            ui,
            "Archiv und Manifest werden endgültig entfernt; zurückholen lässt sich das nicht. \
             Die Spielstände selbst bleiben unberührt – nur diese Sicherung ist danach weg.",
        );
        footer(ui, |ui| {
            if widgets::button(ui, &ButtonStyle::danger(), None, "Löschen", true).clicked() {
                actions.push(Action::ConfirmDeleteBackup);
            }
            if widgets::button(ui, &ButtonStyle::neutral(), None, "Abbrechen", true).clicked() {
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

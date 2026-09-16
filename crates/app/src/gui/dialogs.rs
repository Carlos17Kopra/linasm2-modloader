//! Die vier modalen Fenster des Entwurfs.
//!
//! `egui::Modal` bringt die abdunkelnde Fläche und das Sperren der
//! darunterliegenden Bedienelemente mit; Kopfzeile, Inhalt und Fußleiste
//! zeichnet dieses Modul nach dem Entwurf selbst.

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
    };
    let border = match &dialog {
        Dialog::Restore { .. } => color::WARN_BORDER,
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
            }
        });

    // Klick daneben oder Escape schließt – wie das Kreuz in der Kopfzeile.
    if response.should_close() {
        actions.push(Action::CloseDialog);
    }
}

/// Kopfzeile mit optionalem Warnzeichen, Titel und Schließkreuz.
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

/// Der Inhaltsbereich unter der Kopfzeile: 14 px ringsum, 12 px zwischen den
/// Blöcken.
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

/// „Wiederherstellen, obwohl Steam läuft?“
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

        // Bestätigungsschalter – rot statt Akzent, weil er ein Risiko
        // freigibt und keine gewöhnliche Einstellung ist.
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

/// „Ohne Mods starten?“
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

/// „Steam-Nutzerprofil wählen“
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

/// „Profil löschen?“
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

/// Die Fußleiste eines Dialogs: Schaltflächen rechtsbündig, die bestätigende
/// ganz rechts.
fn footer(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        add(ui);
    });
}

/// Eine Zeile „Beschriftung – Wert“ in einem eingelassenen Kasten.
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

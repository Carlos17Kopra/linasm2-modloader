//! Der Bereich „Savegames“: Backups anlegen, prüfen, zurückspielen.

use super::format::{human_size, human_time};
use super::theme::{color, medium, metric, mono, sans};
use super::widgets::{self, ButtonStyle, Column, Icon};
use super::{Action, App};
use egui::{Align2, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};

const BACKUP_COLUMNS: [Column; 4] = [
    Column::Fixed(158.0),
    Column::Flexible,
    Column::Fixed(90.0),
    Column::Fixed(210.0),
];

pub fn show(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    super::page_heading(
        ui,
        "Savegame-Backups",
        "Wiederherstellen überschreibt die echten Spielstände. Der Loader sichert den aktuellen \
         Stand davor immer automatisch – das ist nicht abschaltbar.",
        680.0,
    );

    if app.saves_blocked.is_some() {
        blocked_banner(app, ui, actions);
        ui.add_space(metric::CONTENT_GAP);
    }

    let outer = ui.available_rect_before_wrap();
    super::draw_card(ui, outer);

    let toolbar =
        Rect::from_min_size(outer.min, Vec2::new(outer.width(), metric::CARD_TOOLBAR_HEIGHT));
    ui.scope_builder(
        UiBuilder::new().max_rect(toolbar.shrink2(Vec2::new(metric::CARD_PADDING, 0.0))),
        |ui| {
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let usable = app.saves_blocked.is_none() && app.task.is_none();

                let mut label = app.backup_label.clone();
                if widgets::text_field(ui, &mut label, "Etikett (optional)", 230.0, None).changed()
                {
                    actions.push(Action::SetBackupLabel(label));
                }
                if widgets::button(
                    ui,
                    &ButtonStyle::ghost().font(medium(12.5)),
                    None,
                    "Backup anlegen",
                    usable,
                )
                .clicked()
                {
                    actions.push(Action::CreateBackup);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} Backups · neueste zuerst",
                            app.backups.len()
                        ))
                        .font(mono(11.5))
                        .color(color::TEXT_DIM2),
                    );
                });
            });
        },
    );
    let head = Rect::from_min_size(
        Pos2::new(outer.left(), toolbar.bottom() + 1.0),
        Vec2::new(outer.width(), metric::TABLE_HEAD_HEIGHT),
    );
    super::draw_column_head(ui, head, &BACKUP_COLUMNS, &["ERSTELLT", "ETIKETT", "GRÖSSE", ""]);
    ui.painter().hline(outer.x_range(), toolbar.bottom() + 0.5, Stroke::new(1.0, color::BORDER_SOFT));

    let body = Rect::from_min_max(
        Pos2::new(outer.left(), head.bottom()),
        Pos2::new(outer.right(), outer.bottom()),
    );
    ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
        if app.backups.is_empty() {
            super::empty_hint(ui, "Noch keine Backups vorhanden.");
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for index in 0..app.backups.len() {
                row(app, ui, index, actions);
            }
        });
    });
}

/// Der Warnkasten, solange das Steam-Nutzerprofil nicht eindeutig ist.
fn blocked_banner(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let Some(reason) = &app.saves_blocked else { return };

    let width = ui.available_width();
    let text_width = width - 13.0 - 11.0 - 10.0 - 13.0;
    let galley = ui.painter().layout(reason.clone(), sans(12.0), color::TEXT, text_width);
    let height = galley.size().y + 9.0 + 28.0 + 22.0;

    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter();
    let radius = CornerRadius::same(8);
    painter.rect_filled(rect, radius, egui::Color32::from_rgb(0x17, 0x1a, 0x21));
    painter.rect_stroke(rect, radius, Stroke::new(1.0, color::BORDER_SOFT), StrokeKind::Inside);
    painter.rect_filled(
        Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())),
        CornerRadius { nw: 8, sw: 8, ne: 0, se: 0 },
        color::WARN,
    );

    super::icons::warning(
        painter,
        Pos2::new(rect.left() + 13.0 + 5.5, rect.top() + 11.0 + galley.size().y.min(16.0) / 2.0),
        11.0,
        color::WARN,
    );
    painter.galley(
        Pos2::new(rect.left() + 13.0 + 11.0 + 10.0, rect.top() + 11.0),
        galley.clone(),
        color::TEXT,
    );

    let controls = Rect::from_min_size(
        Pos2::new(rect.left() + 13.0 + 22.0, rect.top() + 11.0 + galley.size().y + 9.0),
        Vec2::new(rect.width() - 35.0, 28.0),
    );
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(controls)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let enabled = !app.steam_users.is_empty();
            if widgets::button(
                ui,
                &ButtonStyle::ghost().height(28.0).padding_x(12.0).font(sans(12.0)),
                None,
                "Nutzerprofil wählen",
                enabled,
            )
            .clicked()
            {
                actions.push(Action::OpenSteamUserDialog);
            }
            ui.label(
                egui::RichText::new("…/storage/steam/user/").font(mono(11.0)).color(color::TEXT_FAINT),
            );
        },
    );
}

fn row(app: &App, ui: &mut Ui, index: usize, actions: &mut Vec<Action>) {
    let entry = &app.backups[index];
    let width = ui.available_width();
    // `Sense::click()` statt `hover()`: ohne einen Klick-Sinn bekommt die
    // Zeile den rechten Mausklick nicht zu sehen, an dem das Kontextmenü
    // hängt.
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, metric::LIST_ROW_HEIGHT), Sense::click());
    // Die Zeile bleibt hervorgehoben, solange ihr Menü offen steht – sonst
    // ließe sich bei mehreren Backups nicht mehr erkennen, zu welchem das
    // Menü gehört, sobald der Zeiger darin liegt.
    if response.hovered() || response.context_menu_opened() {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, color::HOVER);
    }
    ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, color::BORDER_ROW));

    // Gesperrte Savegame-Funktionen zeigt der Entwurf als abgeblendete
    // Liste – lesbar, aber sichtbar außer Betrieb.
    let usable = app.saves_blocked.is_none() && app.task.is_none();
    let dim = |c: egui::Color32| if usable { c } else { c.gamma_multiply(0.45) };

    let cells = widgets::columns(
        rect.shrink2(Vec2::new(metric::CARD_PADDING, 0.0)),
        &BACKUP_COLUMNS,
        metric::COLUMN_GAP,
    );

    ui.painter().text(
        Pos2::new(cells[0].left(), cells[0].center().y),
        Align2::LEFT_CENTER,
        human_time(&entry.created_at),
        mono(11.5),
        dim(color::TEXT_STRONG),
    );

    let label = entry.label.clone().unwrap_or_else(|| String::from("—"));
    let label_galley =
        widgets::truncated(ui, &label, sans(12.5), dim(color::TEXT), cells[1].width() - 80.0);
    let label_width = label_galley.size().x;
    ui.painter().galley(
        Pos2::new(cells[1].left(), cells[1].center().y - label_galley.size().y / 2.0),
        label_galley,
        dim(color::TEXT),
    );
    if app.verified.contains(&entry.created_at) {
        let badge_rect = Rect::from_min_max(
            Pos2::new(cells[1].left() + label_width + 9.0, cells[1].top()),
            cells[1].max,
        );
        if badge_rect.width() > 70.0 {
            let mut badge_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(badge_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            widgets::badge(
                &mut badge_ui,
                Some(Icon::Check),
                "geprüft",
                dim(color::OK),
                color::OK_BG,
            );
        }
    }

    ui.painter().text(
        Pos2::new(cells[2].left(), cells[2].center().y),
        Align2::LEFT_CENTER,
        archive_size(entry),
        mono(11.5),
        dim(color::TEXT_DIM2),
    );

    let mut buttons = ui.new_child(
        UiBuilder::new()
            .max_rect(cells[3])
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    buttons.spacing_mut().item_spacing.x = 7.0;
    if widgets::button(
        &mut buttons,
        &ButtonStyle::warning(),
        Some(Icon::Warning),
        "Wiederherstellen",
        usable,
    )
    .clicked()
    {
        actions.push(Action::AskRestore(index));
    }
    if widgets::button(&mut buttons, &ButtonStyle::ghost().small(), None, "Prüfen", usable)
        .clicked()
    {
        actions.push(Action::VerifyBackup(index));
    }

    context_menu(app, &response, index, actions);
}

/// Das Kontextmenü einer Backup-Zeile: die beiden Knöpfe der Zeile plus
/// das, wofür dort kein Platz ist.
///
/// Umbenennen, Löschen und Anzeigen rühren nur an das Backup-Verzeichnis des
/// Loaders, nicht an die Spielstände. Sie bleiben deshalb auch dann nutzbar,
/// wenn die Savegame-Funktionen wegen eines ungeklärten Steam-Nutzerprofils
/// gesperrt sind – gerade dann hilft es, die vorhandenen Backups
/// aufzuräumen und zu beschriften.
fn context_menu(app: &App, response: &egui::Response, index: usize, actions: &mut Vec<Action>) {
    let idle = app.task.is_none();
    let usable = app.saves_blocked.is_none() && idle;

    response.context_menu(|ui| {
        ui.set_width(widgets::MENU_WIDTH);
        ui.spacing_mut().item_spacing.y = 1.0;

        if widgets::menu_item(ui, Some(Icon::Pencil), "Umbenennen …", idle, false) {
            actions.push(Action::AskRenameBackup(index));
        }
        if widgets::menu_item(ui, Some(Icon::Folder), "Im Dateimanager zeigen", idle, false) {
            actions.push(Action::ShowBackupInFiles(index));
        }

        widgets::menu_separator(ui);

        if widgets::menu_item(ui, Some(Icon::Check), "Prüfen", usable, false) {
            actions.push(Action::VerifyBackup(index));
        }
        if widgets::menu_item(ui, Some(Icon::Warning), "Wiederherstellen …", usable, false) {
            actions.push(Action::AskRestore(index));
        }

        widgets::menu_separator(ui);

        if widgets::menu_item(ui, Some(Icon::Trash), "Löschen …", idle, true) {
            actions.push(Action::AskDeleteBackup(index));
        }
    });
}

/// Größe des Archivs auf der Platte. Steht nirgends im Manifest – die Datei
/// weiß es selbst.
fn archive_size(entry: &sm2_core::saves::BackupEntry) -> String {
    std::fs::metadata(&entry.archive).map_or_else(|_| String::from("—"), |m| human_size(m.len()))
}

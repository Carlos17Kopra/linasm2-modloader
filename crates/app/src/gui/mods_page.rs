//! Der Bereich „Mods“: Hinweisleiste, Liste mit Ladereihenfolge,
//! Detailkarte – und der Erstlauf-Bildschirm, wenn das Spiel fehlt.

use super::theme::{color, medium, metric, mono, sans};
use super::widgets::{self, ButtonStyle, Column, Icon};
use super::format::{human_size, short_hash};
use super::{Action, App, NoticeAction};
use crate::app_state::NoticeKind;
use egui::{
    Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2,
};

/// Höhe der Detailkarte: Innenabstände, Titelzeile und drei Rasterzeilen –
/// fest, weil der Inhalt immer aus denselben Feldern besteht.
const DETAILS_HEIGHT: f32 = 163.0;

/// Spalten der Mod-Liste, wie im Entwurf notiert:
/// `22px 44px 1fr 72px 124px 56px`.
const MOD_COLUMNS: [Column; 6] = [
    Column::Fixed(22.0),
    Column::Fixed(44.0),
    Column::Flexible,
    Column::Fixed(72.0),
    Column::Fixed(124.0),
    Column::Fixed(56.0),
];

pub fn show(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    if app.state.is_none() {
        game_not_found(app, ui, actions);
        return;
    }

    notices(app, ui, actions);

    let selection = app
        .selected
        .clone()
        .filter(|pak| app.entries().iter().any(|entry| &entry.pak == pak));
    let details_space = if selection.is_some() { DETAILS_HEIGHT + metric::CONTENT_GAP } else { 0.0 };
    let list_height = (ui.available_height() - details_space).max(120.0);

    let list_rect = Rect::from_min_size(
        ui.available_rect_before_wrap().min,
        Vec2::new(ui.available_width(), list_height),
    );
    ui.scope_builder(UiBuilder::new().max_rect(list_rect), |ui| {
        mod_list(app, ui, actions);
    });

    if let Some(pak) = selection {
        ui.add_space(metric::CONTENT_GAP);
        details(app, ui, &pak, actions);
    }
}

/// Die Hinweisleiste über der Liste: eine Zeile je Meldung, links ein
/// farbiger Strich, rechts wahlweise eine Schaltfläche und immer ein Kreuz
/// zum Wegklicken.
fn notices(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    if app.notices.is_empty() {
        return;
    }

    for (index, notice) in app.notices.iter().enumerate() {
        let accent = match notice.kind {
            NoticeKind::Info => color::INFO,
            NoticeKind::Warning => color::WARN,
            NoticeKind::Error => color::DANGER,
        };
        let icon = match notice.kind {
            NoticeKind::Info => Icon::Dot,
            NoticeKind::Warning => Icon::Warning,
            NoticeKind::Error => Icon::Cross,
        };

        let action_width = notice.action.as_ref().map_or(0.0, |action| {
            let label = notice_action_label(action);
            ui.painter().layout_no_wrap(label.to_owned(), sans(11.5), color::TEXT).size().x + 22.0
                + 10.0
        });
        let text_width =
            (ui.available_width() - 12.0 - 11.0 - 10.0 - action_width - 11.0 - 12.0).max(40.0);
        let galley =
            ui.painter().layout(notice.text.clone(), sans(12.0), color::TEXT, text_width);
        let height = galley.size().y.max(11.0) + 16.0;

        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
        let painter = ui.painter();
        let radius = CornerRadius::same(8);
        painter.rect_filled(rect, radius, Color32::from_rgb(0x17, 0x1a, 0x21));
        painter.rect_stroke(rect, radius, Stroke::new(1.0, color::BORDER_SOFT), StrokeKind::Inside);
        painter.rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())),
            CornerRadius { nw: 8, sw: 8, ne: 0, se: 0 },
            accent,
        );

        icon.paint(painter, Pos2::new(rect.left() + 12.0 + 5.5, rect.center().y), 11.0, accent);
        painter.galley(
            Pos2::new(rect.left() + 12.0 + 11.0 + 10.0, rect.center().y - galley.size().y / 2.0),
            galley,
            color::TEXT,
        );

        // Kreuz zum Wegklicken, ganz rechts.
        let dismiss = Rect::from_center_size(
            Pos2::new(rect.right() - 12.0 - 5.5, rect.center().y),
            Vec2::splat(18.0),
        );
        let dismiss_response =
            ui.interact(dismiss, egui::Id::new(("notice_dismiss", index)), Sense::click());
        super::icons::cross(
            ui.painter(),
            dismiss.center(),
            11.0,
            if dismiss_response.hovered() { color::TEXT_STRONG } else { color::TEXT_FAINT },
        );
        if dismiss_response.clicked() {
            actions.push(Action::DismissNotice(index));
        }

        if let Some(action) = &notice.action {
            let label = notice_action_label(action);
            let width = action_width - 10.0;
            let button_rect = Rect::from_min_size(
                Pos2::new(dismiss.left() - 10.0 - width, rect.center().y - 12.5),
                Vec2::new(width, 25.0),
            );
            let response = ui.interact(
                button_rect,
                egui::Id::new(("notice_action", index)),
                Sense::click(),
            );
            let (border, foreground) = if response.hovered() {
                (color::ACCENT, color::TEXT_STRONG)
            } else {
                (color::BORDER_STRONG, color::TEXT)
            };
            let painter = ui.painter();
            painter.rect_filled(button_rect, CornerRadius::same(6), color::HOVER);
            painter.rect_stroke(
                button_rect,
                CornerRadius::same(6),
                Stroke::new(1.0, border),
                StrokeKind::Inside,
            );
            painter.text(
                button_rect.center(),
                Align2::CENTER_CENTER,
                label,
                sans(11.5),
                foreground,
            );
            if response.clicked() {
                actions.push(Action::TriggerNotice(index));
            }
        }

        ui.add_space(6.0);
    }
    ui.add_space(metric::CONTENT_GAP - 6.0);
}

fn notice_action_label(action: &NoticeAction) -> &'static str {
    match action {
        NoticeAction::OpenModsDir => "Verzeichnis öffnen",
        NoticeAction::ShowProfile(_) => "Profil anwenden",
    }
}

/// Die Karte mit der Mod-Liste: Werkzeugleiste, Spaltenkopf, Zeilen.
fn mod_list(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let outer = ui.available_rect_before_wrap();
    let painter = ui.painter();
    let radius = CornerRadius::same(12);
    painter.rect_filled(outer, radius, color::CARD);
    painter.rect_stroke(outer, radius, Stroke::new(1.0, color::BORDER_SOFT), StrokeKind::Inside);

    let toolbar = Rect::from_min_size(
        outer.min,
        Vec2::new(outer.width(), metric::CARD_TOOLBAR_HEIGHT),
    );
    ui.scope_builder(
        UiBuilder::new().max_rect(toolbar.shrink2(Vec2::new(metric::CARD_PADDING, 0.0))),
        |ui| toolbar_row(app, ui, actions),
    );
    let head = Rect::from_min_size(
        Pos2::new(outer.left(), toolbar.bottom() + 1.0),
        Vec2::new(outer.width(), metric::TABLE_HEAD_HEIGHT),
    );
    column_head(ui, head);
    ui.painter().hline(
        outer.x_range(),
        toolbar.bottom() + 0.5,
        Stroke::new(1.0, color::BORDER_SOFT),
    );

    let body = Rect::from_min_max(
        Pos2::new(outer.left(), head.bottom()),
        Pos2::new(outer.right(), outer.bottom()),
    );

    if app.entries().is_empty() {
        ui.scope_builder(UiBuilder::new().max_rect(body), |ui| empty_state(ui, actions));
        return;
    }

    ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // Die Zeilen stoßen aneinander; ihre Trennung ist die 1-px-Linie,
            // nicht ein Abstand.
            ui.spacing_mut().item_spacing.y = 0.0;
            rows(app, ui, actions);
        });
    });
}

/// Import, Filter, Zieh-Hinweis, Zähler.
fn toolbar_row(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 10.0;

        let can_modify = app.can_modify();
        if widgets::button(ui, &ButtonStyle::ghost(), Some(Icon::Plus), "Importieren", can_modify)
            .clicked()
        {
            actions.push(Action::Import);
        }

        let mut filter = app.filter.clone();
        if widgets::text_field(ui, &mut filter, "Filtern", 220.0, Some(Icon::Search)).changed() {
            actions.push(Action::SetFilter(filter));
        }

        ui.label(
            egui::RichText::new(drag_hint(app)).font(sans(11.0)).color(color::TEXT_FAINT),
        );

        let total = app.entries().len();
        let active = app.entries().iter().filter(|e| !e.disabled).count();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{active} von {total} aktiv"))
                    .font(mono(11.5))
                    .color(color::TEXT_DIM2),
            );
        });
    });
}

fn drag_hint(app: &App) -> String {
    if !app.writable {
        String::from("Sortieren und Import gesperrt – Mods-Verzeichnis ist schreibgeschützt")
    } else if app.task.is_some() {
        String::from("Ein Vorgang läuft – Änderungen sind so lange gesperrt")
    } else if app.drag.is_some() {
        String::from("Einfügemarke zeigt die neue Position – loslassen zum Ablegen")
    } else if !app.filter.trim().is_empty() {
        String::from("Ziehen erst ohne Filter möglich")
    } else {
        String::from("Zeilen ziehen zum Sortieren · Dateien aufs Fenster ziehen zum Importieren")
    }
}

fn column_head(ui: &Ui, rect: Rect) {
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, color::TABLE_HEAD);
    painter.hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, color::BORDER_SOFT));

    let inner = rect.shrink2(Vec2::new(metric::CARD_PADDING, 0.0));
    let cells = widgets::columns(inner, &MOD_COLUMNS, metric::COLUMN_GAP);
    let font = sans(10.5);
    let head = |index: usize, text: &str| {
        let galley = painter.layout_job(widgets::tracked_text(text, font.clone(), color::TEXT_FAINT, 0.75));
        painter.galley(
            Pos2::new(cells[index].left(), cells[index].center().y - galley.size().y / 2.0),
            galley,
            color::TEXT_FAINT,
        );
    };
    head(1, "AN");
    head(2, "MOD · LADEREIHENFOLGE, OBEN GEWINNT");
    head(3, "VERSION");
    head(4, "STATUS");
    painter.text(
        cells[5].center(),
        Align2::CENTER_CENTER,
        "RANG",
        font,
        color::TEXT_FAINT,
    );
}

/// Die Zeilen der Liste, samt Ziehen zum Sortieren.
fn rows(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let filter = app.filter.trim().to_lowercase();
    let can_drag = app.can_modify() && filter.is_empty();

    let entries: Vec<(usize, String, bool)> = app
        .entries()
        .iter()
        .enumerate()
        .map(|(index, entry)| (index, entry.pak.clone(), entry.disabled))
        .filter(|(_, pak, _)| {
            filter.is_empty()
                || format!("{} {pak}", app.display_name(pak)).to_lowercase().contains(&filter)
        })
        .collect();

    let width = ui.available_width();
    let mut drop_marker: Option<f32> = None;

    for (index, pak, disabled) in &entries {
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(width, metric::ROW_HEIGHT), Sense::click_and_drag());
        let selected = app.selected.as_deref() == Some(pak.as_str());
        let dragged = app.drag.as_ref().is_some_and(|d| d.pak == *pak);

        row_background(ui, rect, selected, response.hovered(), dragged);
        row_content(app, ui, rect, index + 1, pak, *disabled, can_drag, actions);

        if response.clicked() {
            actions.push(Action::SelectMod(pak.clone()));
        }

        if can_drag {
            if response.drag_started() {
                actions.push(Action::DragStart(pak.clone()));
            }
            if response.dragged() || response.drag_stopped() {
                if let Some(pointer) = ui.ctx().pointer_interact_pos() {
                    let before =
                        if pointer.y > rect.center().y { index + 1 } else { *index };
                    if response.drag_stopped() {
                        actions.push(Action::DropMod { pak: pak.clone(), before });
                    } else {
                        actions.push(Action::DragOver(before));
                    }
                }
            }
        }

        if let Some(drag) = &app.drag {
            if drag.drop_index == Some(*index) && !dragged {
                drop_marker = Some(rect.top());
            }
            if drag.drop_index == Some(index + 1) && *index + 1 == entries.len() {
                drop_marker = Some(rect.bottom());
            }
        }

        ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, color::BORDER_ROW));
    }

    if app.drag.is_some() && app.drag.as_ref().is_none_or(|d| d.drop_index.is_none()) {
        // Ohne Zielangabe keine Marke – sonst stünde sie irreführend an der
        // zuletzt bekannten Stelle.
    }
    if let Some(y) = drop_marker {
        ui.painter().hline(
            egui::Rangef::new(ui.min_rect().left(), ui.min_rect().right()),
            y,
            Stroke::new(2.0, color::ACCENT),
        );
    }
}

fn row_background(ui: &Ui, rect: Rect, selected: bool, hovered: bool, dragged: bool) {
    let background = match (selected, hovered) {
        (true, true) => color::SELECTED_HOVER,
        (true, false) => color::SELECTED,
        (false, true) => color::HOVER,
        (false, false) => Color32::TRANSPARENT,
    };
    let background = if dragged { background.gamma_multiply(0.4) } else { background };
    ui.painter().rect_filled(rect, CornerRadius::ZERO, background);
    if selected {
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(2.0, rect.height())),
            CornerRadius::ZERO,
            color::ACCENT,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn row_content(
    app: &App,
    ui: &mut Ui,
    rect: Rect,
    rank: usize,
    pak: &str,
    disabled: bool,
    can_drag: bool,
    actions: &mut Vec<Action>,
) {
    let cells = widgets::columns(
        rect.shrink2(Vec2::new(metric::CARD_PADDING, 0.0)),
        &MOD_COLUMNS,
        metric::COLUMN_GAP,
    );
    let info = app.mod_info(pak);
    let missing = info.is_none();
    let altered = info.is_some_and(|i| i.known_altered);

    // Anfasser.
    Icon::Grip.paint(
        ui.painter(),
        cells[0].center(),
        12.0,
        if can_drag { color::TEXT_FAINT } else { color::BORDER_STRONG },
    );

    // Schalter.
    let toggle_rect = Rect::from_min_size(
        Pos2::new(cells[1].left(), cells[1].center().y - 8.5),
        Vec2::new(30.0, 17.0),
    );
    let mut toggle_ui = ui.new_child(UiBuilder::new().max_rect(toggle_rect));
    if widgets::toggle(&mut toggle_ui, !disabled, app.can_modify()).clicked() {
        actions.push(Action::ToggleMod(pak.to_string()));
    }

    // Name und Dateiname.
    let name = app.display_name(pak);
    let name_color = if missing {
        color::TEXT_MUTED
    } else if disabled {
        color::TEXT_DIM2
    } else {
        color::TEXT_STRONG
    };
    // Der Name bekommt höchstens zwei Drittel der Spalte, damit der
    // Dateiname daneben nicht grundsätzlich verschwindet; braucht er
    // weniger, fällt der Rest an den Dateinamen.
    let name_galley =
        widgets::truncated(ui, &name, medium(13.0), name_color, cells[2].width() * 0.66);
    let name_width = name_galley.size().x;
    ui.painter().galley(
        Pos2::new(cells[2].left(), cells[2].center().y - name_galley.size().y / 2.0),
        name_galley,
        name_color,
    );
    let pak_rect = Rect::from_min_max(
        Pos2::new((cells[2].left() + name_width + 9.0).min(cells[2].right()), cells[2].top()),
        cells[2].max,
    );
    widgets::column_text(ui, pak_rect, pak, mono(11.0), color::TEXT_FAINT);

    // Version.
    let version = info.and_then(|i| i.version.clone()).unwrap_or_else(|| String::from("—"));
    widgets::column_text(ui, cells[3], &version, mono(11.5), color::TEXT_DIM2);

    // Statusmerker.
    if let Some((icon, label, foreground, background)) = badge_for(missing, altered) {
        let mut badge_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(cells[4])
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        widgets::badge(&mut badge_ui, Some(icon), label, foreground, background);
    }

    // Rang: hoch und runter.
    let button_size = Vec2::new(22.0, 20.0);
    let up = Rect::from_min_size(
        Pos2::new(cells[5].center().x - 22.5, cells[5].center().y - 10.0),
        button_size,
    );
    let down = Rect::from_min_size(
        Pos2::new(cells[5].center().x + 1.5, cells[5].center().y - 10.0),
        button_size,
    );
    if rank_button(ui, up, Icon::TriangleUp, ("rank_up", pak)) {
        actions.push(Action::MoveMod { pak: pak.to_string(), delta: -1 });
    }
    if rank_button(ui, down, Icon::TriangleDown, ("rank_down", pak)) {
        actions.push(Action::MoveMod { pak: pak.to_string(), delta: 1 });
    }
    let _ = rank;
}

/// Welchen Merker eine Zeile trägt.
///
/// Der Entwurf kennt fünf; drei davon („ÜBERNOMMEN“, „NEU“, „ZURÜCK“)
/// beschreiben, was der letzte Abgleich getan hat, und stehen deshalb als
/// Meldung in der Hinweisleiste statt dauerhaft in der Zeile – nach einem
/// Neustart wären sie sonst nicht mehr wahr. In der Zeile bleiben die
/// beiden Zustände, die sich jederzeit aus Bibliothek und Verzeichnis
/// ablesen lassen.
fn badge_for(missing: bool, altered: bool) -> Option<(Icon, &'static str, Color32, Color32)> {
    if missing {
        Some((Icon::Warning, "ÜBERNOMMEN", color::WARN, color::WARN_BG))
    } else if altered {
        Some((Icon::Warning, "GEÄNDERT", color::WARN, color::WARN_BG))
    } else {
        None
    }
}

fn rank_button(ui: &mut Ui, rect: Rect, icon: Icon, salt: (&str, &str)) -> bool {
    let response = ui.interact(rect, egui::Id::new(salt), Sense::click());
    let (border, foreground) = if response.hovered() {
        (color::ACCENT, color::TEXT_STRONG)
    } else {
        (color::BORDER, color::TEXT_DIM2)
    };
    let painter = ui.painter();
    let radius = CornerRadius::same(5);
    painter.rect_filled(rect, radius, color::CONTROL);
    painter.rect_stroke(rect, radius, Stroke::new(1.0, border), StrokeKind::Inside);
    icon.paint(painter, rect.center(), 9.0, foreground);
    response.clicked()
}

/// „Noch keine Mods installiert“.
fn empty_state(ui: &mut Ui, actions: &mut Vec<Action>) {
    let rect = ui.available_rect_before_wrap();
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(60.0, 40.0)))
            .layout(egui::Layout::top_down(egui::Align::Center)),
        |ui| {
            ui.add_space(12.0);
            let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(38.0), Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(icon_rect, CornerRadius::same(10), color::CONTROL);
            painter.rect_stroke(
                icon_rect,
                CornerRadius::same(10),
                Stroke::new(1.0, color::BORDER),
                StrokeKind::Inside,
            );
            super::icons::plus(painter, icon_rect.center(), 15.0, color::ACCENT);

            ui.add_space(13.0);
            ui.label(
                egui::RichText::new("Noch keine Mods installiert")
                    .font(medium(17.0))
                    .color(color::TEXT_STRONG),
            );
            ui.add_space(13.0);
            ui.set_max_width(490.0);
            ui.label(
                egui::RichText::new(
                    "Lade den Mod bei Nexus herunter und importiere die Datei hier. Der Loader \
                     nimmt .pak, .zip, .7z und .rar – auch mehrere auf einmal. Er kopiert das Pak \
                     ins Mods-Verzeichnis des Spiels und trägt es deaktiviert ans Ende der \
                     Ladereihenfolge ein; ein Import verändert dein laufendes Setup nicht.",
                )
                .font(sans(12.5))
                .color(color::TEXT_DIM2),
            );
            ui.add_space(15.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if widgets::button(ui, &ButtonStyle::primary().padding_x(16.0), None, "Dateien wählen", true)
                    .clicked()
                {
                    actions.push(Action::Import);
                }
                ui.label(
                    egui::RichText::new("oder hierher ziehen")
                        .font(sans(11.5))
                        .color(color::TEXT_FAINT),
                );
            });
        },
    );
}

/// Die Detailkarte unter der Liste.
///
/// Auch ein von Hand ins Mods-Verzeichnis gelegtes Pak bekommt sie: es hat
/// keinen Bibliothekseintrag, aber genau das ist die Auskunft, die der
/// Nutzer an dieser Stelle braucht – sonst bliebe ausgerechnet die Zeile
/// ohne Erklärung, die als Einzige einen Warnmerker trägt.
fn details(app: &App, ui: &mut Ui, pak: &str, actions: &mut Vec<Action>) {
    let info = app.mod_info(pak);
    let rect = Rect::from_min_size(
        ui.available_rect_before_wrap().min,
        Vec2::new(ui.available_width(), DETAILS_HEIGHT),
    );
    let (_, _) = ui.allocate_exact_size(rect.size(), Sense::hover());

    let painter = ui.painter();
    let radius = CornerRadius::same(12);
    painter.rect_filled(rect, radius, color::CARD);
    painter.rect_stroke(rect, radius, Stroke::new(1.0, color::BORDER_SOFT), StrokeKind::Inside);

    let inner = rect.shrink2(Vec2::new(14.0, 0.0));

    // Titelzeile.
    let title_y = rect.top() + 12.0 + 9.0;
    let display_name = app.display_name(pak);
    let name_galley = painter.layout_no_wrap(display_name, medium(13.5), color::TEXT_STRONG);
    painter.galley(
        Pos2::new(inner.left(), title_y - name_galley.size().y / 2.0),
        name_galley.clone(),
        color::TEXT_STRONG,
    );
    let pak_rect = Rect::from_min_max(
        Pos2::new(inner.left() + name_galley.size().x + 10.0, title_y - 8.0),
        Pos2::new(inner.right() - 24.0, title_y + 8.0),
    );
    widgets::column_text(ui, pak_rect, pak, mono(11.0), color::TEXT_FAINT);

    let close = Rect::from_center_size(Pos2::new(inner.right() - 6.0, title_y), Vec2::splat(18.0));
    let close_response = ui.interact(close, egui::Id::new("details_close"), Sense::click());
    super::icons::cross(
        ui.painter(),
        close.center(),
        11.0,
        if close_response.hovered() { color::TEXT_STRONG } else { color::TEXT_FAINT },
    );
    if close_response.clicked() {
        actions.push(Action::ClearSelection);
    }

    // Raster: vier Spalten, drei Zeilen.
    let grid_top = rect.top() + 12.0 + 18.0 + 9.0;
    let column_width = (inner.width() - 3.0 * 18.0) / 4.0;
    let cell = |column: usize, row: usize, span: usize| {
        Rect::from_min_size(
            Pos2::new(
                inner.left() + column as f32 * (column_width + 18.0),
                grid_top + row as f32 * 40.0,
            ),
            Vec2::new(column_width * span as f32 + 18.0 * (span - 1) as f32, 31.0),
        )
    };

    let field = |ui: &Ui, rect: Rect, label: &str, value: &str, monospaced: bool| {
        let painter = ui.painter();
        let label_galley =
            painter.layout_job(widgets::tracked_text(label, sans(10.5), color::TEXT_FAINT, 0.5));
        painter.galley(Pos2::new(rect.left(), rect.top()), label_galley, color::TEXT_FAINT);
        let font = if monospaced { mono(11.0) } else { sans(11.5) };
        widgets::column_text(
            ui,
            Rect::from_min_size(
                Pos2::new(rect.left(), rect.top() + 16.0),
                Vec2::new(rect.width(), 15.0),
            ),
            value,
            font,
            color::TEXT,
        );
    };

    let size = match info {
        Some(info) => human_size(info.size),
        None => app
            .state
            .as_ref()
            .and_then(|state| std::fs::metadata(state.paths.mods_dir().join(pak)).ok())
            .map_or_else(|| String::from("—"), |m| human_size(m.len())),
    };
    let imported = info.map_or("—", |i| i.imported_at.as_str());
    let source = match info {
        Some(info) => info.source.clone().unwrap_or_else(|| String::from("—")),
        None => String::from("von Hand ins Mods-Verzeichnis gelegt"),
    };
    let hash = match info {
        Some(info) => short_hash(&info.hash),
        None => String::from("noch nicht gebildet"),
    };
    let notes = match info {
        Some(info) => info.notes.clone().unwrap_or_else(|| String::from("—")),
        None => String::from("Nicht über den Loader importiert – Metadaten unbekannt."),
    };

    field(ui, cell(0, 0, 1), "AUTOR", info.and_then(|i| i.author.as_deref()).unwrap_or("—"), false);
    field(ui, cell(1, 0, 1), "VERSION", info.and_then(|i| i.version.as_deref()).unwrap_or("—"), false);
    field(ui, cell(2, 0, 1), "GRÖSSE", &size, false);
    field(ui, cell(3, 0, 1), "IMPORTIERT", imported, false);
    field(
        ui,
        cell(0, 1, 1),
        "NEXUS-ID",
        &info.and_then(|i| i.nexus_id).map_or_else(|| String::from("—"), |id| id.to_string()),
        false,
    );
    field(ui, cell(1, 1, 2), "HERKUNFT", &source, true);
    field(ui, cell(3, 1, 1), "HASH (BLAKE3)", &hash, true);
    field(ui, cell(0, 2, 4), "NOTIZEN", &notes, false);
}

/// Der Erstlauf-Bildschirm, wenn keine Installation gefunden wurde.
///
/// Die Karte wächst mit ihrem Inhalt, statt eine feste Höhe zu haben: wie
/// viele Suchorte aufgezählt werden, hängt vom System ab (Flatpak-Steam,
/// zusätzliche Bibliotheken), und eine zu kleine Karte schöbe ausgerechnet
/// die beiden Schaltflächen über ihren eigenen Rand hinaus.
fn game_not_found(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let card_width = 600.0_f32.min(ui.available_width());
        let side_gap = ((ui.available_width() - card_width) / 2.0).max(0.0);

        ui.add_space(24.0);
        ui.horizontal(|ui| {
            ui.add_space(side_gap);
            ui.scope_builder(
                UiBuilder::new().max_rect(Rect::from_min_size(
                    Pos2::new(ui.cursor().left(), ui.cursor().top()),
                    Vec2::new(card_width, ui.available_height()),
                )),
                |ui| {
                    widgets::card()
                        .inner_margin(egui::Margin::same(26))
                        .show(ui, |ui| {
                            ui.set_width(card_width - 52.0);
                            ui.with_layout(
                                egui::Layout::top_down(egui::Align::Center),
                                |ui| game_not_found_content(app, ui, actions),
                            );
                        });
                },
            );
        });
    });
}

fn game_not_found_content(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(38.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(icon_rect, CornerRadius::same(10), color::WARN_BG);
    painter.rect_stroke(
        icon_rect,
        CornerRadius::same(10),
        Stroke::new(1.0, color::WARN_BORDER),
        StrokeKind::Inside,
    );
    super::icons::warning(painter, icon_rect.center(), 15.0, color::WARN);

    ui.add_space(14.0);
    ui.label(
        egui::RichText::new("Space Marine 2 nicht gefunden")
            .font(medium(17.0))
            .color(color::TEXT_STRONG),
    );
    ui.add_space(14.0);
    ui.label(
        egui::RichText::new(
            "Der Loader hat die Steam-Bibliotheken durchsucht und keine Installation gefunden. \
             Alles andere funktioniert weiter – sobald das Spielverzeichnis bekannt ist, liest er \
             Mods und Ladereihenfolge ein.",
        )
        .font(sans(12.5))
        .color(color::TEXT_DIM2),
    );

    ui.add_space(14.0);
    widgets::inset().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(widgets::tracked_text(
                "GESUCHT WURDE IN",
                sans(10.5),
                color::TEXT_FAINT,
                0.6,
            ));
            for root in <sm2_core::platform::Current as sm2_core::platform::Platform>::steam_roots()
            {
                ui.label(
                    egui::RichText::new(
                        root.join("steamapps/common/Space Marine 2").display().to_string(),
                    )
                    .font(mono(11.0))
                    .color(color::TEXT_DIM2),
                );
            }
            ui.label(
                egui::RichText::new("sowie alle Bibliotheken aus libraryfolders.vdf")
                    .font(mono(11.0))
                    .color(color::TEXT_DIM2),
            );
        });
    });

    ui.add_space(14.0);
    ui.horizontal(|ui| {
        // Die beiden Schaltflächen als Block in der Mitte der Karte.
        let width = ui.available_width();
        ui.add_space(((width - 330.0) / 2.0).max(0.0));
        ui.spacing_mut().item_spacing.x = 10.0;
        if widgets::button(
            ui,
            &ButtonStyle::primary().padding_x(16.0),
            None,
            "Spielverzeichnis wählen",
            true,
        )
        .clicked()
        {
            actions.push(Action::PickGameDir);
        }
        if widgets::button(
            ui,
            &ButtonStyle::ghost().height(metric::BUTTON_HEIGHT_LARGE).corner_radius(8),
            None,
            "Erneut suchen",
            true,
        )
        .clicked()
        {
            actions.push(Action::RetryDiscover);
        }
    });

    ui.add_space(10.0);
    ui.label(
        egui::RichText::new("Das Verzeichnis muss client_pc/root/mods enthalten.")
            .font(sans(11.0))
            .color(color::TEXT_FAINT),
    );

    if let Some(error) = &app.open_error {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(error).font(sans(11.0)).color(color::WARN));
    }
}

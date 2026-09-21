//! The "compose a save" tab: one backup is the base and supplies every
//! part, single parts are taken from other backups, and the result is
//! written as a new backup.
//!
//! Every decision on this page was made in `compose::ComposeUi`, which
//! is where the tests are. This file only puts the answers on screen.

use super::compose::{GroupRow, GroupSource, PartRow, Picker, Row};
use super::format::human_time;
use super::theme::{color, medium, metric, mono, sans};
use super::widgets::{self, ButtonStyle, Column, Icon};
use super::{Action, App};
use egui::{
    Align, Align2, CornerRadius, Id, Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui,
    UiBuilder, Vec2,
};
use sm2_core::savedata::summary;
use sm2_core::t;

/// The part table: part, source, value, reset.
const COMPOSE_COLUMNS: [Column; 4] = [
    Column::Flexible,
    Column::Fixed(210.0),
    Column::Fixed(120.0),
    Column::Fixed(34.0),
];

/// The base picker's card: a caption line and the field below it.
const BASE_CARD_HEIGHT: f32 = 64.0;
const GROUP_ROW_HEIGHT: f32 = 34.0;
/// A part row carries two lines — the id and the file it lives in.
const PART_ROW_HEIGHT: f32 = 46.0;
const FOOTER_HEIGHT: f32 = 40.0;
const WARNING_HEIGHT: f32 = 24.0;
/// The dropdown field inside a table cell.
const FIELD_HEIGHT: f32 = 26.0;

pub fn show(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    // Not `app.saves_blocked.is_none() && ...`, which is what the
    // Backups tab next door asks: composing reads backups and writes a
    // backup and never touches the game's save directory. Someone who
    // imported backups from another launcher and has no game installed
    // at all can still merge them here.
    let usable = app.task.is_none();

    if app.backups.is_empty() {
        super::empty_hint(ui, &t!("gui.compose.no_backups"));
        return;
    }

    // Where the open dropdown's field ended up. It is only known once
    // the row owning it has been drawn, and the menu has to be drawn
    // last, over everything it covers.
    let mut anchor: Option<Rect> = None;

    let outer = ui.available_rect_before_wrap();
    let mut top = outer.top();

    let base_rect = Rect::from_min_size(
        Pos2::new(outer.left(), top),
        Vec2::new(outer.width(), BASE_CARD_HEIGHT),
    );
    base_card(app, ui, base_rect, usable, actions, &mut anchor);
    top = base_rect.bottom() + metric::CONTENT_GAP;

    if let Some(error) = &app.compose.error {
        top = error_banner(ui, outer, top, error) + metric::CONTENT_GAP;
    }

    let mut bottom = outer.bottom() - FOOTER_HEIGHT;
    footer(
        app,
        ui,
        Rect::from_min_max(Pos2::new(outer.left(), bottom), outer.max),
        usable,
        actions,
    );
    if let Some(warning) = app.compose.version_warning(&app.compose.parts) {
        bottom -= WARNING_HEIGHT;
        let line = Rect::from_min_size(
            Pos2::new(outer.left(), bottom),
            Vec2::new(outer.width(), WARNING_HEIGHT),
        );
        warning_line(ui, line, &warning);
    }

    // The card takes what is left. A window shrunk below the table's
    // own minimum keeps a readable rest rather than an inverted rect.
    let card = Rect::from_min_max(
        Pos2::new(outer.left(), top),
        Pos2::new(outer.right(), (bottom - metric::CONTENT_GAP).max(top + 140.0)),
    );
    table(app, ui, card, usable, actions, &mut anchor);

    if let Some(anchor) = anchor {
        picker_menu(app, ui, anchor, actions);
    }
}

/// The base picker: which backup supplies everything, what it holds, and
/// the way back to it for every part at once.
fn base_card(
    app: &App,
    ui: &mut Ui,
    rect: Rect,
    usable: bool,
    actions: &mut Vec<Action>,
    anchor: &mut Option<Rect>,
) {
    super::draw_card(ui, rect);
    let inner = rect.shrink2(Vec2::new(metric::CARD_PADDING, 0.0));

    ui.painter().text(
        Pos2::new(inner.left(), rect.top() + 16.0),
        Align2::LEFT_CENTER,
        t!("gui.compose.base"),
        sans(11.5),
        color::TEXT_DIM2,
    );

    let base = app.compose.base.as_deref();
    let entry = app.backups.iter().find(|entry| Some(entry.created_at.as_str()) == base);
    let label =
        entry.map_or_else(|| t!("gui.saves.no_value"), |entry| human_time(&entry.created_at));
    let cell = Rect::from_min_size(
        Pos2::new(inner.left(), rect.top() + 26.0),
        Vec2::new(230.0, FIELD_HEIGHT),
    );
    let response = source_field(
        app,
        ui,
        cell,
        Id::new("compose_base_field"),
        &label,
        app.compose.base.as_deref(),
        usable,
    );
    if response.clicked() {
        actions.push(Action::OpenComposePicker(Picker::Base));
    }
    if matches!(app.compose.picker, Some(Picker::Base)) {
        *anchor = Some(response.rect);
    }

    let size = entry.map_or_else(|| t!("gui.saves.no_value"), super::saves_page::archive_size);
    ui.painter().text(
        Pos2::new(cell.right() + 12.0, cell.center().y),
        Align2::LEFT_CENTER,
        t!("gui.compose.base_meta", size = size, parts = app.compose.parts.len()),
        mono(11.5),
        color::TEXT_DIM2,
    );

    let mut buttons = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                Pos2::new(inner.left(), cell.top()),
                Pos2::new(inner.right(), cell.bottom()),
            ))
            .layout(Layout::right_to_left(Align::Center)),
    );
    if widgets::button(
        &mut buttons,
        &ButtonStyle::ghost().small(),
        None,
        &t!("gui.compose.reset_all"),
        usable && !app.compose.sources.is_empty(),
    )
    .clicked()
    {
        actions.push(Action::ResetComposition);
    }
}

/// A failed read, above the table until the user acts on it. The box is
/// `saves_page::blocked_banner`'s, without its buttons.
fn error_banner(ui: &mut Ui, outer: Rect, top: f32, message: &str) -> f32 {
    let text_width = outer.width() - 13.0 - 11.0 - 10.0 - 13.0;
    let galley = ui.painter().layout(message.to_owned(), sans(12.0), color::TEXT, text_width);
    let rect = Rect::from_min_size(
        Pos2::new(outer.left(), top),
        Vec2::new(outer.width(), galley.size().y + 22.0),
    );

    let painter = ui.painter();
    let radius = CornerRadius::same(8);
    painter.rect_filled(rect, radius, egui::Color32::from_rgb(0x17, 0x1a, 0x21));
    painter.rect_stroke(rect, radius, Stroke::new(1.0, color::BORDER_SOFT), StrokeKind::Inside);
    painter.rect_filled(
        Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())),
        CornerRadius { nw: 8, sw: 8, ne: 0, se: 0 },
        color::DANGER,
    );
    super::icons::warning(
        painter,
        Pos2::new(rect.left() + 13.0 + 5.5, rect.center().y),
        11.0,
        color::DANGER,
    );
    painter.galley(
        Pos2::new(rect.left() + 13.0 + 11.0 + 10.0, rect.top() + 11.0),
        galley,
        color::TEXT,
    );
    rect.bottom()
}

/// The card with the filter, the toggle, the column head and the rows.
fn table(
    app: &App,
    ui: &mut Ui,
    outer: Rect,
    usable: bool,
    actions: &mut Vec<Action>,
    anchor: &mut Option<Rect>,
) {
    super::draw_card(ui, outer);

    let rows = app.compose.rows(&app.compose.parts);
    // Parts, not rows: a collapsed group stands for all of its own, and
    // the number belongs to the filter beside it, not to the triangles.
    // While a filter is on, `rows` draws every matching group open, so
    // the collapsed arm cannot double-count what is listed below it.
    let shown: usize = rows
        .iter()
        .map(|row| match row {
            Row::Group(group) if !group.open => group.total,
            Row::Group(_) => 0,
            Row::Part(_) => 1,
        })
        .sum();

    let toolbar =
        Rect::from_min_size(outer.min, Vec2::new(outer.width(), metric::CARD_TOOLBAR_HEIGHT));
    ui.scope_builder(
        UiBuilder::new().max_rect(toolbar.shrink2(Vec2::new(metric::CARD_PADDING, 0.0))),
        |ui| {
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;

                let mut filter = app.compose.part_filter.clone();
                let placeholder = t!("gui.compose.filter_placeholder");
                if widgets::text_field(ui, &mut filter, &placeholder, 220.0, Some(Icon::Search))
                    .changed()
                {
                    actions.push(Action::SetPartFilter(filter));
                }
                if widgets::toggle(ui, app.compose.only_replaced, true).clicked() {
                    actions.push(Action::ToggleOnlyReplaced);
                }
                ui.label(
                    RichText::new(t!("gui.compose.only_replaced"))
                        .font(sans(12.0))
                        .color(color::TEXT_DIM),
                );

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(t!(
                            "gui.compose.count",
                            shown = shown,
                            total = app.compose.parts.len()
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
    let col_part = t!("gui.compose.col_part");
    let col_source = t!("gui.compose.col_source");
    let col_value = t!("gui.compose.col_value");
    super::draw_column_head(
        ui,
        head,
        &COMPOSE_COLUMNS,
        &[col_part.as_str(), col_source.as_str(), col_value.as_str(), ""],
    );
    ui.painter().hline(
        outer.x_range(),
        toolbar.bottom() + 0.5,
        Stroke::new(1.0, color::BORDER_SOFT),
    );

    let body = Rect::from_min_max(
        Pos2::new(outer.left(), head.bottom()),
        Pos2::new(outer.right(), outer.bottom()),
    );
    ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
        if rows.is_empty() {
            // Nothing at all rather than "no match" while the base is
            // still being decoded: there is no part list yet to filter.
            let hint = if app.compose.parts.is_empty() && app.compose.loading.is_some() {
                t!("gui.compose.loading")
            } else {
                t!("gui.compose.parts_empty")
            };
            super::empty_hint(ui, &hint);
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in &rows {
                match row {
                    Row::Group(group) => group_row(app, ui, group, usable, actions, anchor),
                    Row::Part(part) => part_row(app, ui, part, usable, actions, anchor),
                }
            }
        });
    });
}

fn group_row(
    app: &App,
    ui: &mut Ui,
    group: &GroupRow,
    usable: bool,
    actions: &mut Vec<Action>,
    anchor: &mut Option<Rect>,
) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), GROUP_ROW_HEIGHT), Sense::hover());
    let background = if response.hovered() { color::HOVER } else { color::TABLE_HEAD };
    ui.painter().rect_filled(rect, CornerRadius::ZERO, background);
    ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, color::BORDER_ROW));

    let cells = widgets::columns(
        rect.shrink2(Vec2::new(metric::CARD_PADDING, 0.0)),
        &COMPOSE_COLUMNS,
        metric::COLUMN_GAP,
    );

    // Only the name half opens and closes the group, not the whole row:
    // the row also carries the source dropdown, and one click must not
    // both open the menu and collapse the rows it is about to change.
    // Collapsing is a view, so it is not gated on `usable` either.
    let toggle = ui.interact(
        Rect::from_min_max(rect.min, Pos2::new(cells[0].right(), rect.bottom())),
        Id::new(("compose_group_toggle", group.group)),
        Sense::click(),
    );
    if toggle.clicked() {
        actions.push(Action::ToggleGroup(group.group));
    }

    // ▾ invites the click that opens the group, ▴ the one that closes it
    // again. There is no triangle pointing sideways in the icon set, and
    // a chevron is the field's, not a section's.
    super::icons::triangle(
        ui.painter(),
        Pos2::new(cells[0].left() + 5.0, cells[0].center().y),
        9.0,
        color::TEXT_MUTED,
        group.open,
    );

    let name = summary::group_name(group.group);
    let mut cursor = cells[0].left() + 17.0;
    let name_galley = widgets::truncated(
        ui,
        &name,
        medium(12.5),
        color::TEXT_STRONG,
        (cells[0].right() - cursor - 110.0).max(60.0),
    );
    let name_width = name_galley.size().x;
    ui.painter().galley(
        Pos2::new(cursor, cells[0].center().y - name_galley.size().y / 2.0),
        name_galley,
        color::TEXT_STRONG,
    );
    cursor += name_width + 9.0;

    // A bare numeral, not a sentence: how many parts the group holds
    // needs no catalogue entry and no translation.
    let total = ui.painter().text(
        Pos2::new(cursor, cells[0].center().y),
        Align2::LEFT_CENTER,
        group.total.to_string(),
        mono(11.0),
        color::TEXT_FAINT,
    );
    cursor = total.right() + 10.0;

    if group.replaced > 0 {
        let badges = Rect::from_min_max(Pos2::new(cursor, cells[0].top()), cells[0].max);
        if badges.width() > 70.0 {
            let mut badge_ui = ui.new_child(
                UiBuilder::new().max_rect(badges).layout(Layout::left_to_right(Align::Center)),
            );
            widgets::badge(
                &mut badge_ui,
                None,
                &t!("gui.compose.group_replaced", replaced = group.replaced),
                color::ACCENT,
                color::ACCENT_SOFT,
            );
        }
    }

    let (label, created_at) = match &group.source {
        GroupSource::Base => (t!("gui.compose.from_base"), app.compose.base.clone()),
        GroupSource::One(created_at) => (human_time(created_at), Some(created_at.clone())),
        // Several sources at once, so none of them is the one a read
        // could be about — the field simply says so.
        GroupSource::Mixed => (t!("gui.compose.group_mixed"), None),
    };
    let field = source_field(
        app,
        ui,
        cells[1],
        Id::new(("compose_group_field", group.group)),
        &label,
        created_at.as_deref(),
        usable,
    );
    if field.clicked() {
        actions.push(Action::OpenComposePicker(Picker::Group(group.group)));
    }
    if matches!(&app.compose.picker, Some(Picker::Group(open)) if *open == group.group) {
        *anchor = Some(field.rect);
    }
}

fn part_row(
    app: &App,
    ui: &mut Ui,
    part: &PartRow,
    usable: bool,
    actions: &mut Vec<Action>,
    anchor: &mut Option<Rect>,
) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), PART_ROW_HEIGHT), Sense::hover());
    if response.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, color::HOVER);
    }
    ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, color::BORDER_ROW));

    let cells = widgets::columns(
        rect.shrink2(Vec2::new(metric::CARD_PADDING, 0.0)),
        &COMPOSE_COLUMNS,
        metric::COLUMN_GAP,
    );
    let indent = if part.indented { 22.0 } else { 0.0 };
    let name =
        Rect::from_min_max(Pos2::new(cells[0].left() + indent, cells[0].top()), cells[0].max);
    let upper = name.center().y - 10.0;
    let lower = name.center().y + 10.0;

    // A part whose id is its group's own is one of the whole-file
    // groups, and its id reads `tutorial`. With no header above the row
    // to name it, the group's own name is what the reader needs.
    let title = if part.id == part.group {
        summary::group_name(part.group)
    } else {
        part.id.clone()
    };
    let title_galley =
        widgets::truncated(ui, &title, sans(12.5), color::TEXT_STRONG, name.width() - 130.0);
    let title_width = title_galley.size().x;
    ui.painter().galley(
        Pos2::new(name.left(), upper - title_galley.size().y / 2.0),
        title_galley,
        color::TEXT_STRONG,
    );

    let older = app.compose.is_older(part);
    if part.source.is_some() || older {
        let badges = Rect::from_min_max(
            Pos2::new(name.left() + title_width + 9.0, upper - 9.0),
            Pos2::new(name.right(), upper + 9.0),
        );
        if badges.width() > 70.0 {
            let mut badge_ui = ui.new_child(
                UiBuilder::new().max_rect(badges).layout(Layout::left_to_right(Align::Center)),
            );
            badge_ui.spacing_mut().item_spacing.x = 6.0;
            if part.source.is_some() {
                widgets::badge(
                    &mut badge_ui,
                    None,
                    &t!("gui.compose.replaced_badge"),
                    color::ACCENT,
                    color::ACCENT_SOFT,
                );
            }
            if older {
                widgets::badge(
                    &mut badge_ui,
                    Some(Icon::Warning),
                    &t!("gui.compose.older_badge"),
                    color::WARN,
                    color::WARN_BG,
                );
            }
        }
    }

    let file_rect =
        Rect::from_min_max(Pos2::new(name.left(), lower - 8.0), Pos2::new(name.right(), lower + 8.0));
    widgets::column_text(
        ui,
        file_rect,
        part.file,
        mono(11.0),
        color::TEXT_FAINT,
    );

    let label = match &part.source {
        Some(created_at) => human_time(created_at),
        None => t!("gui.compose.from_base"),
    };
    let created_at = part.source.clone().or_else(|| app.compose.base.clone());
    let field = source_field(
        app,
        ui,
        cells[1],
        Id::new(("compose_part_field", &part.id)),
        &label,
        created_at.as_deref(),
        usable,
    );
    if field.clicked() {
        actions.push(Action::OpenComposePicker(Picker::Part(part.id.clone())));
    }
    if matches!(&app.compose.picker, Some(Picker::Part(open)) if *open == part.id) {
        *anchor = Some(field.rect);
    }

    if let Some(value) = part_value(app, part) {
        widgets::column_text(ui, cells[2], &value, mono(11.5), color::TEXT_DIM2);
    }

    if part.source.is_some() {
        let hit = Rect::from_center_size(cells[3].center(), Vec2::splat(22.0));
        let sense = if usable { Sense::click() } else { Sense::hover() };
        let reset = ui.interact(hit, Id::new(("compose_reset", &part.id)), sense);
        let tint = if usable && reset.hovered() { color::TEXT_STRONG } else { color::TEXT_FAINT };
        super::icons::cross(ui.painter(), hit.center(), 11.0, tint);
        if reset.clicked() {
            actions.push(Action::ResetPart(part.id.clone()));
        }
    }
}

/// The figure in the value column, for the three groups that have one.
///
/// It has to be read out of the backup the part comes from, not out of
/// the base: a class taken from another backup is shown at that
/// backup's level. A source that has not been read yet has none yet.
fn part_value(app: &App, part: &PartRow) -> Option<String> {
    let created_at = part.source.as_deref().or(app.compose.base.as_deref())?;
    let documents = app.compose.decoded.get(created_at)?;
    let catalogued = app.compose.parts.iter().find(|entry| entry.id == part.id)?;
    summary::summarize(catalogued, documents)
}

/// One row's dropdown field, drawn like the launch combo in the top bar
/// and at the scale of a table cell.
///
/// `created_at` is the backup the field names. While exactly that one is
/// being read the field says so and takes no click: there is nothing to
/// pick from until it has arrived.
fn source_field(
    app: &App,
    ui: &Ui,
    cell: Rect,
    id: Id,
    label: &str,
    created_at: Option<&str>,
    enabled: bool,
) -> egui::Response {
    let loading = created_at.is_some() && app.compose.loading.as_deref() == created_at;
    let enabled = enabled && !loading;
    let text = if loading { t!("gui.compose.loading") } else { label.to_owned() };

    let rect = Rect::from_min_size(
        Pos2::new(cell.left(), cell.center().y - FIELD_HEIGHT / 2.0),
        Vec2::new(cell.width(), FIELD_HEIGHT),
    );
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let response = ui.interact(rect, id, sense);
    let hovered = enabled && response.hovered();

    let (background, border, foreground) = if enabled {
        (
            color::CONTROL,
            if hovered { color::ACCENT } else { color::BORDER },
            if hovered { color::TEXT_STRONG } else { color::TEXT },
        )
    } else {
        (color::CONTROL_DISABLED, color::BORDER_DISABLED, color::TEXT_FAINT)
    };

    let painter = ui.painter();
    let radius = CornerRadius::same(6);
    painter.rect_filled(rect, radius, background);
    painter.rect_stroke(rect, radius, Stroke::new(1.0, border), StrokeKind::Inside);

    let galley = widgets::truncated(ui, &text, sans(11.5), foreground, rect.width() - 34.0);
    painter.galley(
        Pos2::new(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0),
        galley,
        foreground,
    );
    Icon::ChevronDown.paint(
        painter,
        Pos2::new(rect.right() - 14.5, rect.center().y),
        9.0,
        if enabled { color::TEXT_MUTED } else { color::TEXT_FAINT },
    );

    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// The dropdown itself, over everything else, at the field that opened
/// it.
fn picker_menu(app: &App, ui: &Ui, anchor: Rect, actions: &mut Vec<Action>) {
    let Some(picker) = &app.compose.picker else { return };

    let width = 320.0_f32.max(anchor.width());
    let needle = app.compose.picker_filter.trim().to_lowercase();
    let matching: Vec<&sm2_core::saves::BackupEntry> = app
        .backups
        .iter()
        .filter(|entry| {
            needle.is_empty()
                || human_time(&entry.created_at).to_lowercase().contains(&needle)
                || entry.label.as_deref().unwrap_or_default().to_lowercase().contains(&needle)
        })
        .collect();

    let area = egui::Area::new(Id::new("compose_picker"))
        .order(egui::Order::Foreground)
        .fixed_pos(Pos2::new(anchor.left(), anchor.bottom() + 4.0))
        // A field near the lower edge would otherwise hang its menu off
        // the window, and the last rows of the list are the ones nobody
        // could reach.
        .constrain(true)
        .show(ui.ctx(), |ui| {
            egui::Frame::new()
                .fill(color::MENU)
                .stroke(Stroke::new(1.0, color::BORDER))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(egui::Margin::same(6))
                .show(ui, |ui| {
                    ui.set_width(width);
                    ui.spacing_mut().item_spacing.y = 1.0;

                    let mut filter = app.compose.picker_filter.clone();
                    let placeholder = t!("gui.compose.picker_placeholder");
                    if widgets::text_field(ui, &mut filter, &placeholder, width, Some(Icon::Search))
                        .changed()
                    {
                        actions.push(Action::SetPickerFilter(filter));
                    }
                    ui.add_space(4.0);

                    // The base heads a part's and a group's menu and the
                    // filter never takes it away: picking it is how a
                    // part goes back to the base from inside the
                    // dropdown.
                    if let Some(base) = app.compose.base.clone() {
                        if !matches!(picker, Picker::Base)
                            && widgets::menu_item(ui, None, &t!("gui.compose.from_base"), true, false)
                        {
                            actions.push(pick(picker, base));
                        }
                    }

                    if matching.is_empty() {
                        ui.label(
                            RichText::new(t!("gui.compose.picker_empty"))
                                .font(sans(12.0))
                                .color(color::TEXT_DIM2),
                        );
                        return;
                    }
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        for entry in &matching {
                            let time = human_time(&entry.created_at);
                            let label = match entry.label.as_deref() {
                                Some(label) if !label.is_empty() => format!("{time} · {label}"),
                                _ => time,
                            };
                            if widgets::menu_item(ui, None, &label, true, false) {
                                actions.push(pick(picker, entry.created_at.clone()));
                            }
                        }
                    });
                });
        });

    // A click anywhere else closes the menu again. The field itself is
    // excluded: it toggles the menu on its own, and counting the click
    // twice would reopen what it just closed.
    let pointer = ui.ctx().pointer_interact_pos().unwrap_or(Pos2::new(f32::MIN, f32::MIN));
    if ui.ctx().input(|i| i.pointer.any_click())
        && !area.response.rect.contains(pointer)
        && !anchor.contains(pointer)
    {
        actions.push(Action::CloseComposePicker);
    }
}

/// What a pick in the open menu means, which is the whole difference
/// between the three dropdowns.
fn pick(picker: &Picker, backup: String) -> Action {
    match picker {
        Picker::Base => Action::PickComposeBase(backup),
        Picker::Part(part) => Action::PickPartSource { part: part.clone(), backup },
        Picker::Group(group) => Action::PickGroupSource { group, backup },
    }
}

fn warning_line(ui: &Ui, rect: Rect, text: &str) {
    super::icons::warning(
        ui.painter(),
        Pos2::new(rect.left() + 5.5, rect.center().y),
        11.0,
        color::WARN,
    );
    widgets::column_text(
        ui,
        Rect::from_min_max(Pos2::new(rect.left() + 20.0, rect.top()), rect.max),
        text,
        sans(11.5),
        color::WARN,
    );
}

/// What has been chosen, the label of the result, and the button that
/// writes it.
fn footer(app: &App, ui: &mut Ui, rect: Rect, usable: bool, actions: &mut Vec<Action>) {
    ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.label(
                RichText::new(app.compose.summary()).font(sans(12.0)).color(color::TEXT_DIM2),
            );

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::button(
                    ui,
                    &ButtonStyle::primary(),
                    Some(Icon::Plus),
                    &t!("gui.compose.create_button"),
                    usable && app.compose.base.is_some(),
                )
                .clicked()
                {
                    actions.push(Action::Compose);
                }
                let mut label = app.compose.label.clone();
                let placeholder = t!("gui.compose.label_placeholder");
                if widgets::text_field(ui, &mut label, &placeholder, 230.0, None).changed() {
                    actions.push(Action::SetComposeLabel(label));
                }
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use crate::app_state::language_test_lock;
    use sm2_core::i18n::{set_language, Language};
    use sm2_core::t;

    /// The tab strip and the explanatory line under it, in both
    /// languages — the two pieces of copy that say what this tab is
    /// for, and the first thing a translator gets wrong.
    #[test]
    fn the_tab_introduces_itself_in_both_languages() {
        let _held = language_test_lock();
        set_language(Language::English);
        assert_eq!(t!("gui.compose.tab"), "Compose a save");
        assert!(t!("gui.compose.intro").contains("new backup"));
        set_language(Language::German);
        assert_eq!(t!("gui.compose.tab"), "Save zusammenstellen");
        assert!(t!("gui.compose.intro").contains("neues Backup"));
        set_language(Language::English);
    }
}

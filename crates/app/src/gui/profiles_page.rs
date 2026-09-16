//! The "Profile" section.

use super::format::human_time;
use super::theme::{color, medium, metric, mono, sans};
use super::widgets::{self, ButtonStyle, Column};
use super::{Action, App};
use crate::vanilla::VANILLA_SNAPSHOT_PREFIX;
use egui::{Align2, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, UiBuilder, Vec2};
use sm2_core::t;

const PROFILE_COLUMNS: [Column; 4] = [
    Column::Flexible,
    Column::Fixed(120.0),
    Column::Fixed(140.0),
    Column::Fixed(176.0),
];

pub fn show(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    super::page_heading(
        ui,
        &t!("gui.profiles.heading"),
        &t!("gui.profiles.heading_body"),
        660.0,
    );

    let outer = ui.available_rect_before_wrap();
    super::draw_card(ui, outer);

    let toolbar =
        Rect::from_min_size(outer.min, Vec2::new(outer.width(), metric::CARD_TOOLBAR_HEIGHT));
    ui.scope_builder(
        UiBuilder::new().max_rect(toolbar.shrink2(Vec2::new(metric::CARD_PADDING, 0.0))),
        |ui| {
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let mut name = app.profile_name.clone();
                let name_placeholder = t!("gui.profiles.name_placeholder");
                if widgets::text_field(ui, &mut name, &name_placeholder, 250.0, None).changed() {
                    actions.push(Action::SetProfileName(name));
                }
                let can_save = app.state.is_some();
                if widgets::button(
                    ui,
                    &ButtonStyle::ghost().font(medium(12.5)),
                    None,
                    &t!("gui.profiles.save_button"),
                    can_save,
                )
                .clicked()
                {
                    actions.push(Action::SaveProfile);
                }
            });
        },
    );
    let head = Rect::from_min_size(
        Pos2::new(outer.left(), toolbar.bottom() + 1.0),
        Vec2::new(outer.width(), metric::TABLE_HEAD_HEIGHT),
    );
    let col_name = t!("gui.profiles.col_name");
    let col_active = t!("gui.profiles.col_active");
    let col_saved = t!("gui.profiles.col_saved");
    super::draw_column_head(
        ui,
        head,
        &PROFILE_COLUMNS,
        &[col_name.as_str(), col_active.as_str(), col_saved.as_str(), ""],
    );
    ui.painter().hline(outer.x_range(), toolbar.bottom() + 0.5, Stroke::new(1.0, color::BORDER_SOFT));

    let body = Rect::from_min_max(
        Pos2::new(outer.left(), head.bottom()),
        Pos2::new(outer.right(), outer.bottom()),
    );
    ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
        if app.profiles.is_empty() {
            super::empty_hint(ui, &t!("gui.profiles.empty"));
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for profile in &app.profiles {
                row(app, ui, profile, actions);
            }
        });
    });
}

fn row(app: &App, ui: &mut Ui, profile: &sm2_core::profile::Profile, actions: &mut Vec<Action>) {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, metric::LIST_ROW_HEIGHT), Sense::hover());
    if response.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, color::HOVER);
    }
    ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, color::BORDER_ROW));

    let cells = widgets::columns(
        rect.shrink2(Vec2::new(metric::CARD_PADDING, 0.0)),
        &PROFILE_COLUMNS,
        metric::COLUMN_GAP,
    );

    // The name, followed by the marker for automatically made snapshots.
    let automatic = profile.name.starts_with(VANILLA_SNAPSHOT_PREFIX);
    let name_galley = widgets::truncated(
        ui,
        &profile.name,
        medium(12.5),
        color::TEXT_STRONG,
        cells[0].width() - 100.0,
    );
    let name_width = name_galley.size().x;
    ui.painter().galley(
        Pos2::new(cells[0].left(), cells[0].center().y - name_galley.size().y / 2.0),
        name_galley,
        color::TEXT_STRONG,
    );
    if automatic {
        let badge_rect = Rect::from_min_max(
            Pos2::new(cells[0].left() + name_width + 9.0, cells[0].top()),
            cells[0].max,
        );
        if badge_rect.width() > 90.0 {
            let mut badge_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(badge_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            widgets::badge(
                &mut badge_ui,
                None,
                &t!("gui.profiles.automatic_badge"),
                color::INFO,
                color::INFO_BG,
            );
        }
    }

    let active = profile.entries.iter().filter(|e| !e.disabled).count();
    ui.painter().text(
        Pos2::new(cells[1].left(), cells[1].center().y),
        Align2::LEFT_CENTER,
        t!("gui.profiles.active_count", active = active, total = profile.entries.len()),
        mono(11.5),
        color::TEXT_DIM2,
    );

    ui.painter().text(
        Pos2::new(cells[2].left(), cells[2].center().y),
        Align2::LEFT_CENTER,
        saved_at(app, profile),
        mono(11.5),
        color::TEXT_DIM2,
    );

    // Buttons aligned to the right.
    let mut buttons = ui.new_child(
        UiBuilder::new()
            .max_rect(cells[3])
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    buttons.spacing_mut().item_spacing.x = 7.0;
    let delete = ButtonStyle::ghost()
        .small()
        .font(sans(11.5));
    let mut delete = delete;
    delete.foreground = color::TEXT_DIM2;
    delete.border_hovered = Some(color::DANGER_BORDER);
    delete.foreground_hovered = color::DANGER;
    if widgets::button(&mut buttons, &delete, None, &t!("gui.profiles.delete_button"), true)
        .clicked()
    {
        actions.push(Action::AskDeleteProfile(profile.name.clone()));
    }
    if widgets::button(
        &mut buttons,
        &ButtonStyle::ghost().small(),
        None,
        &t!("gui.profiles.apply_button"),
        app.can_modify(),
    )
    .clicked()
    {
        actions.push(Action::ApplyProfile(profile.name.clone()));
    }
}

/// When the profile was last written. A profile carries no timestamp of its
/// own — the file does, and that is exactly what is meant here.
fn saved_at(app: &App, profile: &sm2_core::profile::Profile) -> String {
    let Some(dir) = app.profiles_dir() else { return t!("gui.profiles.no_value") };
    let path = profile.path_in(&dir);
    let Ok(metadata) = std::fs::metadata(&path) else { return t!("gui.profiles.no_value") };
    let Ok(modified) = metadata.modified() else { return t!("gui.profiles.no_value") };
    let Ok(since_epoch) = modified.duration_since(std::time::UNIX_EPOCH) else {
        return t!("gui.profiles.no_value");
    };
    human_time(&sm2_core::import::format_utc(since_epoch.as_secs() as i64))
}

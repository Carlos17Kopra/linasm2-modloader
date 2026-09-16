//! Transient feedback for actions: the short message that fades in over
//! the lower right corner after a backup, an import or a profile change.
//!
//! The toasts are fed by `set_status`/`set_warning`, so every action that
//! already reports something gets one without a call site of its own. The
//! status bar keeps showing the same message afterwards — a toast that was
//! missed is therefore never the only trace of it.
//!
//! The whole bookkeeping below takes the current time as a parameter
//! (`egui`'s seconds since the program started) instead of reading a clock
//! itself: that keeps it testable without a window.

use super::theme::{color, metric, sans};
use super::{icons, Action, App};
use egui::{Pos2, Rect, Sense, Ui, Vec2};

/// How long a success message stays before it fades out by itself.
const SUCCESS_LIFETIME: f64 = 4.0;

/// The fade at the end of that lifetime.
const FADE: f64 = 0.5;

/// At most this many toasts at once; a further one pushes the oldest out.
/// Without the cap a burst of messages (an import reports per file) would
/// cover the window up to the top edge.
const MAX_VISIBLE: usize = 4;

const WIDTH: f32 = 340.0;
const PADDING: f32 = 12.0;
/// The space in front of the text that the icon and its margin occupy.
const ICON_SPACE: f32 = 26.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Warning,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub id: u64,
    pub kind: ToastKind,
    pub text: String,
    born: f64,
}

impl Toast {
    /// Fully opaque for most of its life, then linearly down to zero over
    /// the last `FADE` seconds. A warning never fades: it is the one
    /// message that must not disappear unseen, so it waits for a click.
    pub fn opacity(&self, now: f64) -> f32 {
        if self.kind == ToastKind::Warning {
            return 1.0;
        }
        let remaining = SUCCESS_LIFETIME - (now - self.born);
        if remaining >= FADE { 1.0 } else { (remaining / FADE).clamp(0.0, 1.0) as f32 }
    }
}

#[derive(Default)]
pub struct Toasts {
    items: Vec<Toast>,
    next_id: u64,
}

impl Toasts {
    /// Adds a message and returns its id. The same text that is still on
    /// screen only gets its timer restarted — a button pressed twice
    /// should not produce two identical tiles.
    pub fn push(&mut self, text: impl Into<String>, kind: ToastKind, now: f64) -> u64 {
        let text = text.into();
        if let Some(existing) = self.items.iter_mut().find(|t| t.kind == kind && t.text == text) {
            existing.born = now;
            return existing.id;
        }

        self.next_id += 1;
        let id = self.next_id;
        self.items.push(Toast { id, kind, text, born: now });
        if self.items.len() > MAX_VISIBLE {
            self.items.remove(0);
        }
        id
    }

    /// Drops everything that has run out. Called once per frame.
    pub fn prune(&mut self, now: f64) {
        self.items.retain(|toast| toast.opacity(now) > 0.0);
    }

    /// Removes a toast by id. An id that no longer exists is deliberately
    /// no error: the timer can remove a toast between the frame that drew
    /// it and the click being handled.
    pub fn dismiss(&mut self, id: u64) {
        self.items.retain(|toast| toast.id != id);
    }

    pub fn visible(&self) -> &[Toast] {
        &self.items
    }

    /// Seconds until something on screen changes, or `None` while nothing
    /// runs on a timer. Without this `egui` would go to sleep after the
    /// last input and a toast that has run out would stay on screen until
    /// the user moves the mouse.
    pub fn next_repaint(&self, now: f64) -> Option<f64> {
        self.items
            .iter()
            .filter(|toast| toast.kind == ToastKind::Success)
            // While a toast is fading, every frame looks different: 0.0
            // asks for the next one right away.
            .map(|toast| (toast.born + SUCCESS_LIFETIME - FADE - now).max(0.0))
            .min_by(f64::total_cmp)
    }
}

/// Draws the toasts above the status bar in the lower right corner and
/// collects the clicks that dismiss them.
///
/// Runs before `dialogs::show` so that an open dialog and its veil lie
/// over the toasts — a modal question must not compete for attention with
/// a message about something already done.
pub fn show(app: &App, ctx: &egui::Context, actions: &mut Vec<Action>) {
    if app.toasts.visible().is_empty() {
        return;
    }
    let now = ctx.input(|input| input.time);
    if let Some(delay) = app.toasts.next_repaint(now) {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(delay));
    }

    egui::Area::new(egui::Id::new("toasts"))
        .anchor(
            egui::Align2::RIGHT_BOTTOM,
            Vec2::new(-metric::CONTENT_PADDING, -(metric::STATUS_BAR_HEIGHT + 12.0)),
        )
        .order(egui::Order::Foreground)
        .interactable(true)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            for toast in app.toasts.visible() {
                draw(ui, toast, now, actions);
            }
        });
}

fn draw(ui: &mut Ui, toast: &Toast, now: f64, actions: &mut Vec<Action>) {
    let alpha = toast.opacity(now);
    let accent = match toast.kind {
        ToastKind::Success => color::ACCENT,
        ToastKind::Warning => color::WARN,
    };

    let text_width = WIDTH - PADDING * 2.0 - ICON_SPACE;
    let galley =
        ui.painter().layout(toast.text.clone(), sans(12.0), color::TEXT.gamma_multiply(alpha), text_width);
    let height = (galley.size().y + PADDING * 2.0).max(42.0);

    let (rect, response) = ui.allocate_exact_size(Vec2::new(WIDTH, height), Sense::click());
    if response.clicked() {
        actions.push(Action::DismissToast(toast.id));
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let painter = ui.painter();
    let radius = egui::CornerRadius::same(8);
    painter.rect_filled(rect, radius, color::CARD.gamma_multiply(alpha));
    painter.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.0, color::BORDER_SOFT.gamma_multiply(alpha)),
        egui::StrokeKind::Inside,
    );
    // The coloured edge on the left is the same signal the blocked banner
    // on the savegames page uses.
    painter.rect_filled(
        Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())),
        egui::CornerRadius { nw: 8, sw: 8, ne: 0, se: 0 },
        accent.gamma_multiply(alpha),
    );

    let icon_center = Pos2::new(rect.left() + 3.0 + ICON_SPACE / 2.0, rect.center().y);
    match toast.kind {
        ToastKind::Success => icons::check(painter, icon_center, 11.0, accent.gamma_multiply(alpha)),
        ToastKind::Warning => icons::warning(painter, icon_center, 11.0, accent.gamma_multiply(alpha)),
    }

    painter.galley(
        Pos2::new(rect.left() + PADDING + ICON_SPACE, rect.center().y - galley.size().y / 2.0),
        galley,
        color::TEXT.gamma_multiply(alpha),
    );
}


#[cfg(test)]
mod tests {
    use super::*;

    fn success(toasts: &mut Toasts, text: &str, now: f64) -> u64 {
        toasts.push(text, ToastKind::Success, now)
    }

    #[test]
    fn a_success_toast_disappears_after_its_lifetime() {
        let mut toasts = Toasts::default();
        success(&mut toasts, "Backup angelegt", 0.0);

        toasts.prune(SUCCESS_LIFETIME - 0.1);
        assert_eq!(toasts.visible().len(), 1);

        toasts.prune(SUCCESS_LIFETIME + 0.1);
        assert!(toasts.visible().is_empty());
    }

    /// A warning is the one message that must not scroll past unseen —
    /// it waits for a click, however long that takes.
    #[test]
    fn a_warning_stays_until_it_is_dismissed() {
        let mut toasts = Toasts::default();
        let id = toasts.push("Import fehlgeschlagen", ToastKind::Warning, 0.0);

        toasts.prune(600.0);
        assert_eq!(toasts.visible().len(), 1);

        toasts.dismiss(id);
        assert!(toasts.visible().is_empty());
    }

    /// Clicking the same button twice must not stack two identical tiles;
    /// the second message only restarts the first one's timer.
    #[test]
    fn the_same_message_refreshes_instead_of_stacking() {
        let mut toasts = Toasts::default();
        success(&mut toasts, "Profil gespeichert", 0.0);
        success(&mut toasts, "Profil gespeichert", 3.0);

        assert_eq!(toasts.visible().len(), 1);
        toasts.prune(3.0 + SUCCESS_LIFETIME - 0.1);
        assert_eq!(toasts.visible().len(), 1, "the timer starts again with the second message");
        toasts.prune(3.0 + SUCCESS_LIFETIME + 0.1);
        assert!(toasts.visible().is_empty());
    }

    #[test]
    fn only_the_newest_toasts_stay() {
        let mut toasts = Toasts::default();
        for i in 0..MAX_VISIBLE + 1 {
            success(&mut toasts, &format!("Meldung {i}"), 0.0);
        }

        let texts: Vec<&str> = toasts.visible().iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts.len(), MAX_VISIBLE);
        assert_eq!(texts[0], "Meldung 1", "the oldest message gives way");
    }

    /// The timer can remove a toast between drawing it and handling the
    /// click on it — the click then refers to an id that is already gone.
    #[test]
    fn dismissing_an_unknown_id_changes_nothing() {
        let mut toasts = Toasts::default();
        let id = success(&mut toasts, "Backup angelegt", 0.0);

        toasts.dismiss(id + 1);

        assert_eq!(toasts.visible().len(), 1);
    }

    #[test]
    fn a_success_toast_fades_out_towards_its_end() {
        let mut toasts = Toasts::default();
        success(&mut toasts, "Backup angelegt", 0.0);
        let toast = toasts.visible()[0].clone();

        assert_eq!(toast.opacity(0.0), 1.0);
        assert_eq!(toast.opacity(SUCCESS_LIFETIME - FADE), 1.0);
        let fading = toast.opacity(SUCCESS_LIFETIME - FADE / 2.0);
        assert!((0.1..0.9).contains(&fading), "{fading}");
    }

    #[test]
    fn a_warning_never_fades() {
        let mut toasts = Toasts::default();
        toasts.push("Steam läuft", ToastKind::Warning, 0.0);

        assert_eq!(toasts.visible()[0].opacity(600.0), 1.0);
    }

    /// Without a scheduled repaint egui goes to sleep and a toast that has
    /// run out stays on screen until the user moves the mouse.
    #[test]
    fn a_running_toast_asks_for_a_repaint_and_a_warning_alone_does_not() {
        let mut toasts = Toasts::default();
        toasts.push("Steam läuft", ToastKind::Warning, 0.0);
        assert_eq!(toasts.next_repaint(0.0), None);

        success(&mut toasts, "Backup angelegt", 0.0);
        let delay = toasts.next_repaint(0.0).expect("a success toast runs out on its own");
        assert!(delay > 0.0 && delay <= SUCCESS_LIFETIME, "{delay}");
    }
}

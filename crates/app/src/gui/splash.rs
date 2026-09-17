//! The start-up screen: the product's name while the first load runs.
//!
//! It exists because `App::load` blocks — Steam detection walks the
//! library folders, the mod library is read and hashed, the backup list is
//! scanned. On a cold cache that is long enough for the window to stand
//! there empty, and on a machine where Steam is not where it is expected
//! it is longer still.
//!
//! The timing is the delicate part. `egui` hands a frame to the
//! compositor only once `ui()` has returned, so a load started on the same
//! frame that first draws this screen would keep that drawing from ever
//! reaching the display — the user would see the empty window, then the
//! finished interface, and never the screen in between. Hence
//! `Step::Wait`: the first frame does nothing but draw, and only the
//! second one loads. `Step::Finish` then waits out `MIN_SECONDS` so that a
//! load taking twenty milliseconds does not turn the screen into a flicker.

use super::theme::{color, medium, sans};
use egui::{Align2, CornerRadius, Pos2, Rect, Sense, Ui, Vec2};
use sm2_core::t;

/// How long the screen stays up at the very least, in seconds.
const MIN_SECONDS: f64 = 1.1;

/// Width and height of the indeterminate bar under the name.
const BAR_SIZE: Vec2 = Vec2::new(180.0, 3.0);

/// How much of that bar the travelling segment covers.
const BAR_SEGMENT: f32 = 0.34;

/// Seconds the travelling segment needs for one way across the bar.
const BAR_SECONDS: f64 = 1.1;

/// The state of the start-up screen, from the first frame until it is
/// replaced by the interface.
#[derive(Debug, Default)]
pub struct Splash {
    /// `input.time` of the first frame that drew it — the clock
    /// `MIN_SECONDS` is measured against. `None` until that frame.
    since: Option<f64>,
    /// Has it been drawn at least once? See the module comment: the load
    /// waits for this.
    painted: bool,
    /// Has `Step::Load` been handed out? Recorded here rather than left to
    /// the caller: `load()` blocks, so in practice `mark_loaded` follows
    /// on the same frame — but a caller that ever gets to draw between the
    /// two must not be told to load a second time, on top of a detection
    /// that is already running.
    load_started: bool,
    /// Is the load behind us?
    loaded: bool,
}

/// What the caller has to do this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Draw the screen, change nothing else.
    Wait,
    /// Draw the screen, then load.
    Load,
    /// Do not draw it any more — the interface takes over this frame.
    Finish,
}

impl Splash {
    /// Decides what this frame does, and records that the screen was
    /// drawn. Called once per frame, before the drawing.
    ///
    /// Kept apart from `show` so the order it enforces — draw first, load
    /// on the next frame, and not a millisecond under `MIN_SECONDS` — can
    /// be tested without a window.
    pub fn step(&mut self, now: f64) -> Step {
        let since = *self.since.get_or_insert(now);

        if self.loaded && now - since >= MIN_SECONDS {
            return Step::Finish;
        }

        let step = if self.painted && !self.load_started {
            self.load_started = true;
            Step::Load
        } else {
            Step::Wait
        };
        self.painted = true;
        step
    }

    /// Records that the load is behind us. From here on only
    /// `MIN_SECONDS` stands between the screen and the interface.
    pub fn mark_loaded(&mut self) {
        self.loaded = true;
    }
}

/// Draws the screen over the whole window: the name in the middle, the
/// word "loading" under it, and a bar that keeps moving so a load that
/// takes a while does not look like a hung window.
pub fn show(ui: &mut Ui, now: f64) {
    let area = ui.available_rect_before_wrap();
    let painter = ui.painter();
    painter.rect_filled(area, CornerRadius::ZERO, color::WINDOW);

    // Slightly above the middle: text centred in an optically empty area
    // sits low, because the eye takes the block's centre of gravity, not
    // its bounding box, for the middle.
    let centre = Pos2::new(area.center().x, area.center().y - 18.0);

    painter.text(
        centre,
        Align2::CENTER_CENTER,
        sm2_core::APP_NAME_SHORT,
        medium(34.0),
        color::TEXT_STRONG,
    );
    painter.text(
        Pos2::new(centre.x, centre.y + 30.0),
        Align2::CENTER_CENTER,
        sm2_core::APP_SUBTITLE,
        sans(13.0),
        color::ACCENT,
    );
    painter.text(
        Pos2::new(centre.x, centre.y + 84.0),
        Align2::CENTER_CENTER,
        t!("gui.splash.loading"),
        sans(11.5),
        color::TEXT_FAINT,
    );

    let bar = Rect::from_center_size(Pos2::new(centre.x, centre.y + 62.0), BAR_SIZE);
    indeterminate_bar(ui, bar, now);

    // Swallows the clicks of an impatient user: without this the panel
    // underneath would collect them and hand them to whatever ends up in
    // that spot once the interface appears.
    ui.allocate_rect(area, Sense::click());
}

/// A bar without a percentage: nothing here knows how far along the load
/// is, and a progress bar that invents one would be a lie. The segment
/// travels back and forth instead — movement is the whole message.
fn indeterminate_bar(ui: &Ui, bar: Rect, now: f64) {
    let radius = CornerRadius::same(2);
    let painter = ui.painter();
    painter.rect_filled(bar, radius, color::CONTROL);

    // A triangle wave over `now`: 0 → 1 → 0. `abs` on the sawtooth is what
    // turns the jump back to the start into a turn, so the segment glides
    // rather than snapping back to the left edge.
    let phase = ((now / BAR_SECONDS) % 2.0) as f32;
    let travel = (phase - 1.0).abs();

    let width = bar.width() * BAR_SEGMENT;
    let left = bar.left() + (bar.width() - width) * (1.0 - travel);
    painter.rect_filled(
        Rect::from_min_size(Pos2::new(left, bar.top()), Vec2::new(width, bar.height())),
        radius,
        color::ACCENT,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first frame must not load. See the module comment: a load
    /// started before the first frame has been handed over keeps the
    /// screen from ever being seen.
    #[test]
    fn the_first_frame_only_draws() {
        let mut splash = Splash::default();
        assert_eq!(splash.step(0.0), Step::Wait);
    }

    #[test]
    fn the_second_frame_starts_the_load() {
        let mut splash = Splash::default();
        splash.step(0.0);
        assert_eq!(splash.step(0.016), Step::Load);
    }

    /// A fast load must not turn the screen into a flicker: until
    /// `MIN_SECONDS` have passed the answer stays `Wait`, however quickly
    /// the load came back.
    #[test]
    fn a_fast_load_still_leaves_the_screen_up_for_the_minimum_time() {
        let mut splash = Splash::default();
        splash.step(0.0);
        splash.step(0.016);
        splash.mark_loaded();

        assert_eq!(splash.step(0.02), Step::Wait);
        assert_eq!(splash.step(MIN_SECONDS - 0.001), Step::Wait);
        assert_eq!(splash.step(MIN_SECONDS), Step::Finish);
    }

    /// The other direction: a load taking longer than the minimum is not
    /// cut short. The screen goes only once the load is actually done.
    #[test]
    fn a_slow_load_is_not_cut_short_by_the_minimum_time() {
        let mut splash = Splash::default();
        splash.step(0.0);
        assert_eq!(splash.step(0.016), Step::Load);

        assert_eq!(splash.step(MIN_SECONDS * 5.0), Step::Wait);

        splash.mark_loaded();
        assert_eq!(splash.step(MIN_SECONDS * 5.0), Step::Finish);
    }

    /// The load is started exactly once. A second `Load` would run the
    /// whole detection again, on top of a finished one.
    #[test]
    fn the_load_is_never_asked_for_twice() {
        let mut splash = Splash::default();
        splash.step(0.0);
        assert_eq!(splash.step(0.016), Step::Load);
        splash.mark_loaded();

        for frame in 2..40 {
            let step = splash.step(0.016 * f64::from(frame));
            assert_ne!(step, Step::Load, "frame {frame} asked for a second load");
        }
    }

    /// The clock starts with the first frame, not at `input.time` zero.
    /// `eframe` does not start counting at 0 — on a window opened later in
    /// the process's life the first frame can arrive at any time, and
    /// measuring against 0 would declare the screen over before it was
    /// ever drawn.
    #[test]
    fn the_minimum_time_is_measured_from_the_first_frame_not_from_zero() {
        let mut splash = Splash::default();
        let first = 420.0;

        splash.step(first);
        splash.step(first + 0.016);
        splash.mark_loaded();

        assert_eq!(splash.step(first + MIN_SECONDS - 0.001), Step::Wait);
        assert_eq!(splash.step(first + MIN_SECONDS), Step::Finish);
    }

    /// The travelling segment must stay inside the bar at every point of
    /// its cycle — including the two turning points, where a sawtooth
    /// without the `abs` would push it out over the right edge.
    #[test]
    fn the_travelling_segment_stays_within_the_bar() {
        let bar = Rect::from_min_size(Pos2::new(10.0, 0.0), BAR_SIZE);
        let width = bar.width() * BAR_SEGMENT;

        for frame in 0..400 {
            let now = f64::from(frame) * 0.01;
            let phase = ((now / BAR_SECONDS) % 2.0) as f32;
            let travel = (phase - 1.0).abs();
            let left = bar.left() + (bar.width() - width) * (1.0 - travel);

            assert!(left >= bar.left() - 0.001, "segment left of the bar at {now}");
            assert!(left + width <= bar.right() + 0.001, "segment right of the bar at {now}");
        }
    }
}

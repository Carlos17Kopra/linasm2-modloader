//! What the interface knows about updates.
//!
//! The check does **not** go through `tasks.rs`. That machinery locks
//! activation, ordering and import through `App::can_modify`, because a
//! job there works on copies of library and configuration and hands them
//! back at the end. A check changes nothing and must not freeze editing
//! for the second or two it takes — so it gets a channel of its own,
//! read once per frame.

use sm2_core::update::{self, Availability, CheckCache, Endpoints, Version};
use sm2_core::Result;
use std::sync::mpsc;

/// Who asked for the check that is running. It decides how loud its
/// answer may be: a check nobody pressed for runs once a day on its own,
/// and a launcher started without a network would otherwise put the same
/// warning on the screen every morning forever. A check someone pressed
/// for wants an answer either way — that is what pressing it was for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The once-a-day check on start.
    Automatic,
    /// "Check now", or the button in the dialog that asks the question.
    Requested,
}

#[derive(Default)]
pub struct UpdateUi {
    /// What the last check — or the cache from an earlier start — found.
    pub known: Option<Availability>,
    /// The running check, if there is one, together with who asked for
    /// it. The two live in one `Option` so that the origin cannot
    /// outlive — or fall behind — the check it belongs to.
    checking: Option<(Origin, mpsc::Receiver<Result<Availability>>)>,
}

impl UpdateUi {
    /// Is there a newer release to offer? `Ahead` and `UpToDate` are not
    /// news: one is a development build, the other is nothing to say.
    pub fn has_news(&self) -> bool {
        self.known.and_then(|found| found.newer()).is_some()
    }

    pub fn is_busy(&self) -> bool {
        self.checking.is_some()
    }

    /// Takes over what a previous start wrote down, so that the mark on
    /// the sidebar is there even when this start has no network. A
    /// cached version that is not newer, or not a version at all, says
    /// nothing and is dropped.
    pub fn adopt_cache(&mut self, cache: &CheckCache) {
        let current = Version::running();
        if let Some(latest) = cache.latest().filter(|latest| *latest > current) {
            self.known = Some(Availability::Newer { current, latest });
        }
    }

    /// Starts a check on a thread of its own. The context is woken when
    /// the answer arrives — without that the window would sit on the
    /// stale frame until the user moved the mouse.
    pub fn start_check(&mut self, ctx: &egui::Context, endpoints: Endpoints, origin: Origin) {
        if self.is_busy() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let outcome = update::check(&endpoints, update::NET_TIMEOUT);
            // An error here means the interface is gone; nothing to do.
            let _ = sender.send(outcome);
            ctx.request_repaint();
        });
        self.checking = Some((origin, receiver));
    }

    /// The answer, once, with the origin of the check that produced it.
    /// Returns `None` while the check is still running and on every frame
    /// after it has been handed over.
    pub fn poll(&mut self) -> Option<(Origin, Result<Availability>)> {
        let (origin, receiver) = self.checking.as_ref()?;
        let origin = *origin;
        let outcome = match receiver.try_recv() {
            Ok(outcome) => outcome,
            Err(mpsc::TryRecvError::Empty) => return None,
            // The thread died without sending. Treat it as finished
            // rather than waiting for an answer that cannot come.
            Err(mpsc::TryRecvError::Disconnected) => {
                self.checking = None;
                return None;
            }
        };
        self.checking = None;
        if let Ok(found) = &outcome {
            self.known = Some(*found);
        }
        Some((origin, outcome))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sm2_core::update::Version;

    #[test]
    fn knows_nothing_at_first() {
        let ui = UpdateUi::default();
        assert!(!ui.has_news());
        assert!(!ui.is_busy());
    }

    #[test]
    fn a_newer_version_is_news() {
        let ui = UpdateUi {
            known: Some(Availability::Newer {
                current: Version::new(0, 4, 0),
                latest: Version::new(0, 5, 0),
            }),
            ..Default::default()
        };
        assert!(ui.has_news());
    }

    /// A development build is newer than every release. Marking that as
    /// news would put a dot on the sidebar that offers a downgrade.
    #[test]
    fn being_ahead_is_not_news() {
        let ui = UpdateUi {
            known: Some(Availability::Ahead {
                current: Version::new(9, 9, 9),
                latest: Version::new(0, 5, 0),
            }),
            ..Default::default()
        };
        assert!(!ui.has_news());
    }

    #[test]
    fn being_up_to_date_is_not_news() {
        let ui = UpdateUi {
            known: Some(Availability::UpToDate { current: Version::new(0, 4, 0) }),
            ..Default::default()
        };
        assert!(!ui.has_news());
    }

    /// A cache from a previous start is what puts the dot there without
    /// a network — but only if it names a version newer than this one.
    #[test]
    fn a_cache_naming_a_newer_version_becomes_news() {
        let mut ui = UpdateUi::default();
        let newer = Version::new(Version::running().major + 1, 0, 0);
        ui.adopt_cache(&CheckCache { last_checked: 0, latest_seen: newer.to_string() });
        assert!(ui.has_news());
    }

    #[test]
    fn a_cache_naming_an_older_version_is_ignored() {
        let mut ui = UpdateUi::default();
        ui.adopt_cache(&CheckCache { last_checked: 0, latest_seen: "0.0.1".into() });
        assert!(!ui.has_news());
    }

    #[test]
    fn a_cache_with_an_unreadable_version_is_ignored() {
        let mut ui = UpdateUi::default();
        ui.adopt_cache(&CheckCache { last_checked: 0, latest_seen: "nightly".into() });
        assert!(!ui.has_news());
        assert!(ui.known.is_none());
    }

    /// Hands `poll` a finished check without a network anywhere near it:
    /// the channel is the same one `start_check`'s thread would send on,
    /// filled here instead.
    fn finished(origin: Origin, outcome: Result<Availability>) -> UpdateUi {
        let (sender, receiver) = mpsc::channel();
        sender.send(outcome).unwrap();
        UpdateUi { known: None, checking: Some((origin, receiver)) }
    }

    fn unreachable() -> Result<Availability> {
        Err(sm2_core::Error::Update(sm2_core::error::UpdateDefect::MalformedAnswer))
    }

    /// The origin has to survive the check, because it is what decides
    /// whether the answer may be shown. Losing it is how an automatic
    /// check ends up warning on every offline start.
    #[test]
    fn the_answer_says_who_asked_for_the_check() {
        let mut ui = finished(Origin::Automatic, unreachable());
        let (origin, outcome) = ui.poll().expect("the answer is waiting");
        assert_eq!(origin, Origin::Automatic);
        assert!(outcome.is_err());

        let mut ui = finished(Origin::Requested, unreachable());
        let (origin, _) = ui.poll().expect("the answer is waiting");
        assert_eq!(origin, Origin::Requested);
    }

    /// A found update is news whoever asked; the origin only governs how
    /// loud an answer of "nothing found" and a failure may be. Either
    /// way the answer is remembered, so the sidebar's dot does not
    /// depend on who started the check.
    #[test]
    fn a_polled_answer_is_remembered_whoever_asked() {
        let found = Availability::Newer {
            current: Version::new(0, 4, 0),
            latest: Version::new(0, 5, 0),
        };
        let mut ui = finished(Origin::Automatic, Ok(found));
        let (_, outcome) = ui.poll().expect("the answer is waiting");
        assert!(outcome.is_ok());
        assert!(ui.has_news());
    }

    /// A failed check leaves what was known alone: a cache adopted on
    /// start still says there is a new version, and one unreachable
    /// morning must not take that away.
    #[test]
    fn a_failed_check_is_handed_over_once_and_then_forgotten() {
        let mut ui = finished(Origin::Automatic, unreachable());
        assert!(ui.poll().is_some());
        assert!(!ui.is_busy());
        assert!(ui.poll().is_none(), "the answer must not be handed over twice");
    }

    /// On Windows there is no `install.sh`; the button sends the user to
    /// the release page instead of failing. The decision is the
    /// platform's, and this is what the interface asks.
    #[test]
    fn the_button_installs_or_opens_the_page() {
        use sm2_core::platform::{Current, Platform, UpdateMethod};
        let expected =
            if cfg!(unix) { UpdateMethod::Installer } else { UpdateMethod::ReleasePage };
        assert_eq!(Current::update_method(), expected);
    }
}

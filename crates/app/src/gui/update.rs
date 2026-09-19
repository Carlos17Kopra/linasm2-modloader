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

#[derive(Default)]
pub struct UpdateUi {
    /// What the last check — or the cache from an earlier start — found.
    pub known: Option<Availability>,
    /// The running check, if there is one.
    checking: Option<mpsc::Receiver<Result<Availability>>>,
}

impl UpdateUi {
    /// Is there a newer release to offer? `Ahead` and `UpToDate` are not
    /// news: one is a development build, the other is nothing to say.
    ///
    /// Not called from production code yet — the sidebar dot that reads
    /// it is the next change on top of this one; `#[allow(dead_code)]`
    /// says so rather than leaving a clippy warning that looks like an
    /// oversight.
    #[allow(dead_code)]
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
    pub fn start_check(&mut self, ctx: &egui::Context, endpoints: Endpoints) {
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
        self.checking = Some(receiver);
    }

    /// The answer, once. Returns `None` while the check is still running
    /// and on every frame after it has been handed over.
    pub fn poll(&mut self) -> Option<Result<Availability>> {
        let outcome = match self.checking.as_ref()?.try_recv() {
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
        Some(outcome)
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
}

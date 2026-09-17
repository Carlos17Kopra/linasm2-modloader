//! The product's name, in the two forms the interface needs.
//!
//! These are deliberately *not* in the message catalogue, even though the
//! user reads them. A proper name has to be byte-identical in every
//! language: a catalogue entry invites a translator to adapt it, and two
//! entries that are meant to never differ are two entries that will drift
//! apart. `APP_NAME` is composed from the two shorter forms rather than
//! written out a second time, and the test below is what keeps the three
//! from disagreeing.

/// The short form: what the sidebar sets in its brand line and what the
/// start-up screen shows large.
pub const APP_NAME_SHORT: &str = "LiNa SM2";

/// What the short form stands for — the second, smaller line under it.
pub const APP_SUBTITLE: &str = "Mod Launcher";

/// The full name: window title, start-up screen, `--version`.
pub const APP_NAME: &str = "LiNa SM2 - Mod Launcher";

/// The name the machine uses: the binary, the XDG directories under
/// `~/.config`, `~/.local/share` and `~/.local/state`, and the Wayland /
/// X11 application id a desktop file has to match to put the right icon
/// on the window.
pub const APP_SLUG: &str = "lina-sm2";

/// The slug used up to and including 0.1.0, when the program was called
/// "SM2 Mod Loader". Still needed by `paths::app_dirs`, which moves an
/// installation left under that name over to `APP_SLUG` — see
/// `paths::migrate_legacy_dir`.
pub const LEGACY_APP_SLUG: &str = "sm2-modloader";

#[cfg(test)]
mod tests {
    use super::*;

    /// The full name is the two short forms joined by " - ". Written out
    /// separately, the three constants could disagree — a window title
    /// saying one thing and the sidebar another — without anything
    /// noticing.
    #[test]
    fn the_full_name_is_composed_of_the_two_short_forms() {
        assert_eq!(APP_NAME, format!("{APP_NAME_SHORT} - {APP_SUBTITLE}"));
    }

    /// The slug ends up in paths and in a Wayland app id. Both take the
    /// name literally, so an accidental capital letter or space in it
    /// would be silently carried into `~/.config` and into the desktop
    /// file's `StartupWMClass`.
    #[test]
    fn the_slug_is_safe_for_a_path_and_an_app_id() {
        for slug in [APP_SLUG, LEGACY_APP_SLUG] {
            assert!(
                slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{slug} must be lowercase ASCII, digits and hyphens only"
            );
        }
    }

    /// The rename only means anything if the two slugs actually differ —
    /// were they equal, the migration in `paths` would quietly become a
    /// no-op and an old installation would look migrated without being.
    #[test]
    fn the_current_slug_differs_from_the_legacy_one() {
        assert_ne!(APP_SLUG, LEGACY_APP_SLUG);
    }
}

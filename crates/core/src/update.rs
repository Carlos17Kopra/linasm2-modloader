//! Looking for a newer release, and installing it.

use std::fmt;
use std::str::FromStr;

/// A released version, the way the tags carry it: three numbers and
/// nothing else. No `semver` crate for that — the project's tags are
/// checked against the workspace version by the release workflow, so
/// there is never a pre-release or a build suffix to interpret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    // The derived `Ord` compares in declaration order, which is exactly
    // major-then-minor-then-patch. Reordering these fields would silently
    // reorder releases.
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

/// The text was not three numbers. Its own type rather than `()` so that
/// a `map_err` at the call site reads as what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MalformedVersion;

impl Version {
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self { major, minor, patch }
    }

    /// The version of this binary. Both crates inherit the workspace
    /// version, so this is the same number `--version` prints and
    /// `install.sh` compares against.
    pub fn running() -> Self {
        // A panic here cannot reach a user: the value is a compile-time
        // constant of this very crate, and the test below runs on every
        // build.
        env!("CARGO_PKG_VERSION").parse().expect("the package version must be three numbers")
    }
}

impl FromStr for Version {
    type Err = MalformedVersion;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.');
        let major = number(parts.next())?;
        let minor = number(parts.next())?;
        let patch = number(parts.next())?;
        if parts.next().is_some() {
            return Err(MalformedVersion);
        }
        Ok(Self { major, minor, patch })
    }
}

fn number(part: Option<&str>) -> Result<u16, MalformedVersion> {
    part.and_then(|part| part.parse().ok()).ok_or(MalformedVersion)
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_plain_version() {
        assert_eq!("1.2.3".parse::<Version>().unwrap(), Version::new(1, 2, 3));
    }

    /// GitHub's tags carry the `v`; the version the binary reports does
    /// not. Both have to parse, or the comparison compares a tag against
    /// nothing.
    #[test]
    fn parses_a_tag_with_its_v() {
        assert_eq!("v0.4.0".parse::<Version>().unwrap(), Version::new(0, 4, 0));
    }

    #[test]
    fn rejects_what_is_not_three_numbers() {
        for text in ["", "1", "1.2", "1.2.3.4", "1.2.x", "nightly", "v", "1..3"] {
            assert!(text.parse::<Version>().is_err(), "{text} must not parse");
        }
    }

    /// The one comparison that is not obvious, and the reason
    /// `install.sh` needs `sort -V` rather than a string compare: as text
    /// "0.10.0" sorts before "0.9.0" and the launcher would offer a
    /// downgrade.
    #[test]
    fn compares_by_number_not_by_text() {
        assert!(Version::new(0, 10, 0) > Version::new(0, 9, 0));
        assert!(Version::new(1, 0, 0) > Version::new(0, 99, 99));
        assert!(Version::new(0, 4, 1) > Version::new(0, 4, 0));
    }

    #[test]
    fn prints_without_the_v() {
        assert_eq!(Version::new(0, 4, 0).to_string(), "0.4.0");
    }

    /// The workspace version is what `--version` prints and what
    /// `install.sh` reads back out of the installed binary to decide
    /// whether an update is due. If it ever stops being three numbers,
    /// every comparison here silently stops working.
    #[test]
    fn the_running_version_is_the_package_version() {
        assert_eq!(Version::running().to_string(), env!("CARGO_PKG_VERSION"));
    }
}

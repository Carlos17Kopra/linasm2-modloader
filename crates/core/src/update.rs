//! Looking for a newer release, and installing it.

use crate::atomic::write_atomic;
use crate::error::{Error, Result, UpdateDefect};
use crate::paths::AppDirs;
use crate::REPO;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

    fn from_str(text: &str) -> std::result::Result<Self, Self::Err> {
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

fn number(part: Option<&str>) -> std::result::Result<u16, MalformedVersion> {
    part.and_then(|part| part.parse().ok()).ok_or(MalformedVersion)
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// How long the launcher waits for github.com before giving up. Ten
/// seconds is generous for one small JSON answer and short enough that a
/// start-up behind a black-holing firewall is an annoyance rather than a
/// hang — the check runs on a thread of its own, so nothing is blocked
/// meanwhile either way.
pub const NET_TIMEOUT: Duration = Duration::from_secs(10);

/// Where the update path talks to. A struct rather than three loose
/// parameters, because the tests have to redirect all of it at a
/// loopback port and must not do that through environment variables:
/// those are process-global, and `cargo test` runs these tests
/// concurrently.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub api: String,
    pub download: String,
    pub raw: String,
}

impl Endpoints {
    /// The production endpoints. The two variables `install.sh` already
    /// understands keep their names, so a private mirror can redirect
    /// both halves of a release the same way; `LINA_SM2_RAW_BASE` is new
    /// because the script never fetches itself.
    pub fn from_env() -> Self {
        Self {
            api: var("LINA_SM2_API_BASE", "https://api.github.com".into()),
            download: var(
                "LINA_SM2_DOWNLOAD_BASE",
                format!("https://github.com/{REPO}/releases/download"),
            ),
            raw: var("LINA_SM2_RAW_BASE", "https://raw.githubusercontent.com".into()),
        }
    }

    /// Where a human is sent when the launcher cannot install for them.
    pub fn release_page(&self, version: Version) -> String {
        format!("https://github.com/{REPO}/releases/tag/v{version}")
    }
}

fn var(name: &str, fallback: String) -> String {
    std::env::var(name).ok().filter(|value| !value.is_empty()).unwrap_or(fallback)
}

/// What the check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    UpToDate { current: Version },
    Newer { current: Version, latest: Version },
    /// A locally built binary, newer than any release.
    Ahead { current: Version, latest: Version },
}

impl Availability {
    /// The version to offer, if there is one to offer at all.
    pub fn newer(&self) -> Option<Version> {
        match self {
            Availability::Newer { latest, .. } => Some(*latest),
            _ => None,
        }
    }
}

/// Asks the releases API for the newest published tag.
pub fn check(endpoints: &Endpoints, timeout: Duration) -> Result<Availability> {
    let body = fetch_text(&format!("{}/repos/{REPO}/releases/latest", endpoints.api), timeout)?;
    let answer: serde_json::Value =
        serde_json::from_str(&body).map_err(|_| Error::Update(UpdateDefect::MalformedAnswer))?;
    // Everything but `tag_name` is ignored: the answer has some forty
    // fields and a new one on GitHub's side must not break the check.
    let latest: Version = answer["tag_name"]
        .as_str()
        .and_then(|tag| tag.parse().ok())
        .ok_or(Error::Update(UpdateDefect::MalformedAnswer))?;
    let current = Version::running();
    Ok(match current.cmp(&latest) {
        Ordering::Less => Availability::Newer { current, latest },
        Ordering::Equal => Availability::UpToDate { current },
        Ordering::Greater => Availability::Ahead { current, latest },
    })
}

/// One GET, one body as text. Shared with the installer download, which
/// is why it lives here rather than inside `check`.
fn fetch_text(url: &str, timeout: Duration) -> Result<String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_global(Some(timeout))
        // GitHub refuses a request without one.
        .user_agent(format!("{}/{}", crate::APP_SLUG, env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut response = agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| match e {
            // ureq treats a non-2xx answer as an error of its own; every
            // other variant is "we never got an answer".
            ureq::Error::StatusCode(status) => Error::Update(UpdateDefect::HttpStatus(status)),
            other => Error::Update(UpdateDefect::Unreachable { detail: other.to_string() }),
        })?;
    response
        .body_mut()
        .read_to_string()
        .map_err(|e| Error::Update(UpdateDefect::Unreachable { detail: e.to_string() }))
}

/// How long a check counts as recent enough. The rate limit of the
/// unauthenticated API (60 requests per hour and address) is the lesser
/// reason; the real one is that the mark on the sidebar survives a start
/// without a network.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// What the last check found, so the next start does not have to repeat
/// it. Deliberately not part of `Settings`: that file is the user's
/// configuration, and a timestamp would rewrite it on every start.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckCache {
    /// Seconds since the epoch.
    pub last_checked: u64,
    /// The newest release seen then, as text — an unparsable value here
    /// must not keep the file from being read.
    pub latest_seen: String,
}

impl CheckCache {
    pub fn path(dirs: &AppDirs) -> PathBuf {
        dirs.state.join("update-check.toml")
    }

    /// A cache that cannot be read or understood is no cache. It is a
    /// convenience, and turning it into an error would let a stray file
    /// keep the launcher from starting.
    pub fn load(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        toml::from_str(&text).ok()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).map_err(|e| {
            Error::io(path, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        write_atomic(path, &text)
    }

    pub fn latest(&self) -> Option<Version> {
        self.latest_seen.parse().ok()
    }

    /// `now` is passed in rather than read here, so the tests can place
    /// a cache in the past and in the future without touching the clock.
    /// A timestamp ahead of `now` is stale on purpose: a clock corrected
    /// backwards would otherwise freeze the check for as long as the
    /// jump lasted.
    pub fn is_fresh(&self, now: u64) -> bool {
        self.last_checked <= now && now - self.last_checked < CHECK_INTERVAL.as_secs()
    }
}

/// Seconds since the epoch. A clock set before 1970 yields 0, which is
/// "very old" — the safe direction: it checks again.
pub fn now_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Writes down what a check just found. Failing to write the cache is
/// not a failure of the check: the caller has its answer either way.
pub fn remember(dirs: &AppDirs, latest: Version) -> Result<()> {
    let cache = CheckCache { last_checked: now_seconds(), latest_seen: latest.to_string() };
    cache.save(&CheckCache::path(dirs))
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

    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    /// Serves exactly one HTTP answer on a loopback port and returns the
    /// base URL for it. The whole point of `check` taking its endpoints
    /// as a parameter: no test in this suite talks to the real network.
    fn serve_once(status_line: &'static str, body: &'static str) -> Endpoints {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut scratch = [0u8; 2048];
            let _ = stream.read(&mut scratch);
            let answer = format!(
                "HTTP/1.1 {status_line}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(answer.as_bytes());
        });
        let base = format!("http://127.0.0.1:{port}");
        Endpoints { api: base.clone(), download: base.clone(), raw: base }
    }

    fn released(tag: &'static str) -> Endpoints {
        // The real answer has some forty fields; everything but this one
        // is ignored on purpose, so that a new field on GitHub's side
        // cannot break the check.
        match tag {
            "v0.9.0" => serve_once("200 OK", r#"{"tag_name":"v0.9.0","name":"0.9.0"}"#),
            "v0.0.1" => serve_once("200 OK", r#"{"tag_name":"v0.0.1"}"#),
            _ => serve_once("200 OK", r#"{"tag_name":"v0.4.0"}"#),
        }
    }

    #[test]
    fn reports_a_newer_release() {
        let endpoints = released("v0.9.0");
        let found = check(&endpoints, Duration::from_secs(5)).unwrap();
        assert!(
            matches!(found, Availability::Newer { latest, .. } if latest == Version::new(0, 9, 0)),
            "{found:?}"
        );
    }

    #[test]
    fn reports_the_running_version_as_up_to_date() {
        let current = Version::running().to_string();
        let body: &'static str =
            Box::leak(format!(r#"{{"tag_name":"v{current}"}}"#).into_boxed_str());
        let endpoints = serve_once("200 OK", body);
        let found = check(&endpoints, Duration::from_secs(5)).unwrap();
        assert!(matches!(found, Availability::UpToDate { .. }), "{found:?}");
    }

    /// A locally built binary is newer than every release. Its own case,
    /// so that the interface can stay quiet instead of offering what
    /// would be a downgrade.
    #[test]
    fn reports_a_development_build_as_ahead() {
        let endpoints = released("v0.0.1");
        let found = check(&endpoints, Duration::from_secs(5)).unwrap();
        assert!(matches!(found, Availability::Ahead { .. }), "{found:?}");
    }

    #[test]
    fn rejects_an_answer_that_is_not_json() {
        let endpoints = serve_once("200 OK", "<html>maintenance</html>");
        let error = check(&endpoints, Duration::from_secs(5)).unwrap_err();
        assert!(
            matches!(error, Error::Update(UpdateDefect::MalformedAnswer)),
            "{error:?}"
        );
    }

    #[test]
    fn rejects_an_answer_without_a_usable_tag() {
        let endpoints = serve_once("200 OK", r#"{"tag_name":"nightly"}"#);
        let error = check(&endpoints, Duration::from_secs(5)).unwrap_err();
        assert!(
            matches!(error, Error::Update(UpdateDefect::MalformedAnswer)),
            "{error:?}"
        );
    }

    #[test]
    fn passes_the_http_status_on() {
        let endpoints = serve_once("404 Not Found", "nope");
        let error = check(&endpoints, Duration::from_secs(5)).unwrap_err();
        assert!(matches!(error, Error::Update(UpdateDefect::HttpStatus(404))), "{error:?}");
    }

    /// A server that accepts the connection and then says nothing is the
    /// case a missing timeout turns into a frozen start-up.
    #[test]
    fn gives_up_on_a_silent_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let _held = listener.accept();
            std::thread::sleep(Duration::from_secs(30));
        });
        let base = format!("http://127.0.0.1:{port}");
        let endpoints = Endpoints { api: base.clone(), download: base.clone(), raw: base };
        let error = check(&endpoints, Duration::from_millis(300)).unwrap_err();
        assert!(
            matches!(error, Error::Update(UpdateDefect::Unreachable { .. })),
            "{error:?}"
        );
    }

    #[test]
    fn a_missing_cache_is_no_cache_and_no_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(CheckCache::load(&dir.path().join("update-check.toml")).is_none());
    }

    /// A cache file is a convenience, never a reason to fail. Anything
    /// unreadable counts as "not checked yet", which costs one request.
    #[test]
    fn a_corrupt_cache_counts_as_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update-check.toml");
        std::fs::write(&path, "this is not toml {{{").unwrap();
        assert!(CheckCache::load(&path).is_none());
    }

    #[test]
    fn a_cache_survives_a_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update-check.toml");
        let cache = CheckCache { last_checked: 1_758_271_234, latest_seen: "0.5.0".into() };
        cache.save(&path).unwrap();
        let read = CheckCache::load(&path).unwrap();
        assert_eq!(read.last_checked, 1_758_271_234);
        assert_eq!(read.latest(), Some(Version::new(0, 5, 0)));
    }

    #[test]
    fn a_cache_younger_than_a_day_is_fresh() {
        let now = 1_000_000_000;
        let cache = CheckCache { last_checked: now - 3600, latest_seen: "0.5.0".into() };
        assert!(cache.is_fresh(now));
    }

    #[test]
    fn a_cache_older_than_a_day_is_stale() {
        let now = 1_000_000_000;
        let cache = CheckCache { last_checked: now - 25 * 3600, latest_seen: "0.5.0".into() };
        assert!(!cache.is_fresh(now));
    }

    /// A clock that jumped backwards — a corrected system time, a dual
    /// boot — would otherwise make a cache from "the future" fresh for
    /// as long as the jump lasted.
    #[test]
    fn a_cache_from_the_future_is_stale() {
        let now = 1_000_000_000;
        let cache = CheckCache { last_checked: now + 5 * 3600, latest_seen: "0.5.0".into() };
        assert!(!cache.is_fresh(now));
    }

    #[test]
    fn a_cached_version_that_is_junk_is_no_version() {
        let cache = CheckCache { last_checked: 0, latest_seen: "nightly".into() };
        assert_eq!(cache.latest(), None);
    }
}

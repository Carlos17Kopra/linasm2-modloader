//! Looking for a newer release, and installing it.

use crate::atomic::write_atomic;
use crate::error::{Error, Result, UpdateDefect};
use crate::paths::AppDirs;
use crate::platform::{Current, Platform, UpdateMethod};
use crate::REPO;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::ffi::OsString;
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
    ///
    /// An override that is neither HTTPS nor loopback is dropped for the
    /// default and a line in the log — see `is_encrypted_or_loopback`.
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
    match std::env::var(name).ok().filter(|value| !value.is_empty()) {
        Some(value) if is_encrypted_or_loopback(&value) => value,
        Some(value) => {
            tracing::warn!("{name} is not https and not loopback, ignoring it: {value}");
            fallback
        }
        None => fallback,
    }
}

/// May an override point here?
///
/// Only over HTTPS, or to this machine. The verification the update path
/// does — fetch `SHA256SUMS`, fetch the script, compare — proves nothing
/// once both come from the same plaintext origin: whoever can rewrite the
/// script on the wire can rewrite the sums in the same breath, and the
/// launcher would then execute what it just "verified".
///
/// The argument that makes these variables safe for `install.sh` does not
/// carry over. There the person typing the one-liner sets them in the same
/// command; here a window started from a menu entry inherits whatever is
/// in the session's environment, set by who knows what and how long ago.
///
/// Loopback stays allowed because nothing leaves the machine there, and
/// because it is how this suite serves its canned answers — though the
/// tests build `Endpoints` directly and never come through here.
fn is_encrypted_or_loopback(url: &str) -> bool {
    if url.starts_with("https://") {
        return true;
    }
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // Userinfo first: `http://127.0.0.1@evil.example/` is a host of
    // `evil.example`, and reading the part before the `@` as the host is
    // the classic way past a check like this one.
    let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    // A port, but only a real one: stripping after the last colon
    // unconditionally would cut `[::1]` down to `[:`.
    let host = match host.rsplit_once(':') {
        Some((before, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            before
        }
        _ => host,
    };
    matches!(host, "localhost" | "[::1]") || host.strip_prefix("127.").is_some_and(is_dotted_quad)
}

/// The three numbers after `127.` — so that `127.0.0.1` passes and a host
/// named `127.evil.example` does not.
fn is_dotted_quad(rest: &str) -> bool {
    let parts: Vec<&str> = rest.split('.').collect();
    parts.len() == 3 && parts.iter().all(|part| part.parse::<u8>().is_ok())
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

    /// The newest *released* version this check saw — not the one to
    /// offer. `Ahead` saw a release older than the running build and
    /// `UpToDate` saw its own, and both are still what the cache has to
    /// write down: the cache answers "what was on github.com the last
    /// time we looked", which a later start compares against whatever it
    /// is running then.
    ///
    /// A method rather than the same three-arm match at each call site.
    /// It stood twice, byte for byte, in the interface and in the command
    /// line — written by two people who each saw only their own copy —
    /// and if `Ahead`'s meaning ever shifts, the two must not shift apart.
    pub fn latest_release(&self) -> Version {
        match self {
            Availability::UpToDate { current } => *current,
            Availability::Newer { latest, .. } | Availability::Ahead { latest, .. } => *latest,
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

/// What both front ends do after a check: write the answer down, and say
/// so in the log if that did not work. The failure costs one request next
/// time and is not worth showing anyone, let alone failing a check over.
///
/// Shared rather than repeated, for the same reason as
/// `Availability::latest_release`: the interface and the command line had
/// the same paragraph twice, down to the wording of the warning.
pub fn remember_found(dirs: &AppDirs, found: Availability) {
    if let Err(e) = remember(dirs, found.latest_release()) {
        tracing::warn!("update cache not written: {e}");
    }
}

/// The name the installer carries in the release's `SHA256SUMS`.
const INSTALLER_NAME: &str = "install.sh";

/// How much of the installer's output is kept for the error message.
const TAIL_LINES: usize = 5;

/// The file `install.sh` replaces, derived the way the script derives
/// it — same variable name, same default. A private mirror that
/// redirects the installation with `LINA_SM2_BIN_DIR` is recognised as
/// the same installation, because both sides read the one variable.
fn managed_binary() -> Option<PathBuf> {
    managed_binary_in(
        std::env::var_os("LINA_SM2_BIN_DIR"),
        std::env::var_os("HOME"),
    )
}

/// The derivation itself, taking what the environment said instead of
/// reading it, so that the empty cases can be tested without setting
/// process-wide variables out from under every other test.
///
/// `None` when neither variable names a directory. Falling back to a
/// relative `lina-sm2` there would be worse than having no answer: it
/// resolves against the working directory, so a launcher started from
/// the folder it lives in would call itself the managed binary and
/// install over a copy the installer never placed.
fn managed_binary_in(bin_dir: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let dir = bin_dir
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            home.filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".local/bin"))
        })?;
    Some(dir.join(crate::APP_SLUG))
}

/// Is the running program the one file `install.sh` manages?
///
/// The installer replaces exactly one path, `$LINA_SM2_BIN_DIR/lina-sm2`
/// (by default `~/.local/bin/lina-sm2`), and nothing else. A launcher
/// started from a `cargo` build, from a distribution package, or from a
/// folder the user unpacked somewhere would not be replaced by it: the
/// installer would put a *second*, newer copy into `~/.local/bin`, say
/// it succeeded, and the restart it asks for would come back on the old
/// version with no error anywhere to explain it. Refusing instead, and
/// naming the release page, is the one direction that cannot leave
/// someone silently stuck on an old build.
///
/// Both sides are resolved through their symlinks first, because
/// `~/.local/bin/lina-sm2` pointing at the running file is the same
/// installation. Resolving can fail — a path that is not there yet, a
/// directory that cannot be read — and a failure answers "no": the other
/// answer is the one that runs an installer.
///
/// Pure, and takes both paths, so that the decision can be tested
/// without a `~/.local/bin` to install into.
pub fn is_managed_binary(current_exe: &Path, managed: &Path) -> bool {
    // A relative path canonicalises against the working directory, which
    // would make the answer depend on where the launcher was started
    // from. Neither side is ever meant to be relative, so treat one as
    // another thing that cannot be told apart.
    if !current_exe.is_absolute() || !managed.is_absolute() {
        return false;
    }
    let (Ok(current_exe), Ok(managed)) = (current_exe.canonicalize(), managed.canonicalize())
    else {
        return false;
    };
    current_exe == managed
}

/// Can pressing "update" replace this copy in place, or does the user
/// have to be sent to the release page? Two reasons for the latter: the
/// platform has no installer, or this is not the copy the installer
/// manages.
///
/// Deliberately not folded into `Platform::update_method()`: which binary
/// happens to be running is not a property of the platform.
pub fn can_install_in_place() -> bool {
    Current::update_method() == UpdateMethod::Installer
        && managed_binary().is_some_and(|managed| {
            std::env::current_exe().is_ok_and(|exe| is_managed_binary(&exe, &managed))
        })
}

/// Downloads the release's own installer, verifies it against the
/// checksums that release published, and runs it for exactly the version
/// the user was shown.
///
/// Running the script rather than reimplementing it is deliberate: it is
/// the same code path the documented `curl … | sh` line uses, it has its
/// own end-to-end suite (`packaging/test-install.sh`), and a second
/// implementation in Rust would be a second thing to keep correct. The
/// trust anchor is HTTPS to github.com, the same one the README's line
/// rests on; the checksum is not a second anchor but protection against
/// a truncated download, which for a script that is then executed is a
/// real failure mode.
pub fn install(endpoints: &Endpoints, version: Version) -> Result<()> {
    if Current::update_method() != UpdateMethod::Installer {
        return Err(Error::Update(UpdateDefect::NoInstallerForPlatform {
            url: endpoints.release_page(version),
        }));
    }
    // Before the download, not after: there is nothing to fetch for a
    // copy the installer would not replace anyway.
    if !can_install_in_place() {
        return Err(Error::Update(UpdateDefect::NotTheManagedBinary {
            url: endpoints.release_page(version),
        }));
    }
    fetch_verify_and_run(endpoints, version)
}

/// The installation itself, once it has been decided that it may happen.
/// Separate from `install` so the tests can exercise the download, the
/// verification and the run without being the binary `install.sh`
/// manages — a test binary in `target/debug/deps` never is.
fn fetch_verify_and_run(endpoints: &Endpoints, version: Version) -> Result<()> {
    let sums = fetch_text(&format!("{}/v{version}/SHA256SUMS", endpoints.download), NET_TIMEOUT)?;
    let script =
        fetch_text(&format!("{}/{REPO}/v{version}/{INSTALLER_NAME}", endpoints.raw), NET_TIMEOUT)?;

    let digest = sha256_hex(script.as_bytes());
    if checksum_for(INSTALLER_NAME, &sums) != Some(digest.as_str()) {
        return Err(Error::Update(UpdateDefect::ChecksumMismatch {
            version: version.to_string(),
        }));
    }

    // The script lives in a temporary directory that is removed when
    // `dir` drops — after `output()` has returned, never before.
    let dir = tempfile::tempdir().map_err(Error::PlainIo)?;
    let path = dir.path().join(INSTALLER_NAME);
    std::fs::write(&path, &script).map_err(|e| Error::io(&path, e))?;

    let output = std::process::Command::new("sh")
        .arg(&path)
        .arg("--version")
        .arg(version.to_string())
        .output()
        .map_err(|e| Error::io(&path, e))?;

    if !output.status.success() {
        return Err(Error::Update(UpdateDefect::InstallerFailed {
            // A process killed by a signal has no code; -1 says "it did
            // not finish" without pretending to know more.
            code: output.status.code().unwrap_or(-1),
            tail: tail_of(&output.stderr, &output.stdout),
        }));
    }
    Ok(())
}

/// The hash of `bytes`, in the lowercase hex `sha256sum` writes.
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The checksum a `SHA256SUMS` file lists for one exact name. Matching
/// the whole name matters: `my-install.sh` ends in the wanted name and
/// must not be taken for it.
fn checksum_for<'a>(name: &str, sums: &'a str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let (hash, listed) = line.split_once("  ")?;
        (listed.trim() == name).then_some(hash.trim())
    })
}

/// The last few lines the installer said, stderr first — that is where
/// `install.sh`'s own `die` writes.
///
/// A script that fails without a word is a real case — killed, or dying
/// on something the shell itself swallowed — and yields the empty
/// string here. `UpdateDefect::InstallerFailed` looks for that and says
/// a sentence of its own instead of trailing off after its colon; the
/// wording is the catalogue's business, not this function's.
fn tail_of(stderr: &[u8], stdout: &[u8]) -> String {
    let text = String::from_utf8_lossy(if stderr.is_empty() { stdout } else { stderr });
    let lines: Vec<&str> = text.lines().filter(|line| !line.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(TAIL_LINES)..].join("; ")
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

    #[test]
    fn hashes_the_way_sha256sum_does() {
        // The value `printf abc | sha256sum` prints.
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn finds_a_name_in_a_sums_file() {
        let sums = "aaaa  lina-sm2-0.5.0-x86_64-linux.tar.gz\nbbbb  install.sh\n";
        assert_eq!(checksum_for("install.sh", sums), Some("bbbb"));
    }

    /// A release whose SHA256SUMS does not cover the installer must not
    /// quietly skip the verification — that is the whole point of it.
    #[test]
    fn a_missing_name_yields_no_checksum() {
        let sums = "aaaa  lina-sm2-0.5.0-x86_64-linux.tar.gz\n";
        assert_eq!(checksum_for("install.sh", sums), None);
    }

    /// A name that merely *contains* the wanted one — `my-install.sh` —
    /// must not be mistaken for it.
    #[test]
    fn a_similar_name_is_not_a_match() {
        let sums = "aaaa  my-install.sh\n";
        assert_eq!(checksum_for("install.sh", sums), None);
    }

    /// HTTPS is the whole trust anchor of the update path. An override
    /// that drops it hands the sums and the script to the same origin,
    /// and the checksum then proves only that an attacker is consistent.
    #[test]
    fn an_endpoint_override_has_to_be_encrypted() {
        for url in [
            "https://api.github.com",
            "https://mirror.example/releases/download",
            "http://127.0.0.1:38121",
            "http://127.0.0.1",
            "http://localhost:8080/api",
            "http://[::1]:38121",
            "http://[::1]",
        ] {
            assert!(is_encrypted_or_loopback(url), "{url} must be allowed");
        }

        for url in [
            "http://api.github.com",
            "http://mirror.example/releases/download",
            // The host is `evil.example`; the loopback address in front
            // of the `@` is userinfo and names nothing.
            "http://127.0.0.1@evil.example/",
            // Not loopback, only spelled to look like it.
            "http://127.evil.example/",
            "http://localhost.evil.example/",
            // `install.sh` understands these; the launcher does not, and
            // must not execute a script that came out of one.
            "file:///tmp/fake-release",
            "ftp://mirror.example",
            "api.github.com",
        ] {
            assert!(!is_encrypted_or_loopback(url), "{url} must be refused");
        }
    }

    #[test]
    fn the_tail_is_the_last_few_lines_stderr_first() {
        let stderr = b"one\ntwo\nthree\nfour\nfive\nsix\n";
        assert_eq!(tail_of(stderr, b"ignored\n"), "two; three; four; five; six");
        assert_eq!(tail_of(b"", b"only stdout\n"), "only stdout");
    }

    /// An installer that fails without a word — killed, or dying on
    /// something the shell swallowed. The empty tail is what
    /// `UpdateDefect::InstallerFailed` looks for to say a sentence of its
    /// own instead of stopping after its colon.
    #[test]
    fn an_installer_that_said_nothing_has_an_empty_tail() {
        assert_eq!(tail_of(b"", b""), "");
        assert_eq!(tail_of(b"   \n\n", b"\n"), "", "blank lines are not output");
    }

    /// What the cache writes down is the newest release that was seen,
    /// not the version to offer. `Ahead` is the case that separates the
    /// two: the running build is newer than everything published, and
    /// the release is still what github.com said.
    #[test]
    fn the_remembered_version_is_the_newest_release_seen() {
        let current = Version::new(1, 0, 0);
        let latest = Version::new(0, 9, 0);
        assert_eq!(Availability::UpToDate { current }.latest_release(), current);
        assert_eq!(Availability::Ahead { current, latest }.latest_release(), latest);
        assert_eq!(
            Availability::Newer { current: latest, latest: current }.latest_release(),
            current
        );
    }

    #[test]
    fn the_file_the_installer_replaces_is_the_managed_one() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join(crate::APP_SLUG);
        std::fs::write(&exe, b"a binary").unwrap();
        assert!(is_managed_binary(&exe, &exe));
    }

    /// A `cargo run` build, a distribution package, a copy unpacked into
    /// some other folder: running the installer for one of those puts a
    /// *second* copy into `~/.local/bin` and reports success, and the
    /// restart it then asks for comes back on the old version with
    /// nothing anywhere to say why.
    #[test]
    fn a_copy_somewhere_else_is_not_the_managed_one() {
        let dir = tempfile::tempdir().unwrap();
        let managed = dir.path().join("bin").join(crate::APP_SLUG);
        let elsewhere = dir.path().join("target/release").join(crate::APP_SLUG);
        for path in [&managed, &elsewhere] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"a binary").unwrap();
        }
        assert!(!is_managed_binary(&elsewhere, &managed));
    }

    /// Nothing installed there yet, a path that has gone, a directory
    /// that cannot be read: not being able to tell must come out as "not
    /// managed". The other answer is the one that runs the installer.
    #[test]
    fn a_path_that_cannot_be_resolved_is_not_managed() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join(crate::APP_SLUG);
        std::fs::write(&exe, b"a binary").unwrap();
        let absent = dir.path().join("nowhere").join(crate::APP_SLUG);
        assert!(!is_managed_binary(&exe, &absent));
        assert!(!is_managed_binary(&absent, &exe));
    }

    /// With neither `LINA_SM2_BIN_DIR` nor `HOME` set there is no
    /// `~/.local/bin` to speak of, and the answer has to be "no idea"
    /// rather than a bare `lina-sm2`: that resolves against the working
    /// directory, so a launcher started from its own folder would decide
    /// it was the copy the installer manages and install over it.
    #[test]
    fn nothing_is_the_managed_binary_without_a_directory_to_put_it_in() {
        assert_eq!(managed_binary_in(None, None), None);
        assert_eq!(managed_binary_in(Some(OsString::new()), Some(OsString::new())), None);
        assert_eq!(
            managed_binary_in(Some(OsString::from("/opt/bin")), None),
            Some(PathBuf::from("/opt/bin").join(crate::APP_SLUG))
        );
        assert_eq!(
            managed_binary_in(None, Some(OsString::from("/home/someone"))),
            Some(PathBuf::from("/home/someone/.local/bin").join(crate::APP_SLUG))
        );
    }

    /// The same hole one layer down, for any future caller that hands
    /// the predicate a path it did not build itself.
    #[test]
    fn a_relative_path_is_not_the_managed_one() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join(crate::APP_SLUG);
        std::fs::write(&exe, b"a binary").unwrap();
        let relative = PathBuf::from(crate::APP_SLUG);
        assert!(!is_managed_binary(&exe, &relative));
        assert!(!is_managed_binary(&relative, &exe));
    }

    /// `~/.local/bin/lina-sm2` may well be a symlink to wherever the file
    /// really lives; that is still the installation `install.sh`
    /// replaces. Comparing the two paths as written would send its owner
    /// to the release page for nothing.
    // Unix only because creating a symlink as a fixture needs privileges
    // on Windows — the second of the three reasons CLAUDE.md lists. The
    // guard itself rests on `canonicalize` and is platform-neutral.
    #[cfg(unix)]
    #[test]
    fn a_symlink_to_the_running_binary_is_the_managed_one() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("opt").join(crate::APP_SLUG);
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, b"a binary").unwrap();
        let managed = dir.path().join(crate::APP_SLUG);
        std::os::unix::fs::symlink(&real, &managed).unwrap();
        assert!(is_managed_binary(&real, &managed));
    }

    /// The guard runs before anything is downloaded. The endpoints point
    /// at a port nothing listens on, so a failure to refuse shows up as
    /// `Unreachable` rather than as a silent pass — and the test binary,
    /// which lives in `target/debug/deps`, is exactly the kind of copy
    /// the installer does not manage.
    // Unix only for the same reason as `serve_release`, below: on Windows
    // `install` stops one step earlier, at `NoInstallerForPlatform`.
    #[cfg(unix)]
    #[test]
    fn refuses_to_install_over_a_binary_the_installer_does_not_manage() {
        let nowhere = "http://127.0.0.1:1".to_string();
        let endpoints = Endpoints {
            api: nowhere.clone(),
            download: nowhere.clone(),
            raw: nowhere,
        };
        let error = install(&endpoints, Version::new(9, 9, 9)).unwrap_err();
        assert!(
            matches!(error, Error::Update(UpdateDefect::NotTheManagedBinary { .. })),
            "{error:?}"
        );
    }

    /// Serves SHA256SUMS and install.sh on one loopback port, each once,
    /// in the order the installer asks for them.
    // Unix only, and this one really is the behaviour, not a fixture
    // limitation: `install.sh` and `sh` do not exist on Windows, where
    // `Current::update_method()` returns `UpdateMethod::ReleasePage`
    // instead of `Installer` and `install` sends the user to the release
    // page rather than running a script. None of the three fixture-
    // related reasons CLAUDE.md lists for `#[cfg(unix)]` applies here.
    #[cfg(unix)]
    fn serve_release(sums: String, script: String) -> Endpoints {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for body in [sums, script] {
                let Ok((mut stream, _)) = listener.accept() else { return };
                let mut scratch = [0u8; 2048];
                let _ = stream.read(&mut scratch);
                let answer = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(answer.as_bytes());
            }
        });
        let base = format!("http://127.0.0.1:{port}");
        Endpoints { api: base.clone(), download: base.clone(), raw: base }
    }

    /// The installer is executed, so it has to be the file the release
    /// published. A truncated download is the realistic failure, and
    /// half a shell script is exactly what must not run.
    // Unix only for the same reason as `serve_release`, above: real
    // platform behaviour, not a fixture limitation.
    #[cfg(unix)]
    #[test]
    fn refuses_an_installer_that_does_not_match_its_checksum() {
        let script = "#!/bin/sh\nexit 0\n".to_string();
        let sums = format!("{}  install.sh\n", sha256_hex(b"something else"));
        let endpoints = serve_release(sums, script);
        let error = fetch_verify_and_run(&endpoints, Version::new(9, 9, 9)).unwrap_err();
        assert!(
            matches!(error, Error::Update(UpdateDefect::ChecksumMismatch { .. })),
            "{error:?}"
        );
    }

    // Unix only for the same reason as `serve_release`, above: real
    // platform behaviour, not a fixture limitation.
    #[cfg(unix)]
    #[test]
    fn runs_an_installer_that_matches() {
        let dir = tempfile::tempdir().unwrap();
        let witness = dir.path().join("ran");
        // The script records that it ran, and with which arguments — the
        // version has to reach it, or the launcher would install
        // whatever is newest instead of what the user was shown.
        let script = format!("#!/bin/sh\nprintf '%s' \"$*\" > {}\n", witness.display());
        let sums = format!("{}  install.sh\n", sha256_hex(script.as_bytes()));
        let endpoints = serve_release(sums, script);

        fetch_verify_and_run(&endpoints, Version::new(9, 9, 9)).unwrap();

        assert_eq!(std::fs::read_to_string(&witness).unwrap(), "--version 9.9.9");
    }

    // Unix only for the same reason as `serve_release`, above: real
    // platform behaviour, not a fixture limitation.
    #[cfg(unix)]
    #[test]
    fn reports_the_exit_code_of_a_failed_installer() {
        let script = "#!/bin/sh\necho 'no write permission' >&2\nexit 3\n".to_string();
        let sums = format!("{}  install.sh\n", sha256_hex(script.as_bytes()));
        let endpoints = serve_release(sums, script);
        let error = fetch_verify_and_run(&endpoints, Version::new(9, 9, 9)).unwrap_err();
        assert!(
            matches!(error, Error::Update(UpdateDefect::InstallerFailed { code: 3, .. })),
            "{error:?}"
        );
    }
}

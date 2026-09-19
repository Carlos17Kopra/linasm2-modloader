# Update Check and Self-Update Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The launcher asks GitHub whether a newer release exists, says so
on the settings page and in the CLI, and installs it when the user presses
a button by running that release's own `install.sh`.

**Architecture:** All domain logic goes into one new module,
`crates/core/src/update.rs`: a `Version` type, a `check` that reads
`tag_name` from the GitHub releases API, a daily cache in the state
directory, and an `install` that downloads `install.sh`, verifies it
against the release's `SHA256SUMS` and runs it. The platform difference —
Linux installs, Windows opens the release page — sits behind the existing
`core::platform::Platform` trait, so nothing above it carries a `cfg`. The
interface keeps its own state in a new `crates/app/src/gui/update.rs`.

**Tech Stack:** Rust, `ureq` 3.4 (blocking HTTP, rustls), `sha2` 0.11,
`serde_json` (already present), egui/eframe, clap.

**Spec:** `docs/superpowers/specs/2026-09-19-update-check-design.md`

## Global Constraints

- Code, comments, test names and `assert!` messages are English. Every
  user-facing sentence lives in `crates/core/i18n/en.toml` and
  `crates/core/i18n/de.toml`, reached with `t!("area.key")`. Both files
  carry exactly the same keys.
- `crates/app/tests/no_german_literals.rs` has an empty `EXEMPT_LITERALS`.
  No German string may appear in any `.rs` file.
- Comment prose wraps at 78 columns including the `///` prefix. Comments
  explain *why*, never *what*.
- MSRV: `sm2-core` 1.85, `lina-sm2` 1.95. `ureq` 3.4.2 and `sha2` 0.11.0
  both declare `rust-version = 1.85`, so neither moves — but there is no
  headroom left.
- `cargo test` (330 tests today) and `cargo clippy --all-targets` stay
  clean after every task.
- Tests that depend on the active language must hold
  `sm2_core::i18n::language_test_lock()` (in `sm2-core`) or
  `crate::app_state::language_test_lock()` (in `lina-sm2`).
- No test may touch the real network. Every test points at
  `127.0.0.1`.
- Tests gated to one platform must say which of the reasons applies. The
  installer tests here are `#[cfg(unix)]` because `install.sh` does not
  exist on Windows — real platform behaviour, **not** one of the three
  fixture gaps CLAUDE.md lists.
- Write the test first, watch it fail, then implement.

## Verified facts this plan rests on

These were checked against a scratch crate compiled with `ureq` 3.4.2 and
`sha2` 0.11.0; do not "correct" them from memory:

- An agent is built as
  `let agent: ureq::Agent = ureq::Agent::config_builder()…build().into();`
- A non-2xx answer arrives as `Err(ureq::Error::StatusCode(code))`, not as
  an `Ok` response with a status. A timeout arrives as a different
  `ureq::Error` variant.
- The body is read with `response.body_mut().read_to_string()`.
- `sha2::Sha256::digest(bytes)` formatted as `{b:02x}` per byte equals
  what `sha256sum` prints.
- `crates/core` and `crates/app` both inherit the workspace version, so
  `env!("CARGO_PKG_VERSION")` inside `sm2-core` is the version
  `--version` prints and `install.sh` compares against.

## Deviations from the spec

Four refinements found while pinning the code down. They are improvements
within the approved design, not changes to it:

1. The three base URLs become one `Endpoints` struct with
   `Endpoints::from_env()`, instead of three loose parameters. Tests then
   never touch environment variables, which are process-global and race
   across the concurrently running test threads.
2. The catalogue keys are `error.update_failed` (umbrella) and the table
   `[error.update_defect]`. The spec wrote `[error.update]`, which would
   collide in TOML with an `update` key inside `[error]` — the same reason
   the existing pair is `error.corrupt_backup` plus
   `[error.backup_defect]`.
3. `UpdateDefect::Unreachable` carries a `detail: String`. Without it a
   user report says only "not reachable"; the existing `error.io` sets the
   precedent of embedding a source.
4. `side_bar::nav_item` gains a `dot: bool` parameter rather than abusing
   the existing count slot for a bullet character.

## File Structure

**Created:**

- `crates/core/src/update.rs` — the whole domain: `Version`,
  `Availability`, `Endpoints`, `check`, `CheckCache`, `install`.
- `crates/app/src/gui/update.rs` — the interface's state for the feature:
  what is known, the running check, the running installation.

**Modified:**

- `Cargo.toml` — `ureq`, `sha2` in `[workspace.dependencies]`.
- `crates/core/Cargo.toml` — both as dependencies.
- `crates/core/src/lib.rs` — `pub mod update;`.
- `crates/core/src/branding.rs` — `REPO` plus its coupling test.
- `crates/core/src/error.rs` — `Error::Update(UpdateDefect)`.
- `crates/core/src/settings.rs` — `update_check: Option<bool>`.
- `crates/core/src/platform/mod.rs`, `unix.rs`, `windows.rs` —
  `UpdateMethod`, `update_method()`, `open_url()`.
- `crates/core/i18n/en.toml`, `de.toml` — the new keys.
- `crates/app/src/cli.rs` — `Command::Update`.
- `crates/app/src/gui/mod.rs` — one field, the actions, the dialog
  variant, the automatic check.
- `crates/app/src/gui/settings_page.rs` — the update block.
- `crates/app/src/gui/side_bar.rs` — the dot.
- `crates/app/src/gui/dialogs.rs` — the first-start question.
- `.github/workflows/release.yml` — `install.sh` as a release asset.
- `README.md`, `CLAUDE.md` — the feature and its constraints.

---

### Task 1: The `Version` type

**Files:**
- Create: `crates/core/src/update.rs`
- Modify: `crates/core/src/lib.rs:14` (add `pub mod update;` in
  alphabetical order, after `settings`)

**Interfaces:**
- Consumes: nothing.
- Produces: `sm2_core::update::Version { major: u16, minor: u16, patch: u16 }`
  with `FromStr` (error type `MalformedVersion`), `Display`, `Ord`, and
  `Version::running() -> Version`.

- [ ] **Step 1: Write the failing tests**

Create `crates/core/src/update.rs` with only the test module:

```rust
//! Looking for a newer release, and installing it.

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
```

- [ ] **Step 2: Run the tests to verify they fail**

Add `pub mod update;` to `crates/core/src/lib.rs` first, then:

Run: `cargo test -p sm2-core update::`
Expected: FAIL, `cannot find type Version in this scope`.

- [ ] **Step 3: Write the implementation**

Above the test module in `crates/core/src/update.rs`:

```rust
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p sm2-core update::` then `cargo clippy --all-targets`
Expected: PASS, no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/update.rs crates/core/src/lib.rs
git commit -m "feat(update): a version type that compares by number"
```

---

### Task 2: The repository slug in one place

**Files:**
- Modify: `crates/core/src/branding.rs` (constant after `LEGACY_APP_SLUG`,
  test in the existing `mod tests`)
- Modify: `crates/core/src/lib.rs:16` (re-export)

**Interfaces:**
- Consumes: nothing.
- Produces: `sm2_core::branding::REPO: &str`, re-exported as
  `sm2_core::REPO`.

- [ ] **Step 1: Write the failing test**

In the existing `mod tests` of `crates/core/src/branding.rs`:

```rust
    /// `install.sh` spells the repository out too, and Rust cannot read
    /// it from there — so it exists twice, which is the one thing this
    /// module is against. The two are held together here instead: change
    /// one and this test names the other. Same construction as the
    /// coupling between `StartupWMClass` and `APP_SLUG`.
    #[test]
    fn install_sh_and_branding_agree_on_the_repository() {
        let script_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../install.sh");
        let script = std::fs::read_to_string(&script_path)
            .unwrap_or_else(|e| panic!("{} is not readable: {e}", script_path.display()));

        let line = script
            .lines()
            .find(|line| line.starts_with("REPO="))
            .expect("install.sh must set REPO=");
        // REPO="${LINA_SM2_REPO:-owner/name}"
        let in_script = line
            .split_once(":-")
            .and_then(|(_, rest)| rest.split_once('}'))
            .map(|(value, _)| value)
            .expect("install.sh's REPO= must keep its ${VAR:-default} shape");

        assert_eq!(in_script, REPO, "install.sh and branding.rs name different repositories");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p sm2-core branding::`
Expected: FAIL, `cannot find value REPO in this scope`.

- [ ] **Step 3: Write the implementation**

In `crates/core/src/branding.rs`, after `LEGACY_APP_SLUG`:

```rust
/// The GitHub repository, as `owner/name`. The update check asks its
/// releases API, and the installer it downloads comes from the same
/// place. `install.sh` names it a second time because a shell script
/// cannot read a Rust constant — the test below is what keeps the two
/// from drifting apart.
pub const REPO: &str = "Carlos17Kopra/linasm2-modloader";
```

In `crates/core/src/lib.rs:16`, extend the re-export:

```rust
pub use branding::{APP_NAME, APP_NAME_SHORT, APP_SLUG, APP_SUBTITLE, LEGACY_APP_SLUG, REPO};
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p sm2-core branding::`
Expected: PASS, 4 tests in that module.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/branding.rs crates/core/src/lib.rs
git commit -m "feat(update): name the repository once, next to the app name"
```

---

### Task 3: The error variants and their catalogue entries

**Files:**
- Modify: `crates/core/src/error.rs` (enum at `:5-29`, a `text()` impl
  next to `ArchiveDefect`'s, one arm in `Display`)
- Modify: `crates/core/i18n/en.toml`, `crates/core/i18n/de.toml`

**Interfaces:**
- Consumes: `Version` (Task 1) — only in a message parameter, as text.
- Produces: `sm2_core::error::UpdateDefect` with variants
  `Unreachable { detail: String }`, `HttpStatus(u16)`, `MalformedAnswer`,
  `ChecksumMismatch { version: String }`, `NoInstallerForPlatform { url: String }`,
  `InstallerFailed { code: i32, tail: String }`, and
  `Error::Update(UpdateDefect)`.

- [ ] **Step 1: Write the failing test**

In the existing `mod tests` of `crates/core/src/error.rs`:

```rust
    /// The detail of an update failure goes through the catalogue like
    /// every other `Display` output — a `String` payload built with
    /// `format!` would freeze its wording in whichever language happened
    /// to be active when the error was constructed.
    #[test]
    fn an_update_failure_names_the_status_it_got() {
        let _guard = crate::i18n::language_test_lock();
        crate::i18n::set_language(crate::i18n::Language::English);
        let error = Error::Update(UpdateDefect::HttpStatus(403));
        let text = error.to_string();
        assert!(text.contains("403"), "{text}");
    }

    #[test]
    fn an_unreachable_host_keeps_its_detail() {
        let _guard = crate::i18n::language_test_lock();
        crate::i18n::set_language(crate::i18n::Language::English);
        let error = Error::Update(UpdateDefect::Unreachable { detail: "dns".into() });
        assert!(error.to_string().contains("dns"), "{error}");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p sm2-core error::`
Expected: FAIL, `no variant named Update`.

- [ ] **Step 3: Write the implementation**

In `crates/core/src/error.rs`, add to `pub enum Error` (before `Io`):

```rust
    /// Looking for a new version, or installing it, went wrong. Its own
    /// defect type for the same reason as `BackupDefect`: the detail is
    /// translated at `Display` time, not baked in where it happened.
    Update(UpdateDefect),
```

Next to the other defect enums:

```rust
/// Why an update check or an installation failed — same idea as
/// `BackupDefect`.
#[derive(Debug)]
pub enum UpdateDefect {
    /// No route, no DNS, no answer in time. `detail` is the client's own
    /// English wording; without it a bug report says only "it did not
    /// work".
    Unreachable { detail: String },
    HttpStatus(u16),
    MalformedAnswer,
    /// The downloaded installer does not match the checksum the release
    /// published for it. A truncated download is the realistic case, and
    /// executing half a shell script is exactly what must not happen.
    ChecksumMismatch { version: String },
    NoInstallerForPlatform { url: String },
    InstallerFailed { code: i32, tail: String },
}
```

Next to `impl ArchiveDefect`:

```rust
impl UpdateDefect {
    fn text(&self) -> String {
        match self {
            UpdateDefect::Unreachable { detail } => {
                i18n::format("error.update_defect.unreachable", &[("detail", detail.clone())])
            }
            UpdateDefect::HttpStatus(status) => {
                i18n::format("error.update_defect.http_status", &[("status", status.to_string())])
            }
            UpdateDefect::MalformedAnswer => i18n::lookup("error.update_defect.malformed_answer"),
            UpdateDefect::ChecksumMismatch { version } => i18n::format(
                "error.update_defect.checksum_mismatch",
                &[("version", version.clone())],
            ),
            UpdateDefect::NoInstallerForPlatform { url } => i18n::format(
                "error.update_defect.no_installer_for_platform",
                &[("url", url.clone())],
            ),
            UpdateDefect::InstallerFailed { code, tail } => i18n::format(
                "error.update_defect.installer_failed",
                &[("code", code.to_string()), ("tail", tail.clone())],
            ),
        }
    }
}
```

In `impl fmt::Display for Error`, next to the `CorruptBackup` arm:

```rust
            Error::Update(defect) => {
                i18n::format("error.update_failed", &[("detail", defect.text())])
            }
```

In `crates/core/i18n/en.toml`, inside `[error]` (after `unusable_archive`):

```toml
update_failed = "Update failed: {detail}"
```

and a new table after `[error.archive_defect]`:

```toml
[error.update_defect]
unreachable = "github.com could not be reached ({detail})"
http_status = "github.com answered with status {status}"
malformed_answer = "github.com's answer could not be read"
checksum_mismatch = "the downloaded installer does not match the checksum published for release {version}"
no_installer_for_platform = "there is no installer for this platform – please download the new version from {url}"
installer_failed = "the installer stopped with exit code {code}: {tail}"
```

In `crates/core/i18n/de.toml`, the same keys:

```toml
update_failed = "Update fehlgeschlagen: {detail}"
```

```toml
[error.update_defect]
unreachable = "github.com war nicht erreichbar ({detail})"
http_status = "github.com hat mit Status {status} geantwortet"
malformed_answer = "die Antwort von github.com war nicht lesbar"
checksum_mismatch = "das heruntergeladene Installationsskript passt nicht zu der Prüfsumme, die Release {version} dafür veröffentlicht hat"
no_installer_for_platform = "für diese Plattform gibt es kein Installationsskript – bitte die neue Version unter {url} herunterladen"
installer_failed = "das Installationsskript hat mit Code {code} abgebrochen: {tail}"
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p sm2-core` then `cargo test -p lina-sm2 i18n`
Expected: PASS, including the key-parity and placeholder tests.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/error.rs crates/core/i18n/en.toml crates/core/i18n/de.toml
git commit -m "feat(update): translatable failures for the update path"
```

---

### Task 4: `check` against the releases API

**Files:**
- Modify: `Cargo.toml` (workspace dependencies), `crates/core/Cargo.toml`
- Modify: `crates/core/src/update.rs`

**Interfaces:**
- Consumes: `Version`, `Error::Update`, `UpdateDefect`, `REPO`.
- Produces:
  - `Endpoints { api: String, download: String, raw: String }` with
    `Endpoints::from_env() -> Endpoints`
  - `Availability::{UpToDate { current }, Newer { current, latest }, Ahead { current, latest }}`
  - `check(endpoints: &Endpoints, timeout: Duration) -> Result<Availability>`
  - `NET_TIMEOUT: Duration`
  - `fetch_text(url: &str, timeout: Duration) -> Result<String>` (crate
    internal, used again by Task 7)

- [ ] **Step 1: Write the failing tests**

In `crates/core/src/update.rs`'s test module:

```rust
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p sm2-core update::`
Expected: FAIL, `cannot find function check`.

- [ ] **Step 3: Add the dependencies**

In `Cargo.toml` under `[workspace.dependencies]`, keeping the existing
order:

```toml
ureq = "3.4"
sha2 = "0.11"
```

In `crates/core/Cargo.toml` under `[dependencies]`:

```toml
ureq.workspace = true
sha2.workspace = true
```

(`sha2` is unused until Task 7; adding both here keeps the two dependency
commits from interleaving. If the reviewer prefers, move the `sha2` lines
to Task 7 — nothing else changes.)

- [ ] **Step 4: Write the implementation**

In `crates/core/src/update.rs`, above the test module:

```rust
use crate::error::{Error, Result, UpdateDefect};
use crate::REPO;
use std::cmp::Ordering;
use std::time::Duration;

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
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p sm2-core update::` then `cargo clippy --all-targets`
Expected: PASS, no warnings.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/core/Cargo.toml crates/core/src/update.rs
git commit -m "feat(update): ask the releases API for the newest tag"
```

---

### Task 5: The daily cache

**Files:**
- Modify: `crates/core/src/update.rs`

**Interfaces:**
- Consumes: `Version`, `Availability`, `atomic::write_atomic`,
  `paths::AppDirs`.
- Produces:
  - `CheckCache { last_checked: u64, latest_seen: String }` with
    `path(dirs)`, `load(path) -> Option<CheckCache>`,
    `save(&self, path) -> Result<()>`, `latest(&self) -> Option<Version>`,
    `is_fresh(&self, now: u64) -> bool`
  - `CHECK_INTERVAL: Duration`
  - `now_seconds() -> u64`
  - `remember(dirs: &AppDirs, latest: Version) -> Result<()>`

- [ ] **Step 1: Write the failing tests**

In the test module of `crates/core/src/update.rs`:

```rust
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p sm2-core update::`
Expected: FAIL, `cannot find struct CheckCache`.

- [ ] **Step 3: Write the implementation**

In `crates/core/src/update.rs`:

```rust
use crate::atomic::write_atomic;
use crate::paths::AppDirs;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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
```

Note for `remember`: `Availability::UpToDate` carries no `latest`, so the
callers pass `current` in that case — the newest *released* version is
what the cache is about.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p sm2-core update::` then `cargo clippy --all-targets`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/update.rs
git commit -m "feat(update): remember the last check for a day"
```

---

### Task 6: The platform split

**Files:**
- Modify: `crates/core/src/platform/mod.rs` (trait, and the new enum next
  to it)
- Modify: `crates/core/src/platform/unix.rs`
- Modify: `crates/core/src/platform/windows.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `platform::UpdateMethod::{Installer, ReleasePage}`,
  `Platform::update_method() -> UpdateMethod`,
  `Platform::open_url(url: &str) -> Result<()>`.

- [ ] **Step 1: Write the failing test**

In `crates/core/src/platform/mod.rs`, add a test module (or extend the
existing one):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Linux installs by running the release's own `install.sh`; Windows
    /// has no such script and can only send the user to the release
    /// page. This is the one genuinely platform-dependent piece of the
    /// update path, and the reason it sits behind the trait instead of
    /// behind a `cfg` somewhere above it.
    #[test]
    fn every_platform_says_how_it_updates() {
        let method = Current::update_method();
        if cfg!(unix) {
            assert_eq!(method, UpdateMethod::Installer);
        } else {
            assert_eq!(method, UpdateMethod::ReleasePage);
        }
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p sm2-core platform::`
Expected: FAIL, `cannot find type UpdateMethod`.

- [ ] **Step 3: Write the implementation**

In `crates/core/src/platform/mod.rs`, next to the trait:

```rust
/// How a found update is applied here. It describes the platform, so it
/// lives with the trait rather than in `update`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateMethod {
    /// Download the release's `install.sh`, verify it, run it.
    Installer,
    /// No installer exists here — send the user to the release page.
    ReleasePage,
}
```

Add to `pub trait Platform`:

```rust
    /// How a found update is applied here.
    fn update_method() -> UpdateMethod;

    /// Opens a URL in whatever the user browses with.
    fn open_url(url: &str) -> Result<()>;
```

In `crates/core/src/platform/unix.rs`:

```rust
    fn update_method() -> UpdateMethod {
        UpdateMethod::Installer
    }

    fn open_url(url: &str) -> Result<()> {
        // Same opener the file manager gets; a URL has no path to name
        // in the error, so `PlainIo` rather than `Error::io`.
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(Error::PlainIo)?;
        Ok(())
    }
```

In `crates/core/src/platform/windows.rs`:

```rust
    fn update_method() -> UpdateMethod {
        UpdateMethod::ReleasePage
    }

    fn open_url(url: &str) -> Result<()> {
        // `start` is a shell builtin, not a program, hence the detour
        // through cmd. The empty argument is the window title `start`
        // would otherwise take the URL for.
        Command::new("cmd").args(["/C", "start", "", url]).spawn().map_err(Error::PlainIo)?;
        Ok(())
    }
```

Add the `UpdateMethod` import to both platform files' `use super::…` line.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p sm2-core platform::` then `cargo clippy --all-targets`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/platform/
git commit -m "feat(update): let the platform say how it updates"
```

---

### Task 7: `install`, and `install.sh` as a release asset

**Files:**
- Modify: `crates/core/src/update.rs`
- Modify: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: `Endpoints`, `Version`, `fetch_text`, `UpdateDefect`,
  `Current::update_method()`.
- Produces: `install(endpoints: &Endpoints, version: Version) -> Result<()>`,
  `sha256_hex(bytes: &[u8]) -> String`,
  `checksum_for(name: &str, sums: &str) -> Option<&str>`.

- [ ] **Step 1: Write the failing tests**

In the test module of `crates/core/src/update.rs`:

```rust
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
```

And, `#[cfg(unix)]` because `install.sh` and `sh` do not exist on
Windows — real platform behaviour, not a fixture limitation:

```rust
    /// Serves SHA256SUMS and install.sh on one loopback port, each once,
    /// in the order the installer asks for them.
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
    #[cfg(unix)]
    #[test]
    fn refuses_an_installer_that_does_not_match_its_checksum() {
        let script = "#!/bin/sh\nexit 0\n".to_string();
        let sums = format!("{}  install.sh\n", sha256_hex(b"something else"));
        let endpoints = serve_release(sums, script);
        let error = install(&endpoints, Version::new(9, 9, 9)).unwrap_err();
        assert!(
            matches!(error, Error::Update(UpdateDefect::ChecksumMismatch { .. })),
            "{error:?}"
        );
    }

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

        install(&endpoints, Version::new(9, 9, 9)).unwrap();

        assert_eq!(std::fs::read_to_string(&witness).unwrap(), "--version 9.9.9");
    }

    #[cfg(unix)]
    #[test]
    fn reports_the_exit_code_of_a_failed_installer() {
        let script = "#!/bin/sh\necho 'no write permission' >&2\nexit 3\n".to_string();
        let sums = format!("{}  install.sh\n", sha256_hex(script.as_bytes()));
        let endpoints = serve_release(sums, script);
        let error = install(&endpoints, Version::new(9, 9, 9)).unwrap_err();
        assert!(
            matches!(error, Error::Update(UpdateDefect::InstallerFailed { code: 3, .. })),
            "{error:?}"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p sm2-core update::`
Expected: FAIL, `cannot find function install`.

- [ ] **Step 3: Write the implementation**

In `crates/core/src/update.rs`:

```rust
use crate::platform::{Current, Platform, UpdateMethod};
use sha2::{Digest, Sha256};

/// The name the installer carries in the release's `SHA256SUMS`.
const INSTALLER_NAME: &str = "install.sh";

/// How much of the installer's output is kept for the error message.
const TAIL_LINES: usize = 5;

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

    let sums = fetch_text(&format!("{}/v{version}/SHA256SUMS", endpoints.download), NET_TIMEOUT)?;
    let script =
        fetch_text(&format!("{}/{REPO}/v{version}/{INSTALLER_NAME}", endpoints.raw), NET_TIMEOUT)?;

    // Bound, not inlined into the comparison: `Some(temp.as_str())` in an
    // `if` condition borrows a temporary that is easy to trip over later.
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
fn tail_of(stderr: &[u8], stdout: &[u8]) -> String {
    let text = String::from_utf8_lossy(if stderr.is_empty() { stdout } else { stderr });
    let lines: Vec<&str> = text.lines().filter(|line| !line.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(TAIL_LINES)..].join("; ")
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p sm2-core update::` then `cargo clippy --all-targets`
Expected: PASS.

- [ ] **Step 5: Put `install.sh` into the release**

In `.github/workflows/release.yml`, in the job that assembles `dist/` for
Linux (next to the `cp LICENSE` line at `:114`), and before the
`sha256sum` line at `:176`, copy the script into `dist/` so that
`SHA256SUMS` covers it:

```yaml
          # The launcher downloads this script to update itself and
          # verifies it against SHA256SUMS before running it. It is a
          # release asset for that reason alone — the documented
          # `curl … | sh` line still fetches it from the default branch.
          cp install.sh dist/
```

The existing `(cd dist && sha256sum lina-sm2-* > SHA256SUMS)` only covers
names starting with `lina-sm2-`. Widen it to name both explicitly:

```yaml
          (cd dist && sha256sum lina-sm2-* install.sh > SHA256SUMS)
```

Upload `dist/install.sh` alongside the other assets in the release step.

- [ ] **Step 6: Check the packaging suite still passes**

Run: `cargo test -p lina-sm2 install_script`
Expected: PASS. If `packaging/test-install.sh` asserts the exact set of
published files, extend it there — that suite is what notices when the
three places disagree about asset names.

- [ ] **Step 7: Commit**

```bash
git add crates/core/src/update.rs .github/workflows/release.yml packaging/
git commit -m "feat(update): install a new version by running its own installer"
```

---

### Task 8: The setting

**Files:**
- Modify: `crates/core/src/settings.rs:10-26` (field and `Default`)

**Interfaces:**
- Consumes: nothing.
- Produces: `Settings::update_check: Option<bool>`.

- [ ] **Step 1: Write the failing tests**

In the existing `mod tests` of `crates/core/src/settings.rs`:

```rust
    /// `None` is "the user has not been asked yet", not "off". Until the
    /// question has been answered nothing goes on the network, and the
    /// interface knows from this field that it still has to ask.
    #[test]
    fn the_update_question_starts_unanswered() {
        assert_eq!(Settings::default().update_check, None);
    }

    /// A settings file written by an older version has no such key. It
    /// must read as unanswered rather than as an error, or an update
    /// would lock the user out of their own configuration.
    #[test]
    fn a_settings_file_without_the_key_reads_as_unanswered() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "auto_backup = false\n").unwrap();
        let settings = Settings::load(&path).unwrap();
        assert_eq!(settings.update_check, None);
        assert!(!settings.auto_backup);
    }

    #[test]
    fn an_answered_question_survives_a_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        let mut settings = Settings::default();
        settings.update_check = Some(true);
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap().update_check, Some(true));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p sm2-core settings::`
Expected: FAIL, `no field update_check`.

- [ ] **Step 3: Write the implementation**

In `crates/core/src/settings.rs`, in `pub struct Settings` after
`language`:

```rust
    /// Whether to look for a newer version on start. `None` until the
    /// user has been asked once — the same shape as `language`, and for
    /// the same reason: the absent value means "not decided", not "no".
    /// Nothing goes on the network while this is `None`.
    pub update_check: Option<bool>,
```

And in `Default`:

```rust
        Self { game_dir: None, auto_backup: true, steam_user: None, language: None, update_check: None }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p sm2-core settings::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/settings.rs
git commit -m "feat(update): a setting for the automatic check"
```

---

### Task 9: The `update` subcommand

**Files:**
- Modify: `crates/app/src/cli.rs` (enum at `:31`, `requires_exclusive_access`
  at `:202-225`, `run_command`'s match at `:292`, new function next to
  `run_lang_command` at `:668`)
- Modify: `crates/core/i18n/en.toml`, `crates/core/i18n/de.toml`

**Interfaces:**
- Consumes: `update::{Endpoints, Availability, check, install, remember, NET_TIMEOUT}`,
  `Version`.
- Produces: `Command::Update { check: bool }`,
  `EXIT_UPDATE_AVAILABLE: i32 = 10`.

- [ ] **Step 1: Write the failing tests**

In the test module of `crates/app/src/cli.rs`:

```rust
    /// Checking reads; installing writes the binary and must not race a
    /// second launcher doing the same. The match in
    /// `requires_exclusive_access` has no `_` arm, so this decision has
    /// to be made before the crate compiles — this test records which
    /// way it went.
    #[test]
    fn checking_needs_no_lock_but_installing_does() {
        assert!(!requires_exclusive_access(&Command::Update { check: true }));
        assert!(requires_exclusive_access(&Command::Update { check: false }));
    }

    #[test]
    fn the_update_command_and_its_flag_have_catalogue_keys() {
        // Covered generally by `every_command_and_argument_has_a_key`;
        // named here so a failure points at this task.
        let _guard = crate::app_state::language_test_lock();
        sm2_core::i18n::set_language(sm2_core::i18n::Language::English);
        assert_ne!(sm2_core::i18n::lookup("cli.update.about"), "cli.update.about");
        assert_ne!(sm2_core::i18n::lookup("cli.update.arg.check"), "cli.update.arg.check");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p lina-sm2 cli::`
Expected: FAIL, `no variant named Update`.

- [ ] **Step 3: Write the implementation**

In `crates/app/src/cli.rs`, in `enum Command` after `Lang`:

```rust
    /// Looks for a new version and installs it
    Update {
        /// Only report, do not install
        #[arg(long)]
        check: bool,
    },
```

In `requires_exclusive_access`, next to the `Lang` arm:

```rust
        // Checking only reads; installing replaces the binary and must
        // not run twice at once.
        Command::Update { check } => !check,
```

In `run_command`'s match:

```rust
        Command::Update { check } => run_update_command(state, check)?,
```

Next to `run_lang_command`:

```rust
/// What `update --check` exits with when there is something to install.
/// Not 1: `main` maps every error to 1 already, and a script has to be
/// able to tell "a new version exists" from "the check failed".
const EXIT_UPDATE_AVAILABLE: i32 = 10;

fn run_update_command(state: &AppState, check: bool) -> Result<()> {
    let endpoints = update::Endpoints::from_env();
    let found = update::check(&endpoints, update::NET_TIMEOUT)?;

    // Written before anything is printed or exited, so that a later
    // `std::process::exit` cannot skip it.
    let seen = match found {
        Availability::UpToDate { current } => current,
        Availability::Newer { latest, .. } | Availability::Ahead { latest, .. } => latest,
    };
    if let Err(e) = update::remember(&state.dirs, seen) {
        // A cache that cannot be written costs one request next time and
        // is not worth failing a check the user asked for.
        tracing::warn!("update cache not written: {e}");
    }

    let Some(latest) = found.newer() else {
        println!("{}", t!("cli.update.up_to_date", version = Version::running()));
        return Ok(());
    };

    if check {
        println!("{}", t!("cli.update.available", version = latest));
        // Nothing is held at this point: `--check` takes no instance
        // lock, and the cache above is already on disk.
        std::process::exit(EXIT_UPDATE_AVAILABLE);
    }

    println!("{}", t!("cli.update.installing", version = latest));
    update::install(&endpoints, latest)?;
    println!("{}", t!("cli.update.installed", version = latest));
    println!("{}", t!("cli.update.restart_needed"));
    Ok(())
}
```

Add to the imports at the top of `cli.rs`:

```rust
use sm2_core::update::{self, Availability, Version};
```

In `crates/core/i18n/en.toml`, after the `[cli.lang.arg]` block:

```toml
[cli.update]
about = "Looks for a new version and installs it"
up_to_date = "{version} is the newest version."
available = "Version {version} is available."
installing = "Installing {version} …"
installed = "{version} installed."
restart_needed = "Please restart the launcher."

[cli.update.arg]
check = "Only report, do not install"
```

In `crates/core/i18n/de.toml`, the same keys:

```toml
[cli.update]
about = "Sucht nach einer neuen Version und installiert sie"
up_to_date = "{version} ist die neueste Version."
available = "Version {version} ist verfügbar."
installing = "Installiere {version} …"
installed = "{version} installiert."
restart_needed = "Bitte den Launcher neu starten."

[cli.update.arg]
check = "Nur berichten, nicht installieren"
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test` then `cargo clippy --all-targets`
Expected: PASS, including `every_command_and_argument_has_a_key` and both
i18n parity tests.

- [ ] **Step 5: Try it by hand**

Run: `cargo run -- update --check; echo "exit $?"`
Expected: either "0.4.0 is the newest version." and `exit 0`, or a
version line and `exit 10`.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src/cli.rs crates/core/i18n/
git commit -m "feat(update): an update subcommand for the command line"
```

---

### Task 10: The interface's state and the question on first start

**Files:**
- Create: `crates/app/src/gui/update.rs`
- Modify: `crates/app/src/gui/mod.rs` (module list, `App` field at
  `:282-332`, `Dialog` at `:159-179`, `Action` at `:231-279`, the action
  handler at `:669`)
- Modify: `crates/app/src/gui/dialogs.rs`
- Modify: `crates/core/i18n/en.toml`, `crates/core/i18n/de.toml`

**Interfaces:**
- Consumes: `update::{Endpoints, Availability, CheckCache, check, remember, now_seconds, NET_TIMEOUT}`,
  `Settings::update_check`.
- Produces:
  - `gui::update::UpdateUi` with `known: Option<Availability>`,
    `start_check(&mut self, ctx: &egui::Context, endpoints: Endpoints)`,
    `poll(&mut self) -> Option<Result<Availability>>`,
    `is_busy(&self) -> bool`, `has_news(&self) -> bool`,
    `adopt_cache(&mut self, cache: &CheckCache)`
  - `Action::{CheckForUpdates, InstallUpdate, ToggleUpdateCheck, AnswerUpdateQuestion(bool)}`
  - `Dialog::UpdateQuestion`

- [ ] **Step 1: Write the failing tests**

Create `crates/app/src/gui/update.rs` with the test module only:

```rust
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
        let mut ui = UpdateUi::default();
        ui.known = Some(Availability::Newer {
            current: Version::new(0, 4, 0),
            latest: Version::new(0, 5, 0),
        });
        assert!(ui.has_news());
    }

    /// A development build is newer than every release. Marking that as
    /// news would put a dot on the sidebar that offers a downgrade.
    #[test]
    fn being_ahead_is_not_news() {
        let mut ui = UpdateUi::default();
        ui.known = Some(Availability::Ahead {
            current: Version::new(9, 9, 9),
            latest: Version::new(0, 5, 0),
        });
        assert!(!ui.has_news());
    }

    #[test]
    fn being_up_to_date_is_not_news() {
        let mut ui = UpdateUi::default();
        ui.known = Some(Availability::UpToDate { current: Version::new(0, 4, 0) });
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Add `mod update;` to the module list at the top of
`crates/app/src/gui/mod.rs` first, then:

Run: `cargo test -p lina-sm2 gui::update`
Expected: FAIL, `cannot find struct UpdateUi`.

- [ ] **Step 3: Write the implementation**

In `crates/app/src/gui/update.rs`, above the tests:

```rust
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
```

In `crates/app/src/gui/mod.rs`:

- add the imports the helpers below need:

```rust
use sm2_core::platform::{Current, Platform, UpdateMethod};
use sm2_core::update::{self, Availability, CheckCache, Endpoints};
```

  (`Current` and `Platform` are only used by `start_update_install` in
  Task 11; add them there if this task is committed on its own, or
  clippy reports them unused.)

- add `update: update::UpdateUi,` to `pub struct App` (next to `task`) and
  `update: Default::default(),` in `blank()`;
- add to `pub enum Dialog`:

```rust
    /// The "may I look for updates on start?" question, shown once on
    /// the first start (`gui.dialog.update_ask_title`). It is a dialog
    /// and not a silent default because it is the only thing in this
    /// program that talks to the network.
    UpdateQuestion,
```

- add to `pub enum Action`:

```rust
    CheckForUpdates,
    InstallUpdate,
    ToggleUpdateCheck,
    AnswerUpdateQuestion(bool),
```

- handle them next to `Action::ToggleAutoBackup => self.toggle_auto_backup()`:

```rust
            Action::CheckForUpdates => self.start_update_check(),
            Action::InstallUpdate => self.start_update_install(),
            Action::ToggleUpdateCheck => {
                let on = self.settings().update_check.unwrap_or(false);
                self.settings_mut().update_check = Some(!on);
                self.save_settings();
            }
            Action::AnswerUpdateQuestion(yes) => {
                self.settings_mut().update_check = Some(yes);
                self.save_settings();
                self.dialog = None;
                if yes {
                    self.start_update_check();
                }
            }
```

- and the two helpers plus the start-up hook, next to `save_settings`:

```rust
    /// A check the user asked for. It runs whatever the setting says —
    /// pressing the button *is* the permission, and it is the only way
    /// to check at all while the question is still unanswered.
    fn start_update_check(&mut self) {
        self.set_busy(t!("gui.settings.update_checking"));
        self.update.start_check(&self.egui_ctx, Endpoints::from_env());
    }

    /// Runs once the first frame is up, from the same place that ends
    /// the splash. Three things have to be true: the user said yes, no
    /// check is running, and the last one is older than a day.
    fn maybe_check_for_updates(&mut self) {
        if self.settings().update_check != Some(true) {
            return;
        }
        let Some(dirs) = self.dirs.clone() else { return };
        let cache = CheckCache::load(&CheckCache::path(&dirs));
        if let Some(cache) = &cache {
            self.update.adopt_cache(cache);
            if cache.is_fresh(update::now_seconds()) {
                return;
            }
        }
        self.update.start_check(&self.egui_ctx, Endpoints::from_env());
    }

    /// Reads the answer of a running check, if one has arrived.
    fn poll_update_check(&mut self) {
        let Some(outcome) = self.update.poll() else { return };
        match outcome {
            Ok(found) => {
                if let Some(dirs) = &self.dirs {
                    let seen = match found {
                        Availability::UpToDate { current } => current,
                        Availability::Newer { latest, .. } | Availability::Ahead { latest, .. } => {
                            latest
                        }
                    };
                    if let Err(e) = update::remember(dirs, seen) {
                        tracing::warn!("update cache not written: {e}");
                    }
                }
                match found.newer() {
                    Some(latest) => {
                        self.set_status(t!("gui.message.update_available", version = latest))
                    }
                    None => self.set_status(t!("gui.settings.update_up_to_date")),
                }
            }
            // Loud, because only a check the user pressed for or one
            // they switched on can get here, and a silent failure would
            // look like "no update exists".
            Err(e) => self.set_warning(t!("gui.message.update_check_failed", detail = e)),
        }
    }
```

Call `poll_update_check()` once per frame in `App::update`, next to where
a running task is polled. Call `maybe_check_for_updates()` where the
splash hands over, and open the dialog there too:

```rust
        if self.settings().update_check.is_none() {
            self.dialog = Some(Dialog::UpdateQuestion);
        } else {
            self.maybe_check_for_updates();
        }
```

In `crates/app/src/gui/dialogs.rs`, add the `Dialog::UpdateQuestion` arm
following the shape of the existing `Dialog::Vanilla` one: title
`gui.dialog.update_ask_title`, body `gui.dialog.update_ask_body`, and two
buttons pushing `Action::AnswerUpdateQuestion(true)` and
`Action::AnswerUpdateQuestion(false)`.

New catalogue keys — `crates/core/i18n/en.toml`:

```toml
# in [gui.dialog]
update_ask_title = "Look for updates?"
update_ask_body = "The launcher can ask github.com once a day whether a newer version exists. It never installs anything on its own – you decide with a button. Without this it makes no network connections at all."
update_ask_yes = "Yes, on every start"
update_ask_no = "No"

# in [gui.message]
update_available = "Version {version} is available."
update_check_failed = "Update check failed: {detail}"
update_installed = "{version} installed – please restart the launcher."
```

`crates/core/i18n/de.toml`:

```toml
# in [gui.dialog]
update_ask_title = "Nach Updates sehen?"
update_ask_body = "Der Launcher kann einmal täglich bei github.com nachfragen, ob es eine neuere Version gibt. Installiert wird nie von allein – das entscheidest du per Knopf. Ohne dies baut er überhaupt keine Netzverbindung auf."
update_ask_yes = "Ja, bei jedem Start"
update_ask_no = "Nein"

# in [gui.message]
update_available = "Version {version} ist verfügbar."
update_check_failed = "Update-Prüfung fehlgeschlagen: {detail}"
update_installed = "{version} installiert – bitte den Launcher neu starten."
```

`gui.settings.update_checking` and `gui.settings.update_up_to_date` are
added in Task 11 together with the rest of that block; add them here if
Task 11 has not run yet, since `t!` on a missing key fails the suite.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test` then `cargo clippy --all-targets`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/gui/ crates/core/i18n/
git commit -m "feat(update): ask once, then check on start"
```

---

### Task 11: The settings block, the dot, and the install button

**Files:**
- Modify: `crates/app/src/gui/settings_page.rs` (a block in the same
  style as the `auto_backup` row at `:196-228`)
- Modify: `crates/app/src/gui/side_bar.rs:10-40` and `:92-99`
- Modify: `crates/app/src/gui/tasks.rs` (the installation as a job)
- Modify: `crates/core/i18n/en.toml`, `crates/core/i18n/de.toml`

**Interfaces:**
- Consumes: `UpdateUi`, the four actions from Task 10,
  `update::install`, `Current::update_method()`, `Platform::open_url`.
- Produces: `App::start_update_install()`.

- [ ] **Step 1: Write the failing test**

In the test module of `crates/app/src/gui/update.rs`:

```rust
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
```

- [ ] **Step 2: Run the test to verify it fails or passes**

Run: `cargo test -p lina-sm2 gui::update`
Expected: PASS already (Task 6 did the work) — it is here as the guard on
the branch written in step 3.

- [ ] **Step 3: Write the installation path**

In `crates/app/src/gui/mod.rs`, next to `start_update_check`:

```rust
    /// The user pressed "update". On a platform without an installer
    /// this is where the release page is opened instead — the platform
    /// decides, not a `cfg` here.
    fn start_update_install(&mut self) {
        let Some(latest) = self.update.known.and_then(|found| found.newer()) else { return };
        let endpoints = Endpoints::from_env();
        if Current::update_method() == UpdateMethod::ReleasePage {
            if let Err(e) = Current::open_url(&endpoints.release_page(latest)) {
                self.set_warning(e.to_string());
            }
            return;
        }
        self.task = Some(tasks::start_update_install(&self.egui_ctx, endpoints, latest));
    }
```

In `crates/app/src/gui/tasks.rs`, add the job following the shape of the
existing ones: a `Outcome::Updated { version }` variant, a
`start_update_install` that spawns the thread, sets the progress label
from `gui.settings.update_installing`, and is **not** cancellable —
aborting `install.sh` between two of its steps is exactly what its own
fsync-before-rename ordering exists to avoid. On success the handler in
`mod.rs` posts `gui.message.update_installed`.

- [ ] **Step 4: Write the settings block**

In `crates/app/src/gui/settings_page.rs`, after the `auto_backup` row and
before the Steam user row, a block in the same idiom: a heading row
`gui.settings.update_title`, two value rows (`update_row_installed` with
`Version::running()`, `update_row_available` with either the found
version, `gui.settings.update_up_to_date`, `gui.settings.update_checking`
while `app.update.is_busy()`, or `gui.settings.no_value`), a
`widgets::button` row with `update_button` (enabled only when
`app.update.has_news()` and no task is running) pushing
`Action::InstallUpdate` and `update_check_button` (disabled while
`app.update.is_busy()`) pushing `Action::CheckForUpdates`, and a toggle
row like the `auto_backup` one bound to
`app.settings().update_check.unwrap_or(false)` pushing
`Action::ToggleUpdateCheck`, with `update_auto_title` and
`update_auto_body`.

- [ ] **Step 5: Write the dot**

In `crates/app/src/gui/side_bar.rs`, give `nav_item` a `dot: bool`
parameter after `count`, and paint a filled circle in `color::ACCENT`
with radius 3 at the right edge of the row when it is set. Pass
`app.update.has_news()` for `Section::Settings` and `false` for the other
three. A comment says why it is not the count slot: a count is a number
the user reads, this is a mark that something is waiting.

- [ ] **Step 6: Add the catalogue keys**

`crates/core/i18n/en.toml`, in `[gui.settings]`:

```toml
update_title = "Update"
update_row_installed = "Installed"
update_row_available = "Available"
update_up_to_date = "up to date"
update_checking = "checking …"
update_installing = "installing the new version …"
update_button = "Update"
update_check_button = "Check now"
update_auto_title = "Look for updates on start"
update_auto_body = "Asks github.com at most once a day. Nothing is installed without your click."
```

`crates/core/i18n/de.toml`:

```toml
update_title = "Update"
update_row_installed = "Installiert"
update_row_available = "Verfügbar"
update_up_to_date = "aktuell"
update_checking = "prüfe …"
update_installing = "installiere die neue Version …"
update_button = "Updaten"
update_check_button = "Jetzt prüfen"
update_auto_title = "Beim Start nach Updates sehen"
update_auto_body = "Fragt höchstens einmal täglich bei github.com nach. Installiert wird nichts ohne Klick."
```

- [ ] **Step 7: Run everything**

Run: `cargo test` then `cargo clippy --all-targets`
Expected: PASS.

- [ ] **Step 8: Try it by hand**

Run: `cargo run`
Expected: the question on the first start (delete
`~/.config/lina-sm2/settings.toml` to see it again), then the block under
Settings, and a dot next to the sidebar entry when a newer release
exists.

- [ ] **Step 9: Commit**

```bash
git add crates/app/src/gui/ crates/core/i18n/
git commit -m "feat(update): the update block on the settings page"
```

---

### Task 12: Documentation

**Files:**
- Modify: `README.md` (next to the install one-liner at `:21-50`)
- Modify: `CLAUDE.md` (a paragraph in "Safety rules that the code depends
  on", and the asset list under "Packaging")

**Interfaces:**
- Consumes: everything above.
- Produces: nothing code depends on.

- [ ] **Step 1: Write the README section**

Under the installation section, a short passage: the launcher can look
for updates itself, it asks once on the first start whether it may, the
setting and both buttons live under Settings, `update --check` and
`update` do the same from the command line with exit code 10 meaning "a
new version exists", and on Windows the button opens the release page
because there is no installer there.

- [ ] **Step 2: Write the CLAUDE.md additions**

In "Safety rules that the code depends on":

> The launcher makes no network connection until the user has answered
> the question on first start, and installs nothing without a click.
> `Settings::update_check` is `None` until then, and `None` means
> "unanswered", not "off". `update::install` never runs a script it has
> not verified against the release's `SHA256SUMS` — which is why
> `install.sh` is a release asset.

In "Packaging", extend the `release.yml` paragraph to name `install.sh`
as the fourth asset the three places have to agree on.

- [ ] **Step 3: Run the suite one last time**

Run: `cargo test && cargo clippy --all-targets`
Expected: PASS, no warnings.

- [ ] **Step 4: Commit**

```bash
git add README.md CLAUDE.md
git commit -m "docs: the update check and what it promises"
```

---

## Self-review against the spec

Checked section by section:

- Goals, non-goals — Tasks 9, 10, 11 cover the user-facing half; the
  non-goals stay out (no auto-install, no restart, no Windows installer,
  no channels).
- `ureq`, `sha2`, core placement, exit codes — Tasks 4, 7, 9.
- `Version`, `Availability`, `check`, `install` — Tasks 1, 4, 7.
- Platform split and `open_url` — Task 6.
- Repository slug in one place — Task 2.
- Cache — Task 5, wired in Task 10.
- Settings and the first-start question — Tasks 8 and 10.
- Interface (block, dot, own module, own channel, `tasks.rs` for the
  installation) — Tasks 10 and 11.
- Command line and `requires_exclusive_access` — Task 9.
- Errors — Task 3.
- Release workflow — Task 7.
- Testing and catalogue keys — spread across the task that introduces
  each key, so the suite is green after every commit.

One thing the spec names that no task implements: nothing. One thing the
tasks add that the spec does not name: the four refinements listed under
"Deviations from the spec" above.

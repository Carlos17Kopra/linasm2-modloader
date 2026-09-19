# Update check and user-triggered self-update

Date: 2026-09-19
Status: approved, ready for an implementation plan

## What this is

The launcher learns to ask GitHub whether a newer release exists, to say
so, and — on a button the user presses — to install it by running the
release's own `install.sh`. Nothing installs itself: every network
request and every installation is something the user switched on or
clicked.

Today the program has no network code and no network dependency at all.
The whole update story lives in `install.sh`, which the README tells
people to re-run by hand.

## Goals

- Tell the user that a newer version exists, without being asked twice.
- Let the user install it from inside the launcher, on Linux.
- Keep the command line equal to the interface: `update --check` and
  `update`.
- Never touch the network without a decision the user made.

## Non-goals

- Automatic installation. The check may run on its own; the
  installation never does.
- Restarting the launcher after an update. It says "please restart".
- A self-update on Windows. There is no `install.sh` there; the button
  opens the release page.
- Pre-releases, channels, downgrades. `install.sh --version` already
  covers going back, and it is a command line matter.

## Decisions

**`ureq` 3.4 with rustls** (default features) as the HTTP client. It
blocks, which fits `gui/tasks.rs`, where everything already runs on
plain threads and nothing is async. It brings its own root
certificates, so the check does not depend on what is installed on the
machine. The alternative — shelling out to `curl`, which `install.sh`
requires anyway — was rejected: it would turn network failures into
exit codes and stderr text that have to be translated back into
sentences.

**`sha2` 0.11** for verifying the downloaded installer. The project
hashes with BLAKE3, but the release publishes `SHA256SUMS`, and that is
the file the installer's own verification is built on. A second hash
file next to it would be worse than a small pure-Rust dependency.

Both go into `[workspace.dependencies]` and into `sm2-core`, not into
the app. Both declare `rust-version = 1.85`, which is exactly the core
crate's MSRV — it does not have to move, but there is no headroom left
either, and a future bump of theirs becomes a bump of ours.

**The check lives in `sm2-core`**, not in the app. It is domain logic:
what the current version is, what the latest one is, and which of the
two wins.

**Exit codes: 0 up to date, 10 an update is available, 1 an error.**
Not the `diff` convention of 0/1/2, because `main.rs:55` already maps
every error to 1; 1 cannot mean two things. 10 is out of the way of
everything `main` does today.

## The module: `crates/core/src/update.rs`

### `Version`

```rust
pub struct Version(u16, u16, u16);
```

`FromStr` accepts `1.2.3` and `v1.2.3`, rejects everything else. `Ord`
compares field by field. No `semver` crate for three numbers — the case
that matters is `0.10.0 > 0.9.0`, exactly what `install.sh` needs
`sort -V` for.

The running version comes from `CARGO_PKG_VERSION`: the same number
`--version` prints and `install.sh` reads back out of the installed
binary.

### `Availability`

```rust
pub enum Availability {
    UpToDate { current: Version },
    Newer { current: Version, latest: Version },
    Ahead { current: Version, latest: Version },
}
```

`Ahead` is a locally built binary that is newer than any release. It is
its own case so that the interface can stay quiet instead of offering a
downgrade.

### `check`

```rust
pub fn check(api_base: &str, timeout: Duration) -> Result<Availability>
```

`GET {api_base}/repos/{REPO}/releases/latest`, with
`Accept: application/vnd.github+json` and
`User-Agent: {APP_SLUG}/{CARGO_PKG_VERSION}` — GitHub refuses requests
without one. `serde_json` (already a workspace dependency) reads
`tag_name`; everything else in the answer is ignored. Connect timeout 5
seconds, whole request 10.

`api_base` is a parameter rather than a constant so the tests can point
it at a local listener. Production passes
`LINA_SM2_API_BASE` if set, else `https://api.github.com` — the same
variable name `install.sh:35` already uses, so both sides of the
release can be redirected the same way.

The other two bases follow the same rule, with the same names the
script uses where it has one:

| base            | environment variable      | default                                        |
| --------------- | ------------------------- | ---------------------------------------------- |
| `api_base`      | `LINA_SM2_API_BASE`       | `https://api.github.com`                        |
| `download_base` | `LINA_SM2_DOWNLOAD_BASE`  | `https://github.com/{REPO}/releases/download`   |
| `raw_base`      | `LINA_SM2_RAW_BASE`       | `https://raw.githubusercontent.com`             |

`LINA_SM2_RAW_BASE` is new; the script has no counterpart because it
never fetches itself. One function resolves all three, so the tests get
at them the same way.

### `install`

```rust
pub fn install(version: Version) -> Result<()>
```

1. Fetch `SHA256SUMS` of that release from
   `{download_base}/v{version}/SHA256SUMS`.
2. Fetch `install.sh` from
   `{raw_base}/{REPO}/v{version}/install.sh`.
3. Hash the script with SHA-256 and compare against the `install.sh`
   line of `SHA256SUMS`. A missing line is a failure, not a skip.
4. Write it into a temporary directory (`tempfile`, already a
   dependency).
5. Run `sh <script> --version {version}`, capturing stdout and stderr.
6. Exit code 0 is success. Anything else is
   `UpdateDefect::InstallerFailed { code, tail }`, where `tail` is the
   last few lines of the captured output.

Steps 1–3 are why `install.sh` has to become a release asset (see
"Release workflow" below). The trust anchor stays HTTPS to GitHub,
the same one the documented `curl … | sh` line rests on — an
independent signature would be theatre while both come from the same
place. The hash is there for the failure mode that is real: a truncated
download that half-installs when executed.

The running binary is replaced by a rename inside `install.sh`, so the
running process keeps its inode and is unaffected. That is why "please
restart" is a message and not a forced restart.

**Amendment.** That paragraph assumes the running binary *is* the one
`install.sh` replaces, and nothing established it. The script writes
exactly one path, `${LINA_SM2_BIN_DIR:-$HOME/.local/bin}/lina-sm2`. A
launcher started from a `cargo` build, a distribution package or a
folder someone unpacked elsewhere is not that file: the installation
would succeed, leave a *second* and newer copy in `~/.local/bin`, and
the restart it asks for would come back on the old version with no
error anywhere.

So step 0 of `install` is `update::can_install_in_place()`, which
resolves `std::env::current_exe()` and the managed path through their
symlinks and compares them; a path that cannot be resolved counts as
not managed. A refusal is `UpdateDefect::NotTheManagedBinary { url }`,
and the interface does not show it at all — it opens the release page,
exactly as it does where the platform has no installer. Someone with an
unusual but genuinely managed installation is sent to the release page
for nothing, which is the cheap direction to be wrong in.

Deliberately *not* folded into `update_method()`: which binary happens
to be running is not a property of the platform.

## Platform split

Two additions to `core::platform::Platform`, so that nothing above it
carries a `cfg`:

```rust
/// How a found update is applied here.
fn update_method() -> UpdateMethod;   // Installer | ReleasePage

/// Open a URL in the user's browser.
fn open_url(url: &str) -> Result<()>;
```

`UpdateMethod` lives next to the trait in `core::platform`, not in
`update.rs`: it describes the platform, and `update.rs` reads it.

`unix.rs` returns `Installer` and opens URLs with `xdg-open`, like
`open_folder` does. `windows.rs` returns `ReleasePage` and opens URLs
with the shell. `ReleasePage` sends the user to
`https://github.com/{REPO}/releases/tag/v{version}`.

This is the one place where the platforms genuinely behave
differently — not a fixture limitation. Tests that cover the installer
path are therefore `#[cfg(unix)]` for a real reason, and must say so.

## The repository slug

`install.sh:28` spells out `Carlos17Kopra/linasm2-modloader`. Rust
cannot read it from there, so it would exist twice — the thing CLAUDE.md
forbids for the program's name.

`REPO` becomes a constant in `crates/core/src/branding.rs`, and a test
`install_sh_and_branding_agree_on_the_repository` reads the script and
compares. Same construction as the existing coupling between
`StartupWMClass` and `APP_SLUG`.

## The cache

The automatic check runs at most once every 24 hours. State directory,
`update-check.toml`:

```toml
last_checked = 1758271234
latest_seen = "0.5.0"
```

Written with `atomic::write_atomic`, like everything else this program
writes. Not in `Settings`: that file is the user's configuration and
would be rewritten on every start.

The rate limit (60 unauthenticated requests per hour and IP) is the
lesser reason. The real one is that the dot on the sidebar entry
survives a start without a network. A cache file that cannot be read or
parsed counts as absent, never as an error.

## Settings and the question on first start

```rust
/// Whether to look for updates on start. `None` until the user has
/// been asked once — see the dialog in `gui/update.rs`.
pub update_check: Option<bool>,
```

The same shape as `language: Option<String>`, and for the same reason:
the absent value means "not decided yet", not "off".

`None` on the first start after the splash opens a modal dialog asking
whether the launcher may look for updates on start. Either answer
writes `Some(..)` and the dialog never comes back. Until it is
answered, nothing goes on the network.

The setting governs the **automatic** check only. "Jetzt prüfen" and
`update --check` are the user asking in so many words, and they work
whatever the setting says — including while it is still `None`, which
is also the only way the command line ever checks, since the CLI never
opens the dialog. Both write the cache like the automatic check does.

## The interface

A block on the settings page, and a dot on the sidebar's settings entry
when an update is known:

```
┌──────────────┬────────────────────────────────┐
│  Mods        │  Update                        │
│  Profile     │                                │
│  Savegames   │  Installiert   0.4.0           │
│  Einstell. ● │  Verfügbar     0.5.0           │
│              │                                │
│              │  [ Updaten ]  [ Jetzt prüfen ] │
│              │                                │
│              │  ☑ Beim Start nach Updates     │
│              │    sehen                       │
├──────────────┴────────────────────────────────┤
```

The feature's own state lives in a new `crates/app/src/gui/update.rs`:
what is known, the receiver of a running check, the receiver of a
running installation, and the captured installer output.
`gui/mod.rs` is 1298 lines already and gets a field, not a feature.

The check does **not** go through `gui/tasks.rs`. That machinery locks
activation, ordering and import through `App::can_modify`, because a
job there works on copies of library and configuration. A check changes
nothing and must not freeze the window's editing — it gets a small
`mpsc` channel of its own, polled once per frame.

The installation does go through `tasks.rs`: it is long, it has output
worth showing, and a second one must not start while it runs.

`Ahead` shows nothing at all. `UpToDate` shows the installed version
and a quiet "up to date".

## The command line

```rust
Command::Update { check: bool }
```

- `update --check` prints the verdict and exits 0, 10 or 1.
- `update` installs, or prints the release URL and exits 1 where it
  cannot: on Windows, and on a copy the installer does not manage
  (see the amendment above).

`requires_exclusive_access` gets `Command::Update { check } => !check`.
Checking changes nothing; installing does. The exhaustive match without
a `_` arm means this has to be decided before it compiles, which is the
point.

## Errors

A new variant in `core::error::Error`, built like `BackupDefect` and
`ArchiveDefect` so the detail is translatable:

```rust
Update(UpdateDefect)

pub enum UpdateDefect {
    Unreachable,                              // no route, DNS, timeout
    HttpStatus(u16),
    MalformedAnswer,
    ChecksumMismatch,
    NoInstallerForPlatform,
    NotTheManagedBinary,                      // see the amendment above
    InstallerFailed { code: i32, tail: String },
}
```

A sketch: the variants that name something carry it as a field in the
implementation — the URL to open, the version whose checksum did not
match, the detail of an unreachable host.

A failure of the automatic check is silent — a `tracing` line, nothing
in the interface. A failure of a check the user asked for is loud:
whoever presses the button wants an answer.

## Release workflow

`install.sh` joins the release assets in `.github/workflows/release.yml`,
so that `SHA256SUMS` covers it. This is the fourth name in the set that
`release.yml`, `install.sh` and `packaging/test-install.sh` have to
agree on; `crates/app/tests/install_script.rs` is what notices when they
stop agreeing.

## Testing

Tests first, and no test touches the network. `check` takes its base
URL as a parameter precisely so the tests can start a `TcpListener` on
`127.0.0.1` and serve a canned HTTP answer.

Core:

- `Version`: parsing with and without `v`, rejection of junk, and
  `0.10.0 > 0.9.0`.
- `check`: up to date, newer, ahead, malformed JSON, HTTP 404, a
  connection that is accepted and then never answers (timeout).
- Cache: a fresh file suppresses the request, a stale one allows it, a
  corrupt one counts as absent.
- `install_sh_and_branding_agree_on_the_repository`.
- Checksum mismatch rejects the script and does not execute it.

App:

- `requires_exclusive_access` for both forms of the new command.
- `update --check` wording in both languages, holding
  `crate::app_state::language_test_lock()`.
- Exit codes 0 / 10.
- The i18n tests cover the new keys on their own, including
  `cli.update.arg.check`.

Everything that runs the installer is `#[cfg(unix)]` because the script
does not exist on Windows — real platform behaviour, not one of the
three fixture gaps CLAUDE.md lists.

## Catalogue keys

New in `en.toml` and `de.toml`, identical key sets:

- `[cli.update]`: `about`, `up_to_date`, `available`, `installing`,
  `installed`, `restart_needed`, `no_installer`
- `[cli.update.arg]`: `check`
- `[gui.settings]`: `update_title`, `update_installed`,
  `update_available`, `update_current`, `update_button`,
  `update_check_button`, `update_auto_title`, `update_auto_body`,
  `update_checking`, `update_restart_needed`
- `[gui.dialog]`: `update_ask_title`, `update_ask_body`,
  `update_ask_yes`, `update_ask_no`
- `[error.update]`: one line per `UpdateDefect` variant

None of these are labels that end up on a disk, so none of them belong
under `[label]` or need the historical-wording test.

## What would be cut first

The cache. Without it the launcher checks on every start and knows
nothing when it starts offline; everything else keeps working. It is
the one part of this design that buys convenience rather than
correctness.

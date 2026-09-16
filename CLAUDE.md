# SM2 Mod Loader

A mod loader for Space Marine 2 on Linux. Rust workspace, two crates:

- `crates/core` (`sm2-core`) — all domain logic, no UI. MSRV 1.85.
- `crates/app` (`sm2-modloader`) — one binary that is both GUI and CLI. Started
  with no arguments it opens the egui interface, with arguments it runs the
  command line. MSRV 1.95 (egui 0.36 requires it).

## Language

**Code comments are English. User-facing text is German.**

That split is deliberate and applies to everything you write here:

- English: `//`, `///`, `//!` and block comments, test function names,
  `assert!` messages, and anything else only a developer reads.
- German: GUI labels, status and notice messages, `Error` variants'
  `#[error("…")]` strings, `bail!`/`println!` output, and the clap `///` doc
  comments that become `--help` text. The program speaks German to its users.

Note the trap in `crates/app/src/cli.rs` and `crates/core/src/error.rs`: doc
comments on clap items and strings inside `#[error(...)]` are *program output*
despite looking like ordinary Rust. They stay German.

Comment prose is wrapped at 78 columns including the `///` prefix.

## Comment style

Comments here explain *why*, not *what*. A comment that restates the code
below it is noise; a comment that names the failure mode a guard prevents, or
the alternative that was rejected and what would have broken, earns its place.
Match that when adding code — especially around `saves.rs`, where the ordering
of filesystem operations is the entire safety argument.

## Safety rules that the code depends on

- Savegames live in a Proton prefix that Steam Cloud can overwrite at any
  moment. `saves::restore` always takes and verifies its own backup first;
  that is not optional and not configurable.
- Archive and manifest are written fsync-before-rename, and every filesystem
  sequence is ordered so that a crash between two steps leaves a readable
  state — never a half-written one.
- Symlinks inside a save or extraction directory are never followed.

## Working on it

    cargo test                  # 232 tests across both crates
    cargo clippy --all-targets  # kept clean
    cargo run                   # GUI
    cargo run -- <subcommand>   # CLI

Write tests first. The existing suite was built that way and the failure modes
it covers (collisions in the same second, corrupt archives, zip-slip, symlink
cycles) are the reason this tool can be trusted with real save data.

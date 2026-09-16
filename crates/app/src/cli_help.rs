//! Puts the catalogue's texts into the `clap` command tree.
//!
//! `clap`'s derive takes its help from doc comments, which are `&'static
//! str` and therefore cannot change with the language. So the tree is
//! walked once before parsing and `localize` overwrites every `about`
//! and `help` unconditionally with a catalogue lookup. The doc comments
//! are not a runtime fallback: if a key is ever missing,
//! `i18n::lookup`'s own fallback chain (active language → English →
//! the raw key) means the user sees the dotted key itself (e.g.
//! `cli.play.arg.foo`), not the doc comment.
//! `every_command_and_argument_has_a_key` below exists so that a missing
//! key is caught in `cargo test`, not by a user reading `--help`.

use sm2_core::i18n::{self, Language};

/// The keys for clap's own `-h`/`--help` and `-V`/`--version` — currently
/// unreachable dead entries, kept only as the documented, deliberate
/// limitation described below.
///
/// clap adds its default `-h`/`--help` (every level) and `-V`/`--version`
/// (root only) while *building* the command for parsing, which happens
/// after this walk has already run over the still-unbuilt tree (see the
/// module doc comment above) — so `"help"`/`"version"` are never in
/// `command.get_arguments()` here, and `mut_arg` is never called for
/// them.
///
/// The seemingly cheap fix — `Cli::command().disable_help_flag(true)`/
/// `.disable_version_flag(true)` plus declaring `-h`/`--help`/`-V`/
/// `--version` ourselves so they exist pre-build — was tried and
/// reverted: `disable_help_flag`/`disable_version_flag` are always
/// `global_setting`s in clap (there is no non-propagating variant in the
/// public API), and `Command::_propagate_subcommand` ORs a parent's
/// global settings into every descendant's settings unconditionally, so
/// disabling the flag on the root disables it on every subcommand too —
/// irreversibly, since a child clearing its own copy of the setting is
/// overwritten again by that OR the next time a build runs. The
/// observable regression was verified directly: before the attempt,
/// `sm2-modloader save --help` printed that subcommand's help (exit 0);
/// after, it errored with "unexpected argument '--help' found" (exit 2)
/// for every subcommand, because none of them still had a `-h`/`--help`
/// of their own. Making root's `-h`/`--help`/`-V`/`--version` localizable
/// this way is therefore not achievable without also re-declaring a
/// `-h`/`--help` on every existing subcommand by hand (reproducing
/// clap's own default `Arg` exactly, to keep their behaviour identical),
/// which is no longer the "cheap" route. `-h`/`--help`/`-V`/`--version`
/// stay in clap's fixed English text everywhere, including at the root —
/// the accepted, documented gap.
const BUILT_IN: [(&str, &str); 2] =
    [("help", "cli.built_in.help"), ("version", "cli.built_in.version")];

pub fn localize(command: clap::Command, prefix: &str) -> clap::Command {
    let mut command = command.about(i18n::lookup(&format!("{prefix}.about")));

    let ids: Vec<clap::Id> = command.get_arguments().map(|arg| arg.get_id().clone()).collect();
    for id in ids {
        let key = match BUILT_IN.iter().find(|(name, _)| *name == id.as_str()) {
            Some((_, key)) => (*key).to_string(),
            None => format!("{prefix}.arg.{id}"),
        };
        command = command.mut_arg(id, |arg| arg.help(i18n::lookup(&key)));
    }

    let names: Vec<String> =
        command.get_subcommands().map(|sub| sub.get_name().to_string()).collect();
    for name in names {
        let child = format!("{prefix}.{name}");
        command = command.mut_subcommand(&name, |sub| localize(sub, &child));
    }
    command
}

/// Collects every key the tree needs and that `en.toml` does not have.
/// Shared by `localize`'s test and nothing else — hence `cfg(test)`.
#[cfg(test)]
fn collect_missing_keys(command: &clap::Command, prefix: &str, missing: &mut Vec<String>) {
    let about = format!("{prefix}.about");
    if !i18n::has_key(&about) {
        missing.push(about);
    }
    for arg in command.get_arguments() {
        let id = arg.get_id().as_str();
        if BUILT_IN.iter().any(|(name, _)| *name == id) {
            continue;
        }
        let key = format!("{prefix}.arg.{id}");
        if !i18n::has_key(&key) {
            missing.push(key);
        }
    }
    for sub in command.get_subcommands() {
        collect_missing_keys(sub, &format!("{prefix}.{}", sub.get_name()), missing);
    }
}

/// Reads `--lang` out of the raw arguments, before `clap` parses.
///
/// It has to happen this early because `--help` is answered *during*
/// parsing: by then the tree has to carry the right language already.
pub fn language_from_args(args: &[String]) -> Option<Language> {
    let mut iter = args.iter();
    while let Some(argument) = iter.next() {
        if let Some(code) = argument.strip_prefix("--lang=") {
            return Language::from_code(code);
        }
        if argument == "--lang" {
            return iter.next().and_then(|code| Language::from_code(code));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// The guard against drift: an argument added later without a key in
    /// the catalogue would silently keep clap's own English text.
    #[test]
    fn every_command_and_argument_has_a_key() {
        let mut missing = Vec::new();
        collect_missing_keys(&crate::cli::Cli::command(), "cli", &mut missing);

        assert!(missing.is_empty(), "keys missing from en.toml:\n{}", missing.join("\n"));
    }

    #[test]
    fn localizing_replaces_the_about_text() {
        // Guarded like every language-flipping test in this crate — see
        // `crate::app_state::language_test_lock`'s doc comment.
        let _held = crate::app_state::language_test_lock();
        sm2_core::i18n::set_language(sm2_core::i18n::Language::English);
        let command = localize(crate::cli::Cli::command(), "cli");

        let about = command.get_about().expect("the root command has an about text").to_string();

        assert_eq!(about, sm2_core::i18n::lookup("cli.about"));
    }

    #[test]
    fn a_language_option_in_front_of_everything_is_recognised() {
        let args = ["sm2-modloader", "--lang", "de", "save", "list"].map(String::from);
        assert_eq!(language_from_args(&args), Some(sm2_core::i18n::Language::German));

        let joined = ["sm2-modloader", "--lang=de"].map(String::from);
        assert_eq!(language_from_args(&joined), Some(sm2_core::i18n::Language::German));

        let without = ["sm2-modloader", "save", "list"].map(String::from);
        assert_eq!(language_from_args(&without), None);
    }
}

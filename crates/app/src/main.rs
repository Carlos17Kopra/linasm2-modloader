mod app_state;
mod cli;
mod cli_help;
mod gui;
mod vanilla;

use sm2_core::instance::InstanceLock;
use sm2_core::t;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    // Without arguments the graphical interface, with arguments the command
    // line — a single binary for both.
    if std::env::args().len() == 1 {
        // The language has to be set before the refusal below, which is the
        // only sentence a second instance ever gets to show. Without this it
        // would appear in English even though German is configured — the
        // interface that would otherwise set the language never opens.
        let dirs = match app_state::load_dirs_and_settings() {
            Ok((dirs, settings)) => {
                sm2_core::i18n::set_language(settings.language());
                Some(dirs)
            }
            // Nothing to lock without directories. `gui::run` runs into the
            // same failure and reports it in its own way.
            Err(_) => None,
        };
        // Named binding on purpose: this guard has to live as long as the
        // interface does. `let _ = ...` would release the lock right here
        // and leave the window running unprotected.
        let _lock = match dirs.as_ref().map(InstanceLock::acquire) {
            Some(Err(e)) => {
                gui::report_start_refused(&e.to_string());
                std::process::exit(1);
            }
            Some(Ok(lock)) => lock,
            None => None,
        };

        if let Err(e) = gui::run() {
            eprintln!("{}", t!("cli.error.gui_start", detail = e));
            std::process::exit(1);
        }
        return;
    }

    if let Err(e) = cli::run() {
        eprintln!("{}", t!("cli.error.prefix", detail = format!("{e:#}")));
        std::process::exit(1);
    }
}

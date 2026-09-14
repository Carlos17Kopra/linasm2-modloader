mod app_state;
mod cli;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    // Ohne Argumente startet später die GUI (Plan 2). Bis dahin: Hilfe zeigen.
    if std::env::args().len() == 1 {
        eprintln!("Die grafische Oberfläche folgt in Plan 2.\n");
        eprintln!("Verfügbare Kommandos: sm2-modloader --help");
        std::process::exit(2);
    }

    if let Err(e) = cli::run() {
        eprintln!("Fehler: {e:#}");
        std::process::exit(1);
    }
}

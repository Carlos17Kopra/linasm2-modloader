mod app_state;
mod cli;
mod gui;
mod vanilla;

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
        if let Err(e) = gui::run() {
            eprintln!("Fehler: die Oberfläche konnte nicht gestartet werden – {e}");
            std::process::exit(1);
        }
        return;
    }

    if let Err(e) = cli::run() {
        eprintln!("Fehler: {e:#}");
        std::process::exit(1);
    }
}

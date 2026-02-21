mod analyze;
mod config;
mod fetch;
mod tui;
mod util;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossterm::event::{self, Event};
use fetch::poller::run_poller;
use fetch::SparkHttpClient;
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;
use tui::Action;
use tui::app::App;

#[tokio::main]
async fn main() {
    let config = match config::resolve_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    // Init tracing to log file (stderr corrupts the TUI)
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/spark-tui.log")
        .expect("failed to open /tmp/spark-tui.log");
    let log_file = Mutex::new(log_file);
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_writer(log_file)
        .with_ansi(false)
        .init();

    // Set panic hook to restore terminal
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = ratatui::restore();
        original_hook(panic_info);
    }));

    let base_url = config.base_url();
    let client = Arc::new(SparkHttpClient::new(base_url, config.token.clone()));
    let poll_interval = Duration::from_secs(config.poll_interval);

    // Create channel
    let (tx, rx) = mpsc::unbounded_channel::<Action>();

    // Spawn poller task
    let poller_tx = tx.clone();
    let poller_client = Arc::clone(&client);
    tokio::spawn(async move {
        run_poller(poller_client, poller_tx, poll_interval).await;
    });

    // Spawn terminal event reader task
    let event_tx = tx.clone();
    tokio::spawn(async move {
        loop {
            match tokio::task::spawn_blocking(|| event::read()).await {
                Ok(Ok(evt)) => {
                    let action = match evt {
                        Event::Key(key) => Action::Key(key),
                        Event::Mouse(mouse) => Action::Mouse(mouse),
                        Event::Resize(w, h) => Action::Resize(w, h),
                        _ => continue,
                    };
                    if event_tx.send(action).is_err() {
                        break;
                    }
                }
                Ok(Err(_)) => break,
                Err(_) => break,
            }
        }
    });

    // Init terminal
    let mut terminal = ratatui::init();

    // Run app
    let mut app = App::new(config.cluster_id);
    let result = app.run(&mut terminal, rx).await;

    // Restore terminal
    ratatui::restore();

    if let Err(e) = result {
        eprintln!("Application error: {}", e);
        std::process::exit(1);
    }
}

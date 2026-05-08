mod app;
mod agent;
mod backend;
mod input;
mod message;
mod ui;

use app::App;
use backend::DiCoreBackend;
use clap::Parser;
use crossterm::event::{Event as CrosstermEvent, EventStream, KeyCode};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures::StreamExt;
use message::FrontendMessage;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;
use tokio::sync::mpsc;

#[derive(Parser, Debug)]
#[command(name = "divrr", about = "Adaptive TUI for di-core agent engine")]
struct Args {
    /// Task to run
    #[arg(short, long)]
    task: Option<String>,

    /// Path to di-core binary
    #[arg(short, long, default_value = "di-core")]
    core: String,
}

enum AppEvent {
    Key(crossterm::event::KeyEvent),
    CoreEvent(message::CoreEvent),
    CoreError(String),
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let args = Args::parse();

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Event channels
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();

    // Spawn crossterm key event reader
    let key_tx = event_tx.clone();
    tokio::spawn(async move {
        let mut reader = EventStream::new();
        loop {
            match reader.next().await {
                Some(Ok(CrosstermEvent::Key(key))) => {
                    if key_tx.send(AppEvent::Key(key)).is_err() {
                        break;
                    }
                }
                _ => {}
            }
        }
    });

    // Spawn di-core backend
    let mut di_core = DiCoreBackend::spawn(&args.core)?;

    // Forward di-core events into the unified event channel
    let core_tx = event_tx;
    let mut core_event_rx = di_core.take_event_rx();
    tokio::spawn(async move {
        while let Some(result) = core_event_rx.recv().await {
            match result {
                Ok(event) => {
                    if core_tx.send(AppEvent::CoreEvent(event)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    if core_tx.send(AppEvent::CoreError(e.to_string())).is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Send initial task if provided
    if let Some(task) = &args.task {
        di_core.send(&FrontendMessage::SpawnAgent { task: task.clone() }).await?;
    }

    let mut app = App::new();

    // Main event loop
    loop {
        // Render
        terminal.draw(|f| app.view(f))?;

        // Wait for next event
        let event = event_rx.recv().await;
        match event {
            Some(AppEvent::Key(key)) => {
                // Ctrl+C always exits
                if key.code == KeyCode::Char('c')
                    && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
                {
                    break;
                }

                if let Some(msg) = app.handle_key(key) {
                    if let Err(e) = di_core.send(&msg).await {
                        app.status_message = Some(format!("Send error: {}", e));
                    }
                }

                if app.should_quit {
                    break;
                }
            }
            Some(AppEvent::CoreEvent(event)) => {
                app.handle_core_event(event);
            }
            Some(AppEvent::CoreError(e)) => {
                app.status_message = Some(format!("di-core: {}", e));
            }
            None => {
                // Channel closed — di-core exited
                app.status_message = Some("di-core process exited".to_string());
                break;
            }
        }
    }

    // Cleanup
    disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

// Threading model:
//   - Main (async) thread: owns terminal rendering + all App state.
//     Receives events via a single mpsc channel (key events, di-core events,
//     settings load results).
//   - Crossterm reader: spawned task that forwards key/paste/resize events
//     into the channel.
//   - di-core backend: spawned child process with its own reader thread.
//     NDJSON events are forwarded into the channel.
//   - Settings gateway ops: spawn_blocking tasks for Unix socket I/O.
//     Results come back as SettingsLoaded events.
//
// Shutdown:
//   1. User presses Ctrl+C / SIGTERM → break main loop.
//   2. `drop(di_core)` kills the child process.
//   3. `_gateway_child` Drop kills the api-gateway process.
//   4. `_guard` Drop restores terminal (raw mode off, LeaveAlternateScreen).
//   5. The api-gateway daemon auto-shuts down after 2 minutes with no clients.

mod app;
mod agent;
mod app_types;
mod backend;
mod clipboard;
mod commands;
mod errors;
mod event;
mod gateway;
mod input;
mod line_cache;
mod logging;
mod message;
mod settings;
mod settings_model;
mod summarize;
mod theme;
mod ui;

use app::App;
use backend::DiCoreBackend;
use clap::Parser;
use crossterm::event::{Event as CrosstermEvent, EventStream, KeyEventKind, KeyCode};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures::StreamExt;
use message::FrontendMessage;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;
use std::time::Duration;
use tokio::sync::mpsc;

/// Send a message to di-core with a 5-second timeout to prevent UI freeze.
async fn send_with_timeout(
    backend: &mut DiCoreBackend,
    msg: &FrontendMessage,
) -> Result<(), String> {
    tokio::time::timeout(Duration::from_secs(5), backend.send(msg))
        .await
        .map_err(|_| "Send timed out (di-core may be stuck)".to_string())
        .and_then(|r| r.map_err(|e| format!("{}", e)))
}

/// Convert provider_params HashMap<String, String> to properly typed HashMap<String, Value>.
fn role_settings_to_params(rs: &settings::RoleSettings) -> std::collections::HashMap<String, serde_json::Value> {
    rs.provider_params.iter().map(|(k, v)| {
        (k.clone(), settings::string_to_json_value(v))
    }).collect()
}

/// RAII guard that restores the terminal on drop (even on panic).
struct TerminalGuard;

impl TerminalGuard {
    fn init() -> color_eyre::Result<Self> {
        enable_raw_mode()?;
        crossterm::execute!(io::stdout(), EnterAlternateScreen)?;
        // Install a panic hook that restores the terminal before printing
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = disable_raw_mode();
            let _ = crossterm::execute!(io::stdout(), LeaveAlternateScreen);
            prev_hook(info);
        }));
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = crossterm::execute!(io::stdout(), LeaveAlternateScreen);
    }
}

#[derive(Parser, Debug)]
#[command(name = "divrr", about = "Adaptive TUI for di-core agent engine")]
struct Args {
    /// Task to run
    #[arg(short, long)]
    task: Option<String>,

    /// Path to di-core binary
    #[arg(short, long, default_value = "di-core")]
    core: String,

    /// Path to api-gateway binary (auto-detected if omitted)
    #[arg(short, long)]
    gateway: Option<String>,
}

pub enum AppEvent {
    Key(crossterm::event::KeyEvent),
    Paste(String),
    Resize,
    Quit,
    CoreEvent(message::CoreEvent),
    CoreError(String),
    SettingsLoaded(settings::SettingsLoadResult),
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let args = Args::parse();

    // Ensure ~/.dirac exists before any I/O (logs, socket, settings).
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        let _ = std::fs::create_dir_all(std::path::Path::new(&home).join(".dirac"));
    }

    // Launch API gateway (per-PID socket). _gateway_child kills the process on drop.
    let mut _gateway_child: Option<gateway::GatewayChild> = match &args.gateway {
        Some(path) => Some(gateway::launch(path)?),
        None => match gateway::find_gateway() {
            Some(path) => Some(gateway::launch(&path)?),
            None => {
                eprintln!("Warning: api-gateway binary not found. Settings panel will be limited.");
                crate::logging::log_event("gateway binary not found, settings panel limited");
                None
            }
        },
    };

    // Terminal setup — guard ensures cleanup on drop/panic
    let _guard = TerminalGuard::init()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    // Event channels
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();

    // SIGTERM → graceful shutdown (SIGINT/Ctrl+C is already handled by
    // crossterm's raw-mode key event). Wakes the main loop so Drop guards
    // restore the terminal cleanly.
    let quit_tx = event_tx.clone();
    tokio::spawn(async move {
        let mut sigterm = tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate(),
        ).expect("failed to install SIGTERM handler");
        sigterm.recv().await;
        let _ = quit_tx.send(AppEvent::Quit);
    });

    // Spawn crossterm key event reader
    let key_tx = event_tx.clone();
    tokio::spawn(async move {
        let mut reader = EventStream::new();
        loop {
            match reader.next().await {
                Some(Ok(CrosstermEvent::Key(key)))
                    if key_tx.send(AppEvent::Key(key)).is_err() => break,
                Some(Ok(CrosstermEvent::Paste(text))) => {
                    if key_tx.send(AppEvent::Paste(text)).is_err() {
                        break;
                    }
                }
                Some(Ok(CrosstermEvent::Resize(..)))
                    if key_tx.send(AppEvent::Resize).is_err() => break,
                _ => {}
            }
        }
    });

    // Spawn di-core backend
    let mut di_core = DiCoreBackend::spawn(&args.core)?;

    // Forward di-core events into the unified event channel
    let core_tx = event_tx.clone();
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

    let mut app = App::new();
    app.event_tx = Some(event_tx.clone());

    // Apply saved theme
    let saved_settings = crate::settings::load_all_settings();
    app.theme = crate::theme::Theme::by_name(&saved_settings.theme);

    // Push saved per-role provider settings to API gateway in background (non-blocking)
    tokio::task::spawn_blocking(|| {
        match std::panic::catch_unwind(|| {
            settings::push_all_to_gateway();
        }) {
            Ok(()) => {}
            Err(_) => {
                crate::logging::log_event("push_all_to_gateway panicked");
            }
        }
    });

    // Send provider config to di-core BEFORE spawning any agents
    {
        let all = settings::load_all_settings();
        for role in settings::ROLES {
            if let Some(rs) = all.roles.get(*role) {
                if !rs.provider.is_empty() && !rs.model.is_empty() {
                    let params = role_settings_to_params(rs);
                    di_core.send(&FrontendMessage::SetProviderConfig {
                        role: role.to_string(),
                        provider: rs.provider.clone(),
                        model: rs.model.clone(),
                        api_key: if rs.api_key.is_empty() { None } else { Some(rs.api_key.clone()) },
                        base_url: if rs.base_url.is_empty() { None } else { Some(rs.base_url.clone()) },
                        params,
                    }).await?;
                }
            }
        }
    }

    // Send initial task if provided
    if let Some(task) = &args.task {
        di_core.send(&FrontendMessage::SpawnAgent { task: task.clone() }).await?;
    }

    // Main event loop
    loop {
        // Render
        let term_size = terminal.size().unwrap_or_else(|_| ratatui::layout::Rect::new(0, 0, 80, 24).into());
        // Compute layout heights to match ui/mod.rs
        let queue_h = if app.input_queue.is_empty() { 0 } else { app.input_queue.len().min(5) as u16 };
        let input_h = if app.input.multi_line {
            let content_lines = app.input.content.lines().count().max(1);
            let max_h = (term_size.height as usize * 3 / 4).max(1);
            content_lines.min(max_h) as u16
        } else {
            1
        };
        app.input.clamp_scroll(input_h as usize);
        let reserved = 1 + queue_h + input_h; // top bar + queue + input
        app.visible_lines = if term_size.height > reserved {
            (term_size.height - reserved) as usize
        } else {
            24
        };
        app.conv_width = term_size.width as usize;
        app.content_lines = app.count_rendered_lines(term_size.width);
        app.clamp_scroll();
        app.check_stream_stall();
        terminal.draw(|f| app.view(f))?;

        // Wait for next event
        let event = event_rx.recv().await;
        match event {
            Some(AppEvent::Key(key)) => {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                // Ctrl+C always exits
                if key.code == KeyCode::Char('c')
                    && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
                {
                    break;
                }

                // Drain pending messages before handling key (e.g. stale SetProviderConfig).
                // On send failure, re-queue for retry on the next key event.
                let pending: Vec<_> = app.pending_messages.drain(..).collect();
                for msg in pending {
                    if let Err(e) = send_with_timeout(&mut di_core, &msg).await {
                        crate::logging::log_event(&format!("send error: {}", e));
                        app.pending_messages.insert(0, msg);
                        app.status_message = Some(format!("Send error (queued for retry): {}", e));
                    }
                }

                if let Some(msg) = app.handle_key(key) {
                    // Drain any new pending messages pushed during handle_key (e.g. SetProviderConfig from :new)
                    // before sending the returned message (e.g. SpawnAgent) to ensure correct ordering.
                    for pending in app.pending_messages.drain(..) {
                        if let Err(e) = send_with_timeout(&mut di_core, &pending).await {
                            crate::logging::log_event(&format!("send error: {}", e));
                            app.status_message = Some(format!("Send error: {}", e));
                        }
                    }
                    if let Err(e) = send_with_timeout(&mut di_core, &msg).await {
                        crate::logging::log_event(&format!("send error: {}", e));
                        app.status_message = Some(format!("Send error: {}", e));
                    }
                }

                if app.should_quit {
                    break;
                }

                // Dispatch pending async gateway operations from settings
                if let Some(op) = app.settings.as_mut().and_then(|s| s.pending_async.take()) {
                    let tx = event_tx.clone();
                    tokio::task::spawn_blocking(move || {
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let mut gw = crate::settings::GatewayConnection::new();
                            let _ = gw.ensure_connected();
                            match op {
                                crate::settings::PendingAsyncOp::ProviderChanged { seq, rs, providers, gateway_available } => {
                                    let (fields, models, provider_info, gateway_error) = crate::settings::build_role_fields(
                                        &rs, &providers, &mut gw, gateway_available,
                                    );
                                    let _ = tx.send(crate::AppEvent::SettingsLoaded(
                                        crate::settings::SettingsLoadResult::ProviderChanged {
                                            seq, fields, model_entries: models, provider_info, gateway_error,
                                        }
                                    ));
                                }
                                crate::settings::PendingAsyncOp::RoleSwitched { seq, rs, providers, gateway_available } => {
                                    let (fields, models, provider_info, gateway_error) = crate::settings::build_role_fields(
                                        &rs, &providers, &mut gw, gateway_available,
                                    );
                                    let _ = tx.send(crate::AppEvent::SettingsLoaded(
                                        crate::settings::SettingsLoadResult::RoleSwitched {
                                            seq, fields, model_entries: models, provider_info, gateway_error,
                                        }
                                    ));
                                }
                                crate::settings::PendingAsyncOp::Save { all_settings } => {
                                    let (save_tx, save_rx) = std::sync::mpsc::channel();
                                    std::thread::spawn(move || {
                                        let mut gw = crate::settings::GatewayConnection::new();
                                        let _ = gw.ensure_connected();
                                        let mut error = None;
                                        for role in crate::settings::ROLES {
                                            if let Some(rs) = all_settings.roles.get(*role) {
                                                if rs.provider.is_empty() { continue; }
                                                if let Err(e) = crate::settings::validate_parameters(
                                                    &mut gw, &rs.provider, &rs.api_key, &rs.model, &rs.base_url,
                                                ) {
                                                    error = Some(format!("Validation failed for {}: {}", role, e));
                                                    break;
                                                }
                                                if let Err(e) = crate::settings::push_role_to_gateway(&mut gw, role, rs) {
                                                    error = Some(format!("Gateway push failed for {}: {}", role, e));
                                                    break;
                                                }
                                            }
                                        }
                                        if error.is_none() {
                                            if let Err(e) = crate::settings::save_all_settings_to_disk(&all_settings) {
                                                error = Some(format!("Failed to save settings: {}", e));
                                            }
                                        }
                                        let messages = if error.is_none() {
                                            let mut msgs = crate::settings::build_provider_config_messages(&all_settings);
                                            if let Some(obs_msg) = crate::settings::build_observer_config_message(&all_settings) {
                                                msgs.push(obs_msg);
                                            }
                                            msgs
                                        } else {
                                            Vec::new()
                                        };
                                        if let Some(ref e) = error {
                                            crate::logging::log_event(&format!("settings save failed: {}", e));
                                        } else {
                                            crate::logging::log_event("settings saved successfully");
                                        }
                                        let _ = save_tx.send((error, messages));
                                    });
                                    // Enforce an overall 30-second timeout for the entire save
                                    let timeout = std::time::Duration::from_secs(30);
                                    let (error, messages) = match save_rx.recv_timeout(timeout) {
                                        Ok(result) => result,
                                        Err(_) => {
                                            crate::logging::log_event("settings save timed out after 30s");
                                            (Some("Save timed out after 30s".to_string()), Vec::new())
                                        }
                                    };
                                    let _ = tx.send(crate::AppEvent::SettingsLoaded(
                                        crate::settings::SettingsLoadResult::Saved {
                                            error,
                                            messages,
                                        }
                                    ));
                                }
                            }
                        }));
                        if result.is_err() {
                            crate::logging::log_event("settings async operation panicked");
                        }
                    });
                }
            }
            Some(AppEvent::Paste(text)) => {
                app.handle_paste(&text);
            }
            Some(AppEvent::Resize) => {
                // Redraw happens at top of loop — this just breaks the recv() await.
            }
            Some(AppEvent::Quit) => {
                break;
            }
            Some(AppEvent::CoreEvent(event)) => {
                event::handle_core_event(&mut app, event);
                // Drain pending messages (e.g. auto-approve responses)
                for msg in app.pending_messages.drain(..) {
                    if let Err(e) = send_with_timeout(&mut di_core, &msg).await {
                        crate::logging::log_event(&format!("send error (core drain): {}", e));
                        app.status_message = Some(format!("Send error: {}", e));
                    }
                }
            }
            Some(AppEvent::CoreError(e)) => {
                crate::logging::log_event(&format!("di-core error: {}", e));
                app.status_message = Some(format!("di-core: {}", e));
            }
            Some(AppEvent::SettingsLoaded(result)) => {
                app.apply_settings_load(result);
                // Drain SetProviderConfig messages produced by successful save
                for msg in app.pending_messages.drain(..) {
                    if let Err(e) = send_with_timeout(&mut di_core, &msg).await {
                        crate::logging::log_event(&format!("send error (settings): {}", e));
                        app.status_message = Some(format!("Send error: {}", e));
                    }
                }
            }
            None => {
                crate::logging::log_event("di-core process exited");
                let msg = if app.agents.is_empty() {
                    "di-core exited before creating any agents. Check ~/.dirac/di-core.log."
                } else {
                    "di-core process exited"
                };
                app.status_message = Some(msg.to_string());
                // Print to stderr so the message is visible after TUI cleanup
                eprintln!("{}", msg);
                break;
            }
        }
    }

    // Explicitly drop di-core to kill the child process before the runtime shuts down
    drop(di_core);

    // _guard restores terminal, _gateway_child kills gateway — both drop here
    Ok(())
}

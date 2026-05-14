mod app;
mod agent;
mod backend;
mod gateway;
mod input;
mod message;
mod settings;
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

    // Launch API gateway (per-PID socket). _gateway_child kills the process on drop.
    let mut _gateway_child: Option<gateway::GatewayChild> = match &args.gateway {
        Some(path) => Some(gateway::launch(path)?),
        None => match gateway::find_gateway() {
            Some(path) => Some(gateway::launch(&path)?),
            None => {
                eprintln!("Warning: api-gateway binary not found. Settings panel will be limited.");
                crate::app::log_event("gateway binary not found, settings panel limited");
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
                Some(Ok(CrosstermEvent::Key(key))) => {
                    if key_tx.send(AppEvent::Key(key)).is_err() {
                        break;
                    }
                }
                Some(Ok(CrosstermEvent::Paste(text))) => {
                    if key_tx.send(AppEvent::Paste(text)).is_err() {
                        break;
                    }
                }
                Some(Ok(CrosstermEvent::Resize(..))) => {
                    if key_tx.send(AppEvent::Resize).is_err() {
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
                crate::app::log_event("push_all_to_gateway panicked");
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

                // Drain pending messages FIRST (e.g. SetProviderConfig from :new)
                // to ensure config arrives before any direct return (e.g. SpawnAgent).
                for msg in app.pending_messages.drain(..) {
                    if let Err(e) = send_with_timeout(&mut di_core, &msg).await {
                        crate::app::log_event(&format!("send error: {}", e));
                        app.status_message = Some(format!("Send error: {}", e));
                    }
                }

                if let Some(msg) = app.handle_key(key) {
                    if let Err(e) = send_with_timeout(&mut di_core, &msg).await {
                        crate::app::log_event(&format!("send error: {}", e));
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
                                    let mut error = None;
                                    for role in crate::settings::ROLES {
                                        if let Some(rs) = all_settings.roles.get(*role) {
                                            if rs.provider.is_empty() { continue; }
                                            // Validate credentials before pushing
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
                                    // Persist to disk only after validation and gateway push succeed
                                    if error.is_none() {
                                        if let Err(e) = crate::settings::save_all_settings_to_disk(&all_settings) {
                                            error = Some(format!("Failed to save settings: {}", e));
                                        }
                                    }
                                    // Build SetProviderConfig messages only on success
                                    let messages = if error.is_none() {
                                        crate::settings::build_provider_config_messages(&all_settings)
                                    } else {
                                        Vec::new()
                                    };
                                    if let Some(ref e) = error {
                                        crate::app::log_event(&format!("settings save failed: {}", e));
                                    } else {
                                        crate::app::log_event("settings saved successfully");
                                    }
                                    let _ = tx.send(crate::AppEvent::SettingsLoaded(
                                        crate::settings::SettingsLoadResult::Saved {
                                            error,
                                            messages,
                                        }
                                    ));
                                }
                            }
                        }));
                        if let Err(_) = result {
                            crate::app::log_event("settings async operation panicked");
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
                app.handle_core_event(event);
                // Drain pending messages (e.g. auto-approve responses)
                for msg in app.pending_messages.drain(..) {
                    if let Err(e) = send_with_timeout(&mut di_core, &msg).await {
                        crate::app::log_event(&format!("send error (core drain): {}", e));
                        app.status_message = Some(format!("Send error: {}", e));
                    }
                }
            }
            Some(AppEvent::CoreError(e)) => {
                crate::app::log_event(&format!("di-core error: {}", e));
                app.status_message = Some(format!("di-core: {}", e));
            }
            Some(AppEvent::SettingsLoaded(result)) => {
                app.apply_settings_load(result);
                // Drain SetProviderConfig messages produced by successful save
                for msg in app.pending_messages.drain(..) {
                    if let Err(e) = send_with_timeout(&mut di_core, &msg).await {
                        crate::app::log_event(&format!("send error (settings): {}", e));
                        app.status_message = Some(format!("Send error: {}", e));
                    }
                }
            }
            None => {
                crate::app::log_event("di-core process exited");
                app.status_message = Some("di-core process exited".to_string());
                break;
            }
        }
    }

    // Explicitly drop di-core to kill the child process before the runtime shuts down
    drop(di_core);

    // _guard restores terminal, _gateway_child kills gateway — both drop here
    Ok(())
}

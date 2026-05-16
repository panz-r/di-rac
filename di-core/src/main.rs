mod agent;
mod observer;
mod protocol;
mod context;
mod daemons;
mod tools;
mod prompt;
mod util;
mod hooks;

use agent::engine::MultiAgentOrchestrator;
use protocol::{CoreEvent, FrontendMessage};
use std::io::{self, BufRead, Write};
use anyhow::Result;
use clap::Parser;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "di-core", about = "Dirac agent engine")]
struct Cli {
    /// Validate a .dhook file and exit
    #[arg(long, value_name = "FILE")]
    validate_hook: Option<std::path::PathBuf>,
}

/// Simple file logger that appends to ~/.di/di-core.log
struct FileLogger {
    file: std::fs::File,
}

impl FileLogger {
    fn new() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        let dir = std::path::Path::new(&home).join(".di");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("di-core.log");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap_or_else(|e| {
                eprintln!("di-core: cannot open {}: {}, falling back to stderr logging", path.display(), e);
                // Use stderr by creating a write-only handle to /dev/stderr
                std::fs::OpenOptions::new()
                    .write(true)
                    .open("/dev/stderr")
                    .expect("stderr should always be available")
            });
        Self { file }
    }

    fn log(&mut self, msg: &str) {
        let ts = chrono::Local::now().format("%H:%M:%S%.3f");
        let _ = writeln!(self.file, "[{}] {}", ts, msg);
        let _ = self.file.flush();
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // --validate-hook: validate a .dhook file and exit
    if let Some(path) = cli.validate_hook {
        let source = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: cannot read {}: {}", path.display(), e);
                std::process::exit(1);
            }
        };
        let diags = crate::hooks::validator::validate_hook(&source);
        if diags.is_empty() {
            println!("{}: OK — no issues found", path.display());
            return Ok(());
        }
        for d in &diags {
            let loc = match d.line {
                Some(line) => format!("{}:{}", path.display(), line),
                None => path.display().to_string(),
            };
            println!("{}: {}: {}", loc, d.severity, d.message);
        }
        // Exit with non-zero if there were errors
        if diags.iter().any(|d| d.severity == crate::hooks::validator::Severity::Error) {
            std::process::exit(1);
        }
        return Ok(());
    }

    let mut log = FileLogger::new();
    log.log("di-core starting");

    let analyzer_binary = std::env::var("DI_ANALYZER_BINARY")
        .unwrap_or_else(|_| "divrr-analyzer".to_string());
    let command_daemon_binary = std::env::var("DI_COMMAND_BINARY")
        .unwrap_or_else(|_| "di-rvv-cmd".to_string());
    let central_socket = std::env::var("DI_CENTRAL_SOCKET")
        .unwrap_or_else(|_| "/tmp/di-vrr-coord.sock".to_string());
    let gateway_socket = std::env::var("DI_API_GATEWAY_SOCKET")
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
            format!("{}/.di/api-gateway.sock", home)
        });

    log.log(&format!("config: analyzer_binary={} cmd_binary={} central={} gateway={}",
        analyzer_binary, command_daemon_binary, central_socket, gateway_socket));

    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "/".to_string());

    // Spawn command daemon as child process (resilient wrapper with auto-restart)
    let command_daemon = crate::daemons::ResilientDaemon::spawn(&command_daemon_binary, &cwd).await?;
    log.log("command daemon spawned");

    // Spawn analyzer daemon as child process (resilient wrapper with auto-restart)
    let analyzer_daemon = crate::daemons::ResilientDaemon::spawn(&analyzer_binary, &cwd).await?;
    log.log("analyzer daemon spawned");

    let mut orchestrator = MultiAgentOrchestrator::new(
        analyzer_daemon,
        command_daemon,
        &gateway_socket,
    );

    log.log("engine started, reading stdin");

    // Early validation: check that the gateway socket exists before entering the main loop.
    // This prevents the agent from running several turns before discovering a misconfigured socket.
    if let Err(e) = orchestrator.gateway_client.validate_socket() {
        log.log(&format!("WARNING: gateway socket validation failed: {}", e));
    }

    // Read stdin in a separate thread and forward lines to an async channel.
    // This allows the main async loop to process incoming messages concurrently
    // while agent tasks are running (critical for approval/followup responses).
    let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::channel::<String>(256);
    std::thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(l) if !l.trim().is_empty() => {
                    if stdin_tx.blocking_send(l).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    // Channel for spawned tasks to report completion so we can clean up frontend_channels.
    let (done_tx, mut done_rx) = tokio::sync::mpsc::channel::<Uuid>(32);

    // SIGTERM channel: on graceful shutdown, break the main loop so that
    // daemon children are killed via Drop before di-core exits.
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);
    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        Ok(mut sigterm) => {
            tokio::spawn(async move {
                sigterm.recv().await;
                let _ = shutdown_tx.send(()).await;
            });
        }
        Err(e) => {
            log.log(&format!("WARNING: could not install SIGTERM handler: {}", e));
        }
    }

    loop {
        tokio::select! {
            // Agent task finished — clean up its channel
            Some(agent_id) = done_rx.recv() => {
                log.log(&format!("agent {} task finished, cleaning up channel", agent_id));
                orchestrator.cleanup_agent(&agent_id);
            }
            // SIGTERM received — break the main loop so Drop runs on daemons
            _ = shutdown_rx.recv() => {
                log.log("SIGTERM received, shutting down");
                break;
            }
            // Stdin message from frontend
            line = stdin_rx.recv() => {
                let line: String = match line {
                    Some(l) => l,
                    None => break,
                };
                log.log(&format!("stdin: {}", &line[..line.len().min(200)]));

                match serde_json::from_str::<FrontendMessage>(&line) {
                    Ok(msg) => {
                        match msg {
                            FrontendMessage::SpawnAgent { task } => {
                                log.log(&format!("SpawnAgent: {}", &task[..task.len().min(80)]));
                                let id = orchestrator.spawn_agent(task.clone());
                                orchestrator.emit_event(CoreEvent::TaskInitialized {
                                    agent_id: id,
                                    history_count: 0,
                                }).await?;

                                if let Some(mut agent) = orchestrator.remove_agent(id) {
                                    log.log(&format!("agent {} spawning task, provider={:?}", id, agent.provider_config.as_ref().map(|c| &c.id)));
                                    let done_tx = done_tx.clone();
                                    let task_clone = task.clone();
                                    tokio::spawn(async move {
                                        // Emit TaskFinished on both Err and panic paths so TUI doesn't hang.
                                        let finish_event = |agent_id: uuid::Uuid, success: bool, msg: String| {
                                            if let Ok(json) = serde_json::to_string(&CoreEvent::TaskFinished {
                                                agent_id, success, message: msg,
                                            }) {
                                                use std::io::Write;
                                                let _ = std::io::stdout().write_all(json.as_bytes());
                                                let _ = std::io::stdout().write_all(b"\n");
                                                let _ = std::io::stdout().flush();
                                            }
                                        };

                                        let agent_id = agent.id;
                                        match agent.run_task(task_clone).await {
                                            Ok(()) => {
                                                // Normal completion — TaskFinished already emitted by engine
                                            }
                                            Err(e) => {
                                                eprintln!("[di-core] agent {} FAILED: {}", agent_id, e);
                                                finish_event(agent_id, false, format!("Agent error: {}", e));
                                            }
                                        }

                                        // Signal cleanup via done_tx (best-effort, non-blocking)
                                        let _ = done_tx.try_send(agent_id);
                                    });
                                }
                            }
                            FrontendMessage::UserResponse { agent_id, text } => {
                                log.log(&format!("UserResponse: agent={} text={}", agent_id, &text[..text.len().min(60)]));
                                if let Err(e) = orchestrator.handle_user_response(agent_id, text).await {
                                    log.log(&format!("UserResponse FAILED for {}: {}", agent_id, e));
                                    orchestrator.emit_event(CoreEvent::TaskFinished {
                                        agent_id,
                                        success: false,
                                        message: format!("Error: {}", e),
                                    }).await?;
                                }
                            }
                            FrontendMessage::Interrupt { agent_id } => {
                                log.log(&format!("Interrupt: agent={}", agent_id));
                                orchestrator.abort_agent(agent_id);
                                // Also send via channel so blocking recv_frontend loops in the agent wake up
                                orchestrator.send_to_agent(agent_id, FrontendMessage::Interrupt { agent_id }).await;
                            }
                            FrontendMessage::ReloadSessionHooks { agent_id } => {
                                log.log(&format!("ReloadSessionHooks: agent={}", agent_id));
                                // Route through frontend channel — the agent is in a spawned task
                                // and not in self.agents. send_to_agent uses frontend_channels.
                                orchestrator.send_to_agent(agent_id, FrontendMessage::ReloadSessionHooks { agent_id }).await;
                            }
                            FrontendMessage::ApprovalResponse { agent_id, approval_id, approved } => {
                                log.log(&format!("ApprovalResponse: agent={} approved={}", agent_id, approved));
                                if !orchestrator.send_to_agent(agent_id, FrontendMessage::ApprovalResponse { agent_id, approval_id, approved }).await {
                                    log.log(&format!("ApprovalResponse: no channel for agent {}", agent_id));
                                }
                            }
                            FrontendMessage::FollowupAnswer { agent_id, text } => {
                                log.log(&format!("FollowupAnswer: agent={}", agent_id));
                                if !orchestrator.send_to_agent(agent_id, FrontendMessage::FollowupAnswer { agent_id, text }).await {
                                    log.log(&format!("FollowupAnswer: no channel for agent {}", agent_id));
                                }
                            }
                            FrontendMessage::Timeout { duration_ms } => {
                                log.log(&format!("Timeout: {}ms", duration_ms));
                                orchestrator.set_all_frontend_timeouts(duration_ms);
                            }
                            FrontendMessage::SetProviderConfig { role, provider, model, api_key, base_url, params } => {
                                use crate::daemons::ProviderConfig;
                                let config = ProviderConfig {
                                    id: provider,
                                    model,
                                    api_key,
                                    base_url,
                                    region: None,
                                    project_id: None,
                                    params,
                                };
                                log.log(&format!("SetProviderConfig: role={} provider={} model={} params={}", role, config.id, config.model, config.params.len()));
                                match role.as_str() {
                                    "act" => {
                                        orchestrator.set_provider_config(config);
                                    }
                                    "plan" => {
                                        log.log("Setting plan config");
                                        orchestrator.set_plan_config(config);
                                    }
                                    "distiller" => {
                                        log.log("Setting distiller config");
                                        orchestrator.set_distiller_config(config);
                                    }
                                    _ => {}
                                }
                            }
                            msg @ FrontendMessage::SetObserverConfig { .. } => {
                                log.log("SetObserverConfig: updating observer settings");
                                orchestrator.set_observer_config(msg);
                            }
                            FrontendMessage::SetMode { agent_id, mode } => {
                                log.log(&format!("SetMode: agent={} mode={}", agent_id, mode));
                                orchestrator.set_agent_mode(agent_id, &mode).await;
                            }
                        }
                    }
                    Err(e) => {
                        let err_msg = format!("PARSE ERROR: {} — line: {}", e, &line[..line.len().min(200)]);
                        log.log(&err_msg);
                        // Emit to stdout so TUI can show it (best-effort)
                        if let Ok(json) = serde_json::to_string(&serde_json::json!({
                            "type": "TaskFinished",
                            "payload": {
                                "agent_id": uuid::Uuid::nil(),
                                "success": false,
                                "message": err_msg,
                            }
                        })) {
                            use std::io::Write;
                            let _ = std::io::stdout().write_all(json.as_bytes());
                            let _ = std::io::stdout().write_all(b"\n");
                            let _ = std::io::stdout().flush();
                        }
                    }
                }
            }
        }
    }

    log.log("di-core stdin EOF, exiting");
    Ok(())
}

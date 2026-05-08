mod agent;
mod observer;
mod protocol;
mod context;
mod daemons;
mod tools;

use agent::engine::MultiAgentOrchestrator;
use protocol::{CoreEvent, FrontendMessage};
use std::io::{self, BufRead};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let analyzer_socket = std::env::var("DIRAC_ANALYZER_SOCKET")
        .unwrap_or_else(|_| "/tmp/di-analyzer.sock".to_string());
    let command_socket = std::env::var("DIRAC_COMMAND_SOCKET")
        .unwrap_or_else(|_| "/tmp/di-cmd.sock".to_string());
    let central_socket = std::env::var("DIRAC_CENTRAL_SOCKET")
        .unwrap_or_else(|_| "/tmp/di-central.sock".to_string());
    let gateway_socket = std::env::var("DIRAC_API_GATEWAY_SOCKET")
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
            format!("{}/.dirac/api-gateway.sock", home)
        });

    let mut orchestrator = MultiAgentOrchestrator::new(
        &analyzer_socket,
        &command_socket,
        &central_socket,
        &gateway_socket,
    );

    eprintln!("di-core: engine started");

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() { continue; }

        match serde_json::from_str::<FrontendMessage>(&line) {
            Ok(msg) => {
                match msg {
                    FrontendMessage::SpawnAgent { task } => {
                        let id = orchestrator.spawn_agent(task.clone());
                        orchestrator.emit_event(CoreEvent::TaskInitialized {
                            agent_id: id,
                            history_count: 0,
                        })?;

                        // Run the agent task in a background tokio task
                        // For now, we run inline since the orchestrator holds mutable state.
                        // TODO: use per-agent channels for concurrent execution.
                        if let Some(agent) = orchestrator.agents.get_mut(&id) {
                            if let Err(e) = agent.run_task(task).await {
                                eprintln!("Agent {} failed: {}", id, e);
                            }
                        }
                        // Clean up agent to release Arc reference counts
                        orchestrator.remove_agent(id);
                    }
                    FrontendMessage::UserResponse { agent_id, text } => {
                        if let Err(e) = orchestrator.handle_user_response(agent_id, text).await {
                            eprintln!("User response handling failed for {}: {}", agent_id, e);
                        }
                    }
                    FrontendMessage::Interrupt { agent_id } => {
                        if orchestrator.abort_agent(agent_id) {
                            eprintln!("Interrupted agent: {}", agent_id);
                        } else {
                            eprintln!("Agent {} not found for interrupt", agent_id);
                        }
                    }
                    FrontendMessage::ApprovalResponse { agent_id, approved } => {
                        if orchestrator.send_to_agent(agent_id, FrontendMessage::ApprovalResponse { agent_id, approved }).await {
                            eprintln!("Routed approval response to agent {}", agent_id);
                        } else {
                            eprintln!("Agent {} not found for approval response", agent_id);
                        }
                    }
                    FrontendMessage::FollowupAnswer { agent_id, text } => {
                        if orchestrator.send_to_agent(agent_id, FrontendMessage::FollowupAnswer { agent_id, text }).await {
                            eprintln!("Routed followup answer to agent {}", agent_id);
                        } else {
                            eprintln!("Agent {} not found for followup answer", agent_id);
                        }
                    }
                    FrontendMessage::Timeout { duration_ms } => {
                        orchestrator.set_all_frontend_timeouts(duration_ms);
                        eprintln!("Frontend timeout set to {}ms for all agents", duration_ms);
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to parse frontend message: {}", e);
            }
        }
    }

    Ok(())
}

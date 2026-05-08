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
    // These would typically come from environment variables or CLI flags
    let orchestrator = MultiAgentOrchestrator::new(
        "/tmp/di-analyzer.sock", 
        "/tmp/di-cmd.sock",
        "/tmp/di-central.sock",
        "http://localhost:3000" // api-gateway
    );

    let mut orchestrator = orchestrator;

    eprintln!("di-core: engine started");

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() { continue; }

        match serde_json::from_str::<FrontendMessage>(&line) {
            Ok(msg) => {
                match msg {
                    FrontendMessage::SpawnAgent { task } => {
                        let id = orchestrator.spawn_agent(task);
                        orchestrator.emit_event(CoreEvent::TaskInitialized {
                            agent_id: id,
                            history_count: 0,
                        })?;
                    }
                    FrontendMessage::UserResponse { agent_id, text } => {
                        orchestrator.handle_user_response(agent_id, text).await?;
                    }
                    FrontendMessage::Interrupt { agent_id } => {
                        eprintln!("Interrupting agent: {}", agent_id);
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

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CommandStatus {
    Running,
    Completed,
    Failed,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundCommand {
    pub id: String,
    pub command: String,
    pub start_time: DateTime<Utc>,
    pub status: CommandStatus,
    pub log_path: String,
    pub exit_code: Option<i32>,
}

pub struct BackgroundCommandTracker {
    commands: Mutex<HashMap<String, BackgroundCommand>>,
}

impl BackgroundCommandTracker {
    pub fn new() -> Self {
        Self {
            commands: Mutex::new(HashMap::new()),
        }
    }

    pub fn track(&self, cmd: BackgroundCommand) {
        self.commands.lock().unwrap().insert(cmd.id.clone(), cmd);
    }

    pub fn update_status(&self, id: &str, status: CommandStatus, exit_code: Option<i32>) {
        if let Some(cmd) = self.commands.lock().unwrap().get_mut(id) {
            cmd.status = status;
            cmd.exit_code = exit_code;
        }
    }

    pub fn get(&self, id: &str) -> Option<BackgroundCommand> {
        self.commands.lock().unwrap().get(id).cloned()
    }

    pub fn get_running(&self) -> Vec<BackgroundCommand> {
        self.commands
            .lock()
            .unwrap()
            .values()
            .filter(|c| c.status == CommandStatus::Running)
            .cloned()
            .collect()
    }

    pub fn count_running(&self) -> usize {
        self.commands
            .lock()
            .unwrap()
            .values()
            .filter(|c| c.status == CommandStatus::Running)
            .count()
    }

    /// Formatted summary for prompt injection. Includes command IDs so the
    /// model can reference them with TaskOutput after context management.
    pub fn get_summary(&self) -> Option<String> {
        let running = self.get_running();
        if running.is_empty() {
            return None;
        }

        let mut lines = vec![format!("# Background Commands ({} running)", running.len())];
        for c in &running {
            let duration = (Utc::now() - c.start_time).num_minutes();
            lines.push(format!(
                "- ID: {}, command: \"{}\" (running {}m, log: {})",
                c.id, c.command, duration, c.log_path
            ));
        }
        lines.push("Use TaskOutput with these IDs to retrieve results.".to_string());
        Some(lines.join("\n"))
    }

    pub fn cancel(&self, id: &str) -> bool {
        if let Some(cmd) = self.commands.lock().unwrap().get_mut(id) {
            if cmd.status == CommandStatus::Running {
                cmd.status = CommandStatus::Cancelled;
                return true;
            }
        }
        false
    }
}

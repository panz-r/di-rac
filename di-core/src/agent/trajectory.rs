use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum Role {
    User,
    Assistant,
    System,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: Uuid,
    pub role: Role,
    pub content: serde_json::Value,
    pub timestamp: DateTime<Utc>,
    pub tokens: usize,
    pub is_compressed: bool,
}

pub struct Trajectory {
    pub messages: Vec<Message>,
}

impl Trajectory {
    pub fn new() -> Self {
        Self { messages: Vec::new() }
    }

    pub fn add_message(&mut self, role: Role, content: serde_json::Value, tokens: usize) -> Uuid {
        let msg = Message {
            id: Uuid::new_v4(),
            role,
            content,
            timestamp: Utc::now(),
            tokens,
            is_compressed: false,
        };
        let id = msg.id;
        self.messages.push(msg);
        id
    }

    pub fn get_total_tokens(&self) -> usize {
        self.messages.iter().map(|m| m.tokens).sum()
    }
}

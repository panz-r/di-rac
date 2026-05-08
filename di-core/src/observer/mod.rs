use crate::agent::trajectory::{Trajectory, Message, Role};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct SQSResult {
    pub score: f32,
    pub status: String, 
    pub monotonicity: f32,
}

pub struct Observer {
    pub resolved_subgoals: HashSet<String>,
    pub turns_since_reflection: usize,
    pub config: ObserverConfig,
}

pub struct ObserverConfig {
    pub reflection_cooldown: usize,
    pub confidence_threshold: f32,
}

impl Observer {
    pub fn new() -> Self {
        Self {
            resolved_subgoals: HashSet::new(),
            turns_since_reflection: 0,
            config: ObserverConfig {
                reflection_cooldown: 4,
                confidence_threshold: 0.5,
            },
        }
    }

    pub fn compute_sqs(&mut self, trajectory: &Trajectory) -> SQSResult {
        let assistant_msgs: Vec<&Message> = trajectory.messages
            .iter()
            .filter(|m| matches!(m.role, Role::Assistant))
            .rev()
            .take(10)
            .collect();

        if assistant_msgs.is_empty() {
            return SQSResult { score: 1.0, status: "FOCUSED".to_string(), monotonicity: 1.0 };
        }

        let ee_ratio = self.calculate_ee_ratio(&assistant_msgs);
        let mono = self.calculate_monotonicity(trajectory);

        let diffusion = 0.4;
        let dcr = 0.2;
        let cps = 0.5;

        let score = 0.30 * (1.0 - diffusion) + 0.25 * ee_ratio + 0.20 * dcr + 0.15 * cps + 0.10 * mono;

        let status = if score < 0.35 {
            "STAGNATING"
        } else if score > 0.6 {
            "EXPLORING"
        } else {
            "FOCUSED"
        };

        SQSResult {
            score: score as f32,
            status: status.to_string(),
            monotonicity: mono as f32,
        }
    }

    fn calculate_ee_ratio(&self, msgs: &[&Message]) -> f32 {
        let mut unique_files = HashSet::new();
        let mut loop_counts = std::collections::HashMap::new();

        for msg in msgs {
            let file = "global"; 
            unique_files.insert(file);
            let h = format!("{}:think", file);
            *loop_counts.entry(h).or_insert(0) += 1;
        }

        let max_loops = loop_counts.values().max().cloned().unwrap_or(1);
        (unique_files.len() as f32 / msgs.len() as f32) * (1.0 / max_loops as f32)
    }

    fn calculate_monotonicity(&mut self, trajectory: &Trajectory) -> f32 {
        let last_tool = trajectory.messages.iter().filter(|m| matches!(m.role, Role::Tool)).last();
        if let Some(msg) = last_tool {
            if msg.content.to_string().contains("fixed") {
                self.resolved_subgoals.insert(Uuid::new_v4().to_string());
            }
        }
        1.0 
    }
}

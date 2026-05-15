//! Observation store with in-memory cache and JSONL persistence.

use super::{Observation, ObservationType};
use std::fs;
use std::io::{BufRead, Write};
use std::path::PathBuf;

const DEFAULT_JSONL_FILE: &str = ".di/observations.jsonl";
const MAX_STORED_OBSERVATIONS: usize = 200;

pub struct ObservationStore {
    observations: Vec<Observation>,
    persist_path: Option<PathBuf>,
}

impl ObservationStore {
    pub fn new(task_path: Option<&str>) -> Self {
        let persist_path = task_path.map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_JSONL_FILE));

        let observations = if persist_path.exists() {
            Self::load_from_disk(&persist_path)
        } else {
            Vec::new()
        };

        Self {
            observations,
            persist_path: Some(persist_path),
        }
    }

    /// Create an in-memory store without disk persistence (for tests).
    pub fn new_in_memory() -> Self {
        Self {
            observations: Vec::new(),
            persist_path: None,
        }
    }

    /// Append an observation, persisting to disk.
    pub fn append(&mut self, obs: Observation) {
        if self.observations.len() >= MAX_STORED_OBSERVATIONS {
            if let Some(idx) = self.observations.iter().position(|o| o.obs_type != ObservationType::Reflection) {
                self.observations.remove(idx);
            } else {
                self.observations.remove(0);
            }
        }

        if let Some(ref path) = self.persist_path {
            if let Ok(line) = serde_json::to_string(&obs) {
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(path) {
                    let _ = writeln!(f, "{}", line);
                }
            }
        }

        self.observations.push(obs);
    }

    /// Build a text block of observations, filtered by type, limited to last N entries.
    pub fn build_observation_block(&self, filter: Option<ObservationType>, last_n: Option<usize>) -> String {
        let mut filtered: Vec<&Observation> = self.observations.iter()
            .filter(|o| filter.as_ref().map_or(true, |t| o.obs_type == *t))
            .collect();

        // Take last N (TS returns last 2 for watcher/filter/critic)
        if let Some(n) = last_n {
            let start = filtered.len().saturating_sub(n);
            filtered = filtered[start..].to_vec();
        }

        if filtered.is_empty() {
            return String::new();
        }

        filtered.iter()
            .map(|o| format!("[{}] {}", o.obs_type, o.text))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Get the latest observation of a given type.
    pub fn get_latest(&self, obs_type: ObservationType) -> Option<&Observation> {
        self.observations.iter().rev()
            .find(|o| o.obs_type == obs_type)
    }

    /// Estimate total token count across all observations (rough: chars / 3).
    pub fn estimate_token_count(&self) -> usize {
        self.observations.iter()
            .map(|o| o.text.len().max(o.token_estimate))
            .sum::<usize>()
            / 4
    }

    /// Archive the existing JSONL file and replace with a single observation.
    pub fn archive_and_replace(&mut self, obs: Observation) {
        // Archive old file if it exists
        if let Some(ref path) = self.persist_path {
            if path.exists() {
                let archive_path = path.with_extension("archived.jsonl");
                let _ = fs::rename(path, &archive_path);
            }
            // Write new file
            if let Ok(line) = serde_json::to_string(&obs) {
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if let Ok(mut f) = fs::File::create(path) {
                    let _ = writeln!(f, "{}", line);
                }
            }
        }

        self.observations.clear();
        self.observations.push(obs);
    }

    /// Clear all and replace without archiving (for tests / forced reset).
    pub fn clear_and_replace(&mut self, obs: Observation) {
        if let Some(ref path) = self.persist_path {
            let _ = fs::remove_file(path);
            if let Ok(line) = serde_json::to_string(&obs) {
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if let Ok(mut f) = fs::File::create(path) {
                    let _ = writeln!(f, "{}", line);
                }
            }
        }
        self.observations.clear();
        self.observations.push(obs);
    }

    /// Number of stored observations.
    pub fn len(&self) -> usize {
        self.observations.len()
    }

    /// Access all observations.
    pub fn get_all(&self) -> &[Observation] {
        &self.observations
    }

    /// Access all observations mutably.
    pub fn get_all_mut(&mut self) -> &mut [Observation] {
        &mut self.observations
    }

    fn load_from_disk(path: &PathBuf) -> Vec<Observation> {
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };

        let reader = std::io::BufReader::new(file);
        reader.lines()
            .filter_map(|line| line.ok())
            .filter_map(|line| serde_json::from_str::<Observation>(&line).ok())
            .take(MAX_STORED_OBSERVATIONS)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observer::{CriticAction, SkeletonFidelity};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn make_obs(obs_type: ObservationType, text: &str) -> Observation {
        Observation {
            obs_type,
            text: text.to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
            confidence: 0.7,
            token_estimate: text.len() / 3,
            compressed_range: None,
            critic_action: None,
            sqs: None,
            fidelity: None,
            key: None,
        }
    }

    #[test]
    fn test_append_and_len() {
        let mut store = ObservationStore::new_in_memory();
        assert_eq!(store.len(), 0);

        store.append(make_obs(ObservationType::Watcher, "test watcher"));
        assert_eq!(store.len(), 1);

        store.append(make_obs(ObservationType::Critic, "test critic"));
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn test_build_observation_block_no_filter() {
        let mut store = ObservationStore::new_in_memory();
        store.append(make_obs(ObservationType::Watcher, "insight A"));
        store.append(make_obs(ObservationType::Critic, "insight B"));

        let block = store.build_observation_block(None, None);
        assert!(block.contains("insight A"));
        assert!(block.contains("insight B"));
    }

    #[test]
    fn test_build_observation_block_filtered() {
        let mut store = ObservationStore::new_in_memory();
        store.append(make_obs(ObservationType::Watcher, "watcher insight"));
        store.append(make_obs(ObservationType::Critic, "critic insight"));

        let block = store.build_observation_block(Some(ObservationType::Watcher), None);
        assert!(block.contains("watcher insight"));
        assert!(!block.contains("critic insight"));
    }

    #[test]
    fn test_build_observation_block_last_n() {
        let mut store = ObservationStore::new_in_memory();
        store.append(make_obs(ObservationType::Watcher, "first"));
        store.append(make_obs(ObservationType::Watcher, "second"));
        store.append(make_obs(ObservationType::Watcher, "third"));

        let block = store.build_observation_block(Some(ObservationType::Watcher), Some(2));
        assert!(!block.contains("first"));
        assert!(block.contains("second"));
        assert!(block.contains("third"));
    }

    #[test]
    fn test_get_latest() {
        let mut store = ObservationStore::new_in_memory();
        store.append(make_obs(ObservationType::Watcher, "older"));
        store.append(make_obs(ObservationType::Critic, "critic"));
        store.append(make_obs(ObservationType::Watcher, "newer"));

        let latest = store.get_latest(ObservationType::Watcher);
        assert!(latest.is_some());
        assert_eq!(latest.unwrap().text, "newer");
    }

    #[test]
    fn test_archive_and_replace() {
        let mut store = ObservationStore::new_in_memory();
        store.append(make_obs(ObservationType::Watcher, "old 1"));
        store.append(make_obs(ObservationType::Watcher, "old 2"));
        assert_eq!(store.len(), 2);

        store.archive_and_replace(make_obs(ObservationType::Reflection, "compressed"));
        assert_eq!(store.len(), 1);
        assert_eq!(store.get_all()[0].text, "compressed");
    }

    #[test]
    fn test_estimate_token_count() {
        let mut store = ObservationStore::new_in_memory();
        store.append(make_obs(ObservationType::Watcher, "short"));
        let tokens = store.estimate_token_count();
        assert!(tokens > 0);
    }

    #[test]
    fn test_observation_metadata_roundtrip() {
        let obs = Observation {
            obs_type: ObservationType::Critic,
            text: "test".into(),
            timestamp: 12345,
            confidence: 0.8,
            token_estimate: 10,
            compressed_range: Some([1, 5]),
            critic_action: Some(CriticAction::Restart),
            sqs: Some(0.35),
            fidelity: Some(SkeletonFidelity::Structural),
            key: None,
        };
        let json = serde_json::to_string(&obs).unwrap();
        let parsed: Observation = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.compressed_range.unwrap(), [1, 5]);
        assert_eq!(parsed.critic_action.unwrap(), CriticAction::Restart);
        assert_eq!(parsed.sqs.unwrap(), 0.35);
    }
}

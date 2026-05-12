use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// State tracked for each file the agent has read.
pub struct FileReadState {
    pub content_hash: String,
    pub read_count: usize,
    pub last_read_event_id: Uuid,
    pub edited_since_read: bool,
}

/// Tracks which files the agent has read or edited during a task,
/// with content hashes for staleness detection.
pub struct FileContextTracker {
    pub files_read: HashMap<String, FileReadState>,
    pub files_edited: HashSet<String>,
    pub files_metadata_observed: HashSet<String>,
}

impl FileContextTracker {
    pub fn new() -> Self {
        Self {
            files_read: HashMap::new(),
            files_edited: HashSet::new(),
            files_metadata_observed: HashSet::new(),
        }
    }

    /// Record that a file was read, with a hash of its content.
    pub fn mark_read(&mut self, path: &str, content_hash: &str) {
        let entry = self.files_read.entry(path.to_string())
            .or_insert(FileReadState {
                content_hash: String::new(),
                read_count: 0,
                last_read_event_id: Uuid::nil(),
                edited_since_read: false,
            });
        entry.content_hash = content_hash.to_string();
        entry.read_count += 1;
        entry.edited_since_read = false;
        entry.last_read_event_id = Uuid::new_v4();
    }

    /// Record that a file was edited or written. Marks any existing read
    /// state as stale so subsequent prompts warn the model.
    pub fn mark_edited(&mut self, path: &str) {
        self.files_edited.insert(path.to_string());
        if let Some(state) = self.files_read.get_mut(path) {
            state.edited_since_read = true;
        }
    }

    /// Record that a file was observed via search/symbols/repo without a
    /// content hash. These indicate awareness of the file's existence but
    /// not its full content.
    pub fn mark_metadata_observed(&mut self, path: &str) {
        self.files_metadata_observed.insert(path.to_string());
    }

    /// Returns a validity-aware summary for injection into the system prompt.
    ///
    /// Format:
    /// ```
    /// File context:
    /// - src/foo.rs: read, then edited; previous read may be stale
    /// - src/bar.rs: read, unchanged
    /// Files edited: src/baz.rs
    /// ```
    pub fn get_summary(&self) -> String {
        let mut lines = Vec::new();

        if !self.files_read.is_empty() {
            let mut paths: Vec<_> = self.files_read.keys().collect();
            paths.sort();

            for path in paths {
                let state = &self.files_read[path];
                let status = if state.edited_since_read {
                    "read, then edited; previous read may be stale"
                } else {
                    "read, unchanged"
                };
                lines.push(format!("- {}: {}", path, status));
            }
        }

        // Files that were edited but never read in this session
        let edited_only: Vec<_> = self.files_edited.iter()
            .filter(|p| !self.files_read.contains_key(*p))
            .map(|s| s.as_str())
            .collect();

        // Files observed via search/symbols/repo (not full content reads)
        let metadata_only: Vec<_> = self.files_metadata_observed.iter()
            .filter(|p| !self.files_read.contains_key(*p))
            .map(|s| s.as_str())
            .collect();

        let mut parts = Vec::new();
        if !lines.is_empty() {
            parts.push(format!("File context:\n{}", lines.join("\n")));
        }
        if !edited_only.is_empty() {
            let mut sorted = edited_only;
            sorted.sort();
            parts.push(format!("Files edited: {}", sorted.join(", ")));
        }
        if !metadata_only.is_empty() {
            let mut sorted = metadata_only;
            sorted.sort();
            parts.push(format!("Referenced via search/symbols: {}", sorted.join(", ")));
        }

        if parts.is_empty() {
            String::new()
        } else {
            parts.join("\n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_not_treated_as_file_read() {
        let mut ctx = FileContextTracker::new();
        ctx.mark_metadata_observed("src/bar.rs");
        let summary = ctx.get_summary();
        assert!(summary.contains("Referenced via search/symbols"));
        assert!(!summary.contains("read, unchanged"));
    }

    #[test]
    fn content_read_supersedes_metadata() {
        let mut ctx = FileContextTracker::new();
        ctx.mark_metadata_observed("src/baz.rs");
        ctx.mark_read("src/baz.rs", "hash1");
        let summary = ctx.get_summary();
        assert!(summary.contains("read, unchanged"));
        assert!(!summary.contains("Referenced via search/symbols"));
    }
}

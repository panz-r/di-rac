use std::collections::HashSet;

/// Tracks which files the agent has read or edited during a task.
pub struct FileContextTracker {
    pub files_read: HashSet<String>,
    pub files_edited: HashSet<String>,
}

impl FileContextTracker {
    pub fn new() -> Self {
        Self {
            files_read: HashSet::new(),
            files_edited: HashSet::new(),
        }
    }

    pub fn mark_read(&mut self, path: &str) {
        self.files_read.insert(path.to_string());
    }

    pub fn mark_edited(&mut self, path: &str) {
        self.files_edited.insert(path.to_string());
    }

    /// Returns a formatted summary for injection into the system prompt.
    pub fn get_summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.files_read.is_empty() {
            let mut files: Vec<_> = self.files_read.iter().map(|s| s.as_str()).collect();
            files.sort();
            parts.push(format!("Files read: {}", files.join(", ")));
        }
        if !self.files_edited.is_empty() {
            let mut files: Vec<_> = self.files_edited.iter().map(|s| s.as_str()).collect();
            files.sort();
            parts.push(format!("Files edited: {}", files.join(", ")));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("File context:\n{}", parts.join("\n"))
        }
    }
}

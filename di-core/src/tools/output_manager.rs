/// Manages large tool outputs by writing them to disk.
/// When a tool result exceeds the budget, the output content is saved to
/// .di/out/ and replaced with a reference + preview.
pub struct OutputManager {
    output_dir: std::path::PathBuf,
    budget_bytes: usize,
    preview_bytes: usize,
}

impl OutputManager {
    pub fn new() -> Self {
        let cwd = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("/"));
        Self::with_root(&cwd)
    }

    pub fn with_root(workspace_root: &std::path::Path) -> Self {
        let output_dir = workspace_root.join(".di").join("out");
        if let Err(e) = std::fs::create_dir_all(&output_dir) {
            eprintln!("[di-core] OutputManager: failed to create output dir: {}", e);
        }
        Self::cleanup_old_outputs(&output_dir);
        Self {
            output_dir,
            budget_bytes: 32768,
            preview_bytes: 4096,
        }
    }

    /// Extract the human-readable content from a tool result.
    /// For bash: extract stdout + stderr as plain text.
    /// For other tools: serialize the JSON.
    fn extract_content(result: &serde_json::Value) -> String {
        if let Some(stdout) = result.get("stdout").and_then(|v| v.as_str()) {
            // Bash-style result: combine stdout and stderr
            let mut content = stdout.to_string();
            if let Some(stderr) = result.get("stderr").and_then(|v| v.as_str()) {
                if !stderr.is_empty() {
                    content.push_str("\n--- stderr ---\n");
                    content.push_str(stderr);
                }
            }
            if let Some(exit_code) = result.get("exit_code").and_then(|v| v.as_i64()) {
                if exit_code != 0 {
                    content.push_str(&format!("\n--- exit code: {} ---", exit_code));
                }
            }
            content
        } else {
            result.to_string()
        }
    }

    /// If the result content exceeds the budget, write to disk and return
    /// a reference string with a preview. Otherwise return the original result.
    pub fn enforce_budget(
        &self,
        result: serde_json::Value,
        tool_name: &str,
    ) -> serde_json::Value {
        let content = Self::extract_content(&result);
        if content.len() <= self.budget_bytes {
            return result;
        }
        self.save_and_replace(&content, tool_name)
    }

    fn save_and_replace(&self, content: &str, tool_name: &str) -> serde_json::Value {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let filename = format!("{}_{}.txt", tool_name, now);
        let path = self.output_dir.join(&filename);

        if let Err(e) = std::fs::write(&path, content) {
            eprintln!("[di-core] OutputManager: failed to write {}: {}", path.display(), e);
            return serde_json::Value::String(format!("ERROR | Failed to save output: {}", e));
        }

        let size_kb = content.len() / 1024;
        let preview = self.truncate_preview(content);

        let output = format!(
            "[Output saved to .di/out/{} ({}KB)]\n{}\n\n--- [Output truncated. Use get_outputs read file={}] ---",
            filename, size_kb, preview, filename,
        );

        serde_json::Value::String(output)
    }

    fn truncate_preview<'a>(&self, content: &'a str) -> &'a str {
        let byte_limit = self.preview_bytes.min(content.len());
        let mut end = byte_limit;
        while end > 0 && !content.is_char_boundary(end) {
            end -= 1;
        }
        &content[..end]
    }

    fn cleanup_old_outputs(output_dir: &std::path::Path) {
        let max_age = std::time::Duration::from_secs(24 * 3600);
        if let Ok(entries) = std::fs::read_dir(output_dir) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    if let Ok(modified) = metadata.modified() {
                        if let Ok(age) = modified.elapsed() {
                            if age > max_age {
                                let _ = std::fs::remove_file(entry.path());
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn list_outputs(&self) -> Vec<String> {
        let mut outputs = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.output_dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    outputs.push(name.to_string());
                }
            }
        }
        outputs.sort();
        outputs.reverse();
        outputs
    }

    pub fn output_dir(&self) -> &std::path::Path {
        &self.output_dir
    }
}

/// Manages environment details (OS, shell, cwd, etc.) injected into the system prompt.
pub struct EnvironmentManager {
    pub details: Option<String>,
}

/// Sanitize a string for safe injection into system prompts.
/// Replaces characters that could be used for prompt injection or
/// formatting attacks (control chars, unusual whitespace, etc.).
fn sanitize_for_prompt(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\n' | '\r' | '\t' => ' ',
            c if c.is_control() => '?',
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

impl EnvironmentManager {
    pub fn new() -> Self {
        Self { details: None }
    }

    /// Build environment details from the current process state.
    pub fn gather(&mut self) {
        let cwd = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        let home = std::env::var("HOME")
            .unwrap_or_else(|_| "/root".to_string());
        let shell = std::env::var("SHELL")
            .unwrap_or_else(|_| "/bin/bash".to_string());
        let user = std::env::var("USER")
            .unwrap_or_else(|_| "unknown".to_string());

        self.details = Some(format!(
            "Environment:\n- OS: linux\n- CWD: {}\n- HOME: {}\n- SHELL: {}\n- USER: {}",
            sanitize_for_prompt(&cwd),
            sanitize_for_prompt(&home),
            sanitize_for_prompt(&shell),
            sanitize_for_prompt(&user),
        ));
    }

    pub fn get_details(&self) -> Option<&str> {
        self.details.as_deref()
    }
}

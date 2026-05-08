/// Manages environment details (OS, shell, cwd, etc.) injected into the system prompt.
pub struct EnvironmentManager {
    pub details: Option<String>,
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
            cwd, home, shell, user
        ));
    }

    pub fn get_details(&self) -> Option<&str> {
        self.details.as_deref()
    }
}

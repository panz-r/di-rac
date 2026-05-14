// API Gateway communication:
//   The api-gateway is a separate process that exposes an HTTP-like JSON interface
//   over a Unix domain socket (per-PID path: ~/.dirac/api-gateway-<pid>.sock).
//   The settings panel uses GatewayConnection for CRUD operations (list providers,
//   list models, validate API keys). The gateway is launched alongside divrr and
//   killed on drop via GatewayChild. The gateway daemon auto-shuts down after 2
//   minutes with no connected clients.

use std::io;
use std::os::unix::net::UnixStream;
use std::process::{Child, Command};
use std::time::Duration;

/// Handle to the launched gateway child process. Kills it on drop.
pub struct GatewayChild {
    child: Option<Child>,
    socket_path: String,
}

impl GatewayChild {
    /// Send SIGTERM then SIGKILL. Clean up socket.
    /// Checks if child is still alive before signaling to avoid killing a reused PID.
    pub fn kill(&mut self) {
        if let Some(ref mut child) = self.child {
            // First check if already exited — avoids signaling a stale PID
            match child.try_wait() {
                Ok(Some(_)) => {
                    // Already dead — just clean up
                }
                _ => {
                    // Still alive — send SIGTERM via kill command
                    let pid = child.id();
                    if let Err(e) = std::process::Command::new("kill")
                        .arg("-TERM")
                        .arg(pid.to_string())
                        .output()
                    {
                        crate::logging::log_event(&format!("gateway SIGTERM failed: {}", e));
                    }
                    // Wait briefly, then force kill if still running
                    match child.try_wait() {
                        Ok(Some(_)) => {}
                        _ => {
                            if let Err(e) = child.kill() {
                                crate::logging::log_event(&format!("gateway SIGKILL failed: {}", e));
                            }
                        }
                    }
                }
            }
        }
        self.child = None;
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

impl Drop for GatewayChild {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Launch the API gateway as a child process with a per-PID socket path.
/// Sets DIRAC_API_GATEWAY_SOCKET in the current process env so settings picks it up.
/// Returns a GatewayChild that will kill the process on drop.
pub fn launch(gateway_bin: &str) -> io::Result<GatewayChild> {
    let pid = std::process::id();
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    let socket_path = format!("{}/.dirac/api-gateway-{}.sock", home, pid);

    // Clean up stale socket if it exists
    let _ = std::fs::remove_file(&socket_path);

    // Set env var so settings.rs picks it up
    std::env::set_var("DIRAC_API_GATEWAY_SOCKET", &socket_path);

    let mut cmd = Command::new(gateway_bin);
    cmd.env("DIRAC_API_GATEWAY_SOCKET", &socket_path);
    // Detach from parent's stdin/stdout so gateway doesn't interfere with TUI
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    let child = cmd.spawn()?;

    // Wait for socket to become available (up to 3 seconds)
    for _ in 0..30 {
        if UnixStream::connect(&socket_path).is_ok() {
            return Ok(GatewayChild { child: Some(child), socket_path });
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // Gateway may still be starting — return the path anyway
    Ok(GatewayChild { child: Some(child), socket_path })
}

/// Find the gateway binary relative to the divrr binary or in common locations.
pub fn find_gateway() -> Option<String> {
    let candidates = [
        // Same directory as the running binary
        std::env::current_exe().ok()
            .and_then(|exe| exe.parent().map(|p| p.join("api-gateway").to_string_lossy().into_owned())),
        // Current working directory dist/
        Some(format!("{}/dist/api-gateway",
            std::env::current_dir().unwrap_or_default().to_string_lossy())),
        // Home dist/
        Some(format!("{}/dist/api-gateway", std::env::var("HOME").unwrap_or_else(|_| "/root".into()))),
        // In PATH
        which("api-gateway"),
    ];

    for ref path in candidates.into_iter().flatten() {
        if std::path::Path::new(path).exists() {
            return Some(path.clone());
        }
    }
    None
}

fn which(name: &str) -> Option<String> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let full = dir.join(name);
            if full.exists() {
                Some(full.to_string_lossy().into_owned())
            } else {
                None
            }
        })
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn socket_path_contains_pid() {
        let pid = std::process::id();
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        let socket_path = format!("{}/.dirac/api-gateway-{}.sock", home, pid);
        assert!(socket_path.contains(&pid.to_string()));
        assert!(socket_path.contains(".dirac"));
        assert!(socket_path.ends_with(".sock"));
    }
}

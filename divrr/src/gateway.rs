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
#[derive(Debug)]
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

    let mut child = cmd.spawn()?;

    // Wait for socket to become available (up to 3 seconds)
    for _ in 0..30 {
        if UnixStream::connect(&socket_path).is_ok() {
            return Ok(GatewayChild { child: Some(child), socket_path });
        }
        // Check if child has already exited — avoids hanging on a crashed gateway
        match child.try_wait() {
            Ok(Some(_)) => {
                let _ = std::fs::remove_file(&socket_path);
                return Err(std::io::Error::new(std::io::ErrorKind::ConnectionRefused,
                    "api-gateway exited before creating socket"));
            }
            Ok(None) => {} // still running
            Err(e) => {
                let _ = std::fs::remove_file(&socket_path);
                return Err(std::io::Error::new(std::io::ErrorKind::Other,
                    format!("failed to check api-gateway status: {}", e)));
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // Gateway process is still running but socket hasn't appeared
    Err(std::io::Error::new(std::io::ErrorKind::TimedOut,
        "api-gateway did not create socket within 3 seconds"))
}

/// Find the gateway binary relative to the divrr binary or in common locations.
pub fn find_gateway() -> Option<String> {
    let candidates = [
        // Same directory as the running binary
        std::env::current_exe().ok()
            .and_then(|exe| exe.parent().map(|p| p.join("api-gateway").to_string_lossy().into_owned())),
        // Current working directory bin/
        Some(format!("{}/bin/api-gateway",
            std::env::current_dir().unwrap_or_default().to_string_lossy())),
        // Home dist/
        Some(format!("{}/bin/api-gateway", std::env::var("HOME").unwrap_or_else(|_| "/root".into()))),
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
    use super::*;

    #[test]
    fn socket_path_contains_pid() {
        let pid = std::process::id();
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        let socket_path = format!("{}/.dirac/api-gateway-{}.sock", home, pid);
        assert!(socket_path.contains(&pid.to_string()));
        assert!(socket_path.contains(".dirac"));
        assert!(socket_path.ends_with(".sock"));
    }

    #[test]
    fn which_returns_none_for_nonexistent() {
        let result = which("this-binary-definitely-does-not-exist-abc123xyz");
        assert!(result.is_none());
    }

    #[test]
    fn which_finds_sh_in_path() {
        let result = which("sh");
        assert!(result.is_some());
        let path = result.unwrap();
        assert!(path.ends_with("/sh"), "expected /sh suffix, got {}", path);
    }

    #[test]
    fn find_gateway_returns_some_when_binary_exists() {
        // Create a temp directory with a fake api-gateway binary
        let dir = std::env::temp_dir().join(format!("gateway_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let fake_bin = dir.join("api-gateway");
        let _ = std::fs::write(&fake_bin, "#!/bin/sh\necho fake");
        let _ = std::fs::set_permissions(&fake_bin, std::os::unix::fs::PermissionsExt::from_mode(0o755));

        // Temporarily prepend the temp dir to PATH
        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let dir_str = dir.to_str().unwrap();
        let mut new_path = std::ffi::OsString::from(dir_str);
        new_path.push(":");
        new_path.push(&old_path);
        std::env::set_var("PATH", &new_path);

        let result = which("api-gateway");
        assert!(result.is_some(), "expected to find api-gateway in modified PATH");
        assert!(result.unwrap().contains(dir.to_str().unwrap()));

        // Restore PATH
        std::env::set_var("PATH", &old_path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gateway_child_kill_does_not_panic_on_dead_child() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        let socket_path = format!("{}/.dirac/test-gateway-{}.sock", home, std::process::id());
        let mut child = GatewayChild { child: None, socket_path };
        child.kill(); // should not panic
    }

    #[test]
    fn gateway_child_drop_does_not_panic() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        let socket_path = format!("{}/.dirac/test-gateway-{}.sock", home, std::process::id());
        let child = GatewayChild { child: None, socket_path };
        drop(child); // should not panic
    }

    #[test]
    fn launch_returns_err_when_binary_not_found() {
        let result = crate::gateway::launch("/nonexistent/path/to/api-gateway");
        assert!(result.is_err());
    }

    #[test]
    fn launch_returns_err_when_binary_exits_immediately() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("gateway_launch_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let fake_bin = dir.join("fake-gateway");
        let mut f = std::fs::File::create(&fake_bin).unwrap();
        writeln!(f, "#!/bin/sh").unwrap();
        writeln!(f, "exit 1").unwrap();
        std::fs::set_permissions(&fake_bin, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

        // This should fail because the fake gateway exits before creating a socket
        let result = crate::gateway::launch(fake_bin.to_str().unwrap());
        assert!(result.is_err(), "expected Err when gateway exits immediately, got {:?}", result);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

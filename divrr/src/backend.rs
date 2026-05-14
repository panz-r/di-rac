use crate::message::{CoreEvent, FrontendMessage};
use color_eyre::eyre::{eyre, Result};
use futures::StreamExt;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio_util::codec::{FramedRead, LinesCodec};

pub struct DiCoreBackend {
    child: Child,
    stdin: tokio::process::ChildStdin,
    event_rx: mpsc::Receiver<Result<CoreEvent>>,
}

impl DiCoreBackend {
    /// Spawn di-core as a child process with piped stdio.
    pub fn spawn(core_path: &str) -> Result<Self> {
        // Find command daemon binary: check env var, then next to di-core, then known locations
        let cmd_binary = std::env::var("DIRAC_COMMAND_BINARY").ok()
            .filter(|p| std::path::Path::new(p).exists())
            .unwrap_or_else(|| {
                let candidates = [
                    // Next to di-core binary
                    std::path::Path::new(core_path).parent().map(|p| p.join("di-rvv-cmd")),
                ];
                candidates.iter()
                    .filter_map(|c| c.as_ref())
                    .find(|p| p.exists())
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "di-rvv-cmd".to_string())
            });

        // Find analyzer daemon binary
        let analyzer_binary = std::env::var("DIRAC_ANALYZER_BINARY").ok()
            .filter(|p| std::path::Path::new(p).exists())
            .unwrap_or_else(|| {
                let candidates = [
                    std::path::Path::new(core_path).parent().map(|p| p.join("di-rvv-analyzer")),
                ];
                candidates.iter()
                    .filter_map(|c| c.as_ref())
                    .find(|p| p.exists())
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "di-rvv-analyzer".to_string())
            });

        let mut child = Command::new(core_path)
            .env("DIRAC_COMMAND_BINARY", &cmd_binary)
            .env("DIRAC_ANALYZER_BINARY", &analyzer_binary)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| eyre!("Failed to spawn di-core: {}", e))?;

        let stdin = child.stdin.take().ok_or_else(|| eyre!("Failed to open di-core stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| eyre!("Failed to open di-core stdout"))?;

        let (event_tx, event_rx) = mpsc::channel(256);

        // Background task: read NDJSON lines from di-core stdout
        let framed = FramedRead::new(stdout, LinesCodec::new_with_max_length(10 * 1024 * 1024));
        tokio::spawn(async move {
            let mut stream = framed;
            while let Some(result) = stream.next().await {
                match result {
                    Ok(line) => {
                        let line: String = line;
                        if line.trim().is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<CoreEvent>(&line) {
                            Ok(event) => {
                                if event_tx.send(Ok(event)).await.is_err() {
                                    break;
                                }
                            }
                            Err(e) => {
                                if event_tx.send(Err(color_eyre::eyre::eyre!(
                                    "Parse error: {} — line: {}",
                                    e,
                                    &line[..line.len().min(200)]
                                )))
                                .await
                                .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        // A single bad line (e.g. exceeding 10 MB line limit) should
                        // not terminate the session. Log via CoreError and continue
                        // reading the next line.
                        let _ = event_tx
                            .send(Err(color_eyre::eyre::eyre!("IO error: {}", e)))
                            .await;
                        continue;
                    }
                }
            }
        });

        // Background task: write di-core stderr to log file
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
                let log_path = std::path::Path::new(&home).join(".dirac").join("di-core.log");

                let rotate = || {
                    if let Ok(meta) = std::fs::metadata(&log_path) {
                        if meta.len() > 10_485_760 {
                            if let Ok(data) = std::fs::read(&log_path) {
                                let keep = 1_048_576; // 1 MiB
                                let start = data.len().saturating_sub(keep);
                                let start = data[start..].iter().position(|&b| b == b'\n')
                                    .map(|p| start + p + 1)
                                    .unwrap_or(start);
                                let _ = std::fs::write(&log_path, &data[start..]);
                            }
                        }
                    }
                };

                // Initial rotation check
                rotate();

                let framed = FramedRead::new(stderr, LinesCodec::new_with_max_length(1_048_576));
                let mut stream = framed;
                if let Ok(file) = std::fs::OpenOptions::new().append(true).create(true).open(&log_path) {
                    use std::io::{BufWriter, Write};
                    let mut writer = BufWriter::new(file);
                    let mut line_count = 0u64;
                    while let Some(Ok(line)) = stream.next().await {
                        let ts = chrono::Local::now().format("%H:%M:%S%.3f");
                        let _ = writeln!(writer, "[{}] {}", ts, line);
                        line_count += 1;
                        // Recheck file size every 1000 lines so large bursts are trimmed
                        if line_count.is_multiple_of(1000) {
                            writer.flush().ok();
                            rotate();
                        }
                    }
                }
            });
        }

        Ok(Self {
            child,
            stdin,
            event_rx,
        })
    }

    /// Take ownership of the event receiver (for forwarding into the main loop).
    pub fn take_event_rx(&mut self) -> mpsc::Receiver<Result<CoreEvent>> {
        let mut rx = mpsc::channel(1).1; // dummy
        std::mem::swap(&mut self.event_rx, &mut rx);
        rx
    }

    /// Send a FrontendMessage to di-core's stdin.
    pub async fn send(&mut self, msg: &FrontendMessage) -> Result<()> {
        let mut json = serde_json::to_string(msg)?;
        json.push('\n');
        self.stdin.write_all(json.as_bytes()).await?;
        self.stdin.flush().await?;
        Ok(())
    }
}

impl Drop for DiCoreBackend {
    fn drop(&mut self) {
        // Kill the child process. Avoid blocking sleeps in drop — the OS
        // will reap the zombie when our process exits, and try_wait is a
        // non-blocking best-effort check.
        let _ = self.child.start_kill();
        let _ = self.child.try_wait();
    }
}

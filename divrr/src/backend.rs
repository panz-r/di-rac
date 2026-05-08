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
        let mut child = Command::new(core_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| eyre!("Failed to spawn di-core: {}", e))?;

        let stdin = child.stdin.take().ok_or_else(|| eyre!("Failed to open di-core stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| eyre!("Failed to open di-core stdout"))?;

        let (event_tx, event_rx) = mpsc::channel(256);

        // Background task: read NDJSON lines from di-core stdout
        let framed = FramedRead::new(stdout, LinesCodec::new());
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
                        let _ = event_tx
                            .send(Err(color_eyre::eyre::eyre!("IO error: {}", e)))
                            .await;
                        break;
                    }
                }
            }
        });

        // Background task: log di-core stderr
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let framed = FramedRead::new(stderr, LinesCodec::new());
                let mut stream = framed;
                while let Some(Ok(line)) = stream.next().await {
                    eprintln!("[di-core] {}", line);
                }
            });
        }

        Ok(Self {
            child,
            stdin,
            event_rx,
        })
    }

    /// Receive the next CoreEvent from di-core (non-blocking with select!).
    pub async fn recv_event(&mut self) -> Option<Result<CoreEvent>> {
        self.event_rx.recv().await
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

    /// Check if the di-core process is still running.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for DiCoreBackend {
    fn drop(&mut self) {
        // Best-effort kill on drop
        let _ = self.child.start_kill();
    }
}

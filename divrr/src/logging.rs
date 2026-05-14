use std::sync::Mutex;

static LOG_MUTEX: Mutex<()> = Mutex::new(());

/// Append a timestamped line to ~/.dirac/divrr.log (best-effort, never fails).
/// When the log exceeds 1MB, keeps the most recent 256KB.
pub fn log_event(msg: &str) {
    let _lock = LOG_MUTEX.lock().unwrap();
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    let path = std::path::Path::new(&home).join(".dirac").join("divrr.log");
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > 1_048_576 {
            if let Ok(data) = std::fs::read(&path) {
                let keep = 262_144;
                let start = data.len().saturating_sub(keep);
                let start = data[start..].iter().position(|&b| b == b'\n')
                    .map_or(start, |p| start + p + 1);
                let _ = std::fs::write(&path, &data[start..]);
            }
        }
    }
    let _ = std::fs::OpenOptions::new().append(true).create(true).open(&path)
        .map(|mut f| {
            use std::io::Write;
            let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
            let _ = writeln!(f, "[{}] {}", ts, msg);
        });
}

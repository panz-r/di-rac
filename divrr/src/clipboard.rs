use std::io::Write;

/// Copy text to clipboard, trying multiple methods:
/// 1. xclip (most reliable on mixed Wayland/X11 setups)
/// 2. arboard (native OS clipboard)
/// 3. wl-copy (Wayland native)
/// 4. OSC 52 escape sequence (terminal-native, works in tmux/SSH)
pub fn copy_to_clipboard(text: &str) -> Result<(), crate::errors::ClipboardError> {
    use crate::errors::ClipboardError;

    // 1. Try xclip
    if let Ok(mut child) = std::process::Command::new("xclip")
        .args(["-selection", "clipboard"])
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        if child.wait().map(|s| s.success()).unwrap_or(false) {
            return Ok(());
        }
    }

    // 2. Try arboard
    if arboard::Clipboard::new().and_then(|mut cb| cb.set_text(text)).is_ok() {
        return Ok(());
    }

    // 3. Try wl-copy (Wayland native)
    if let Ok(mut child) = std::process::Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        if child.wait().map(|s| s.success()).unwrap_or(false) {
            return Ok(());
        }
    }

    // 4. OSC 52 escape sequence
    let encoded = {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(text.as_bytes())
    };
    let seq = format!("\x1b]52;c;{}\x07", encoded);
    if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
        if f.write_all(seq.as_bytes()).is_ok() {
            return Ok(());
        }
    }

    Err(ClipboardError::NoMethod)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_empty_input() {
        // Empty string should not panic — success depends on environment
        let _ = copy_to_clipboard("");
    }

    #[test]
    fn test_clipboard_large_input() {
        // 100KB string should not panic or overflow
        let large = "x".repeat(100_000);
        let _ = copy_to_clipboard(&large);
    }

    #[test]
    fn test_clipboard_error_display() {
        let err = crate::errors::ClipboardError::NoMethod;
        let msg = err.to_string();
        assert!(msg.contains("xclip"));
        assert!(msg.contains("OSC 52"));
    }

    #[test]
    fn test_clipboard_unicode_input() {
        // Unicode content should not panic
        let unicode = "Hello 世界 🌍 café";
        let _ = copy_to_clipboard(unicode);
    }
}

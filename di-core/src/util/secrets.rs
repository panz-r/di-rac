use regex::Regex;
use std::sync::LazyLock;

static SECRET_REGEXES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    let patterns: &[&str] = &[
        r"sk-[a-zA-Z0-9]{20,}",
        r"AKIA[0-9A-Z]{16}",
        r"ghp_[a-zA-Z0-9]{36}",
        r"xox[bpaors]-[a-zA-Z0-9\-]+",
        r#"api[_\-]?key\s*[:=]\s*["']?[a-zA-Z0-9]{20,}"#,
    ];
    patterns.iter().filter_map(|p| Regex::new(p).ok()).collect()
});

/// Returns true if text matches any known secret pattern.
#[allow(dead_code)]
pub fn scan_for_secrets(text: &str) -> bool {
    SECRET_REGEXES.iter().any(|re| re.is_match(text))
}

/// Replaces all secret matches in text with `[REDACTED]`.
pub fn redact_secrets(text: &str) -> String {
    let mut result = text.to_string();
    for re in SECRET_REGEXES.iter() {
        result = re.replace_all(&result, "[REDACTED]").to_string();
    }
    result
}

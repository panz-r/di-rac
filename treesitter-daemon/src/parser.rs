use crate::error::{AnalyzerError, ErrorCode};
use crate::language::Language;
use std::fs;
use std::path::Path;

/// Parsed source with its AST and metadata.
pub struct ParsedSource {
    pub source: String,
    pub language: Language,
    pub tree: tree_sitter::Tree,
}

/// Parse source code with automatic or explicit language detection.
/// If `workspace_root` is set, file paths are validated to be within it.
pub fn parse_source(
    file_path: Option<&Path>,
    content: Option<&str>,
    language_override: Option<&str>,
    workspace_root: Option<&Path>,
) -> Result<ParsedSource, AnalyzerError> {
    // Validate file path against workspace root
    if let (Some(path), Some(root)) = (file_path, workspace_root) {
        validate_path_in_workspace(path, root)?;
    }

    // Determine language
    let language = if let Some(lang_str) = language_override {
        Language::from_str(lang_str)
            .ok_or_else(|| AnalyzerError::unsupported_language(lang_str))?
    } else if let Some(path) = file_path {
        Language::from_path(path).ok_or_else(|| {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("(none)");
            AnalyzerError::unsupported_language(format!(
                "Cannot detect language for extension: .{}",
                ext
            ))
        })?
    } else {
        return Err(AnalyzerError::new(
            ErrorCode::InvalidCommand,
            "Either 'file' or 'content' must be provided, or 'language' must be specified explicitly.",
        ));
    };

    // Read source
    let source = if let Some(path) = file_path {
        let metadata = fs::metadata(path).map_err(|e| {
            AnalyzerError::file_not_found(format!("{}: {}", path.display(), e))
        })?;
        const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;
        if metadata.len() > MAX_FILE_SIZE {
            return Err(AnalyzerError::new(
                ErrorCode::ParseError,
                format!(
                    "File too large: {} ({} bytes, max {} bytes)",
                    path.display(),
                    metadata.len(),
                    MAX_FILE_SIZE
                ),
            ));
        }
        fs::read_to_string(path)
            .map_err(|e| AnalyzerError::file_not_found(format!("{}: {}", path.display(), e)))?
    } else if let Some(content_str) = content {
        const MAX_CONTENT_SIZE: usize = 10 * 1024 * 1024;
        if content_str.len() > MAX_CONTENT_SIZE {
            return Err(AnalyzerError::new(
                ErrorCode::ParseError,
                format!(
                    "Content too large: {} bytes (max {} bytes)",
                    content_str.len(),
                    MAX_CONTENT_SIZE
                ),
            ));
        }
        content_str.to_string()
    } else {
        return Err(AnalyzerError::new(
            ErrorCode::InvalidCommand,
            "Either 'file' or 'content' must be provided.",
        ));
    };

    // Parse
    let mut parser = language.parser();
    let tree = parser
        .parse(&source, None)
        .ok_or_else(|| AnalyzerError::parse_error("Failed to parse source"))?;

    Ok(ParsedSource {
        source,
        language,
        tree,
    })
}

/// Validate that `path` is within `workspace_root` (canonicalized comparison).
fn validate_path_in_workspace(path: &Path, root: &Path) -> Result<(), AnalyzerError> {
    let canonical_path = path.canonicalize().map_err(|e| {
        AnalyzerError::file_not_found(format!("{}: {}", path.display(), e))
    })?;
    let canonical_root = root.canonicalize().map_err(|e| {
        AnalyzerError::internal_error(format!("Cannot canonicalize workspace root: {}", e))
    })?;

    if !canonical_path.starts_with(&canonical_root) {
        return Err(AnalyzerError::path_outside_workspace(
            path.display().to_string(),
        ));
    }
    Ok(())
}

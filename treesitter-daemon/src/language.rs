use serde::{Deserialize, Serialize};
use std::path::Path;
use tree_sitter::Parser;

/// Supported languages for analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Python,
    #[serde(rename = "typescript")]
    TypeScript,
    JavaScript,
    C,
    #[serde(rename = "cpp")]
    Cpp,
    Rust,
    Go,
    Bash,
}

impl Language {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "py" | "py3" | "pyw" => Some(Language::Python),
            "ts" | "tsx" => Some(Language::TypeScript),
            "js" | "jsx" | "mjs" | "cjs" => Some(Language::JavaScript),
            "c" => Some(Language::C),
            "cpp" | "cc" | "cxx" | "c++" | "hpp" | "hh" | "hxx" | "h" => Some(Language::Cpp),
            "rs" => Some(Language::Rust),
            "go" => Some(Language::Go),
            "sh" | "bash" | "zsh" | "bats" => Some(Language::Bash),
            _ => None,
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|e| e.to_str())
            .and_then(Self::from_extension)
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "python" | "py" => Some(Language::Python),
            "typescript" | "ts" | "tsx" => Some(Language::TypeScript),
            "javascript" | "js" | "jsx" => Some(Language::JavaScript),
            "c" => Some(Language::C),
            "cpp" | "c++" | "cxx" => Some(Language::Cpp),
            "rust" | "rs" => Some(Language::Rust),
            "go" | "golang" => Some(Language::Go),
            "bash" | "sh" | "shell" => Some(Language::Bash),
            _ => None,
        }
    }

    pub fn parser(&self) -> Parser {
        let mut parser = Parser::new();
        let lang = self.tree_sitter_language();
        parser
            .set_language(&lang)
            .expect("tree-sitter language should load successfully");
        parser
    }

    pub fn tree_sitter_language(&self) -> tree_sitter::Language {
        match self {
            Language::Python => tree_sitter_python::LANGUAGE.into(),
            Language::TypeScript | Language::JavaScript => {
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
            }
            Language::C => tree_sitter_c::LANGUAGE.into(),
            Language::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Language::Rust => tree_sitter_rust::LANGUAGE.into(),
            Language::Go => tree_sitter_go::LANGUAGE.into(),
            Language::Bash => tree_sitter_bash::LANGUAGE.into(),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Language::Python => "python",
            Language::TypeScript => "typescript",
            Language::JavaScript => "javascript",
            Language::C => "c",
            Language::Cpp => "cpp",
            Language::Rust => "rust",
            Language::Go => "go",
            Language::Bash => "bash",
        }
    }

    pub fn all() -> &'static [Language] {
        &[
            Language::Python,
            Language::TypeScript,
            Language::JavaScript,
            Language::C,
            Language::Cpp,
            Language::Rust,
            Language::Go,
            Language::Bash,
        ]
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

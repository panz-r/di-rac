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
    Java,
    #[serde(rename = "csharp")]
    CSharp,
    Ruby,
    #[serde(rename = "php")]
    Php,
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
            "java" => Some(Language::Java),
            "cs" | "csx" => Some(Language::CSharp),
            "rb" | "rake" | "gemspec" => Some(Language::Ruby),
            "php" | "phtml" | "php3" | "php4" | "php5" | "phps" | "phpt" => Some(Language::Php),
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
            "java" => Some(Language::Java),
            "csharp" | "c#" | "cs" => Some(Language::CSharp),
            "ruby" | "rb" => Some(Language::Ruby),
            "php" => Some(Language::Php),
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

    /// Try to create a parser, returning an error on ABI mismatch
    /// instead of panicking.  Useful for graceful degradation.
    pub fn try_parser(&self) -> Result<Parser, tree_sitter::LanguageError> {
        let mut parser = Parser::new();
        let lang = self.tree_sitter_language();
        parser.set_language(&lang)?;
        Ok(parser)
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
            Language::Java => tree_sitter_java::LANGUAGE.into(),
            Language::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
            Language::Ruby => tree_sitter_ruby::LANGUAGE.into(),
            Language::Php => tree_sitter_php::LANGUAGE_PHP.into(),
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
            Language::Java => "java",
            Language::CSharp => "csharp",
            Language::Ruby => "ruby",
            Language::Php => "php",
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
            Language::Java,
            Language::CSharp,
            Language::Ruby,
            Language::Php,
        ]
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

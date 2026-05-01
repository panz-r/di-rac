mod commands;
mod error;
mod extractor;
mod language;
mod parser;
mod queries;
mod skeleton;

use clap::Parser;
use error::AnalyzerError;
use language::Language;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

/// Dirac Analyzer – persistent tree-sitter structural analysis daemon.
#[derive(Parser)]
#[command(name = "dirac-analyzer", version = env!("CARGO_PKG_VERSION"))]
struct Cli {
    /// Run a single command and exit (default: daemon mode reading stdin line-by-line)
    #[arg(short = '1', long = "oneshot")]
    oneshot: bool,

    /// Command to run (only with --oneshot): outline, skeleton, symbols, handles
    #[arg(short, long)]
    command: Option<String>,

    /// Path to the source file (only with --oneshot)
    #[arg(short, long)]
    file: Option<String>,

    /// Source code content (only with --oneshot)
    #[arg(long)]
    content: Option<String>,

    /// Language override: python, typescript, javascript
    #[arg(short, long)]
    language: Option<String>,

    /// Restrict file access to this workspace root directory
    #[arg(short = 'w', long)]
    workspace_root: Option<String>,
}

/// JSON request structure — matches the API contract.
#[derive(serde::Deserialize, Debug)]
struct JsonRequest {
    command: String,
    #[serde(default)]
    id: Option<serde_json::Value>,
    file: Option<String>,
    content: Option<String>,
    language: Option<String>,
}

fn main() {
    let cli = Cli::parse();

    // Pre-load all grammars at startup to keep them warm
    eprintln!("dirac-analyzer: pre-loading grammars...");
    preload_grammars();
    eprintln!("dirac-analyzer: ready");

    let workspace_root = cli.workspace_root.as_ref().map(PathBuf::from);
    let workspace_root_ref = workspace_root.as_ref();

    if cli.oneshot {
        // Single-request mode (manual testing / legacy)
        let id: Option<serde_json::Value> = None;
        let command = match &cli.command {
            Some(c) => c.clone(),
            None => {
                let err = AnalyzerError::invalid_command("missing --command with --oneshot");
                println!("{}", err.to_json_response(id.as_ref()));
                return;
            }
        };
        let response = process_request(
            &command,
            id,
            cli.file.as_deref(),
            cli.content.as_deref(),
            cli.language.as_deref(),
            workspace_root_ref,
        );
        println!("{}", response);
    } else {
        // Daemon mode: read stdin line by line indefinitely
        run_daemon(workspace_root_ref);
    }
}

/// Pre-load and discard a tree for each grammar to warm the parser.
fn preload_grammars() {
    for lang in Language::all() {
        let mut parser = lang.parser();
        let dummy = match lang {
            Language::Python => "x = 1",
            Language::TypeScript | Language::JavaScript => "let x = 1;",
            Language::C => "int x = 1;",
            Language::Cpp => "int x = 1;",
            Language::Rust => "let x = 1;",
            Language::Go => "var x = 1",
            Language::Bash => "x=1",
        };
        // Parse-and-discard to ensure grammar is fully loaded into memory
        let _ = parser.parse(dummy, None);
    }
}

/// Daemon loop: read newline-delimited JSON requests from stdin, write JSON responses to stdout.
fn run_daemon(workspace_root: Option<&PathBuf>) {
    let stdin = io::stdin();
    let reader = stdin.lock();

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => break, // stdin closed
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue; // skip blank lines
        }

        let request: JsonRequest = match serde_json::from_str(trimmed) {
            Ok(req) => req,
            Err(e) => {
                let err = AnalyzerError::new(
                    error::ErrorCode::InvalidCommand,
                    format!("Failed to parse JSON request: {}", e),
                );
                writeln!(io::stdout(), "{}", err.to_json_response(None)).ok();
                io::stdout().flush().ok();
                continue;
            }
        };

        let response = process_request(
            &request.command,
            request.id,
            request.file.as_deref(),
            request.content.as_deref(),
            request.language.as_deref(),
            workspace_root,
        );

        writeln!(io::stdout(), "{}", response).ok();
        io::stdout().flush().ok();
    }

    eprintln!("dirac-analyzer: stdin closed, shutting down");
}

/// Core processing: parse, dispatch, return JSON string.
fn process_request(
    command: &str,
    id: Option<serde_json::Value>,
    file: Option<&str>,
    content: Option<&str>,
    language_override: Option<&str>,
    workspace_root: Option<&PathBuf>,
) -> String {
    let file_path = file.map(PathBuf::from);
    let file_path_ref = file_path.as_deref();
    let workspace_root_ref = workspace_root.map(|p| p.as_path());

    let parsed = match parser::parse_source(
        file_path_ref,
        content,
        language_override,
        workspace_root_ref,
    ) {
        Ok(p) => p,
        Err(e) => return e.to_json_response(id.as_ref()),
    };

    match commands::dispatch(parsed, command, id.clone()) {
        Ok(output) => output.to_json(),
        Err(e) => e.to_json_response(id.as_ref()),
    }
}

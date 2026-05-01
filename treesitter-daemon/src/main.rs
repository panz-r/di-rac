mod commands;
mod error;
mod extractor;
mod language;
mod parser;
mod queries;
mod skeleton;
mod cache;

use clap::Parser;
use error::AnalyzerError;
use language::Language;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use crate::cache::ParseCache;

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

    /// Symbol handle for expand-symbol command (e.g. "fn:login")
    #[arg(long)]
    handle: Option<String>,

    /// List of file paths for batch / warm-cache commands (comma-separated)
    #[arg(long, value_delimiter = ',')]
    files: Option<Vec<String>>,

    /// Subcommand for batch processing
    #[arg(long)]
    subcommand: Option<String>,

    /// Search query for search-symbols
    #[arg(long)]
    query: Option<String>,

    /// Kind filter for search-symbols (function, class, method)
    #[arg(long)]
    kind: Option<String>,

    /// Root directory for repo-map
    #[arg(long)]
    root: Option<String>,

    /// Max results for search-symbols
    #[arg(long)]
    max_results: Option<usize>,
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
    /// Symbol handle (e.g. "fn:login", "class:AuthService") for expand-symbol command.
    #[serde(default)]
    handle: Option<String>,
    /// List of file paths for batch / warm-cache commands.
    #[serde(default)]
    files: Option<Vec<String>>,
    /// Subcommand for batch processing (e.g. "outline", "symbols").
    #[serde(default)]
    subcommand: Option<String>,
    /// Search query string for search-symbols.
    #[serde(default)]
    query: Option<String>,
    /// Optional kind filter for search-symbols (e.g. "function", "class").
    #[serde(default)]
    kind: Option<String>,
    /// Root directory for repo-map.
    #[serde(default)]
    root: Option<String>,
    /// Maximum number of results for search-symbols.
    #[serde(default)]
    max_results: Option<usize>,
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
        let mut cache = ParseCache::new();
        let response = process_request(
            &command,
            id,
            cli.file.as_deref(),
            cli.content.as_deref(),
            cli.language.as_deref(),
            workspace_root_ref,
            &mut cache,
            cli.handle.as_deref(),
            cli.files.as_ref(),
            cli.subcommand.as_deref(),
            cli.query.as_deref(),
            cli.kind.as_deref(),
            cli.root.as_deref(),
            cli.max_results,
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
        let mut parser = match lang.try_parser() {
            Ok(p) => p,
            Err(_) => {
                eprintln!("dirac-analyzer: warning: failed to load grammar for {}", lang.as_str());
                continue;
            }
        };
        let dummy = match lang {
            Language::Python => "x = 1",
            Language::TypeScript | Language::JavaScript => "let x = 1;",
            Language::C => "int x = 1;",
            Language::Cpp => "int x = 1;",
            Language::Rust => "let x = 1;",
            Language::Go => "var x = 1",
            Language::Bash => "x=1",
            Language::Java => "class X { int x = 1; }",
            Language::CSharp => "class X { int x = 1; }",
            Language::Ruby => "def x; 1; end",
            Language::Php => "<?php $x = 1;",
        };
        // Parse-and-discard to ensure grammar is fully loaded into memory
        let _ = parser.parse(dummy, None);
    }
}

/// Daemon loop: read newline-delimited JSON requests from stdin, write JSON responses to stdout.
fn run_daemon(workspace_root: Option<&PathBuf>) {
    let mut cache = ParseCache::new();
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
            &mut cache,
            request.handle.as_deref(),
            request.files.as_ref(),
            request.subcommand.as_deref(),
            request.query.as_deref(),
            request.kind.as_deref(),
            request.root.as_deref(),
            request.max_results,
        );

        writeln!(io::stdout(), "{}", response).ok();
        io::stdout().flush().ok();
    }

    eprintln!("dirac-analyzer: stdin closed, shutting down");
}

/// Core processing: parse, dispatch, return JSON string.
#[allow(clippy::too_many_arguments)]
fn process_request(
    command: &str,
    id: Option<serde_json::Value>,
    file: Option<&str>,
    content: Option<&str>,
    language_override: Option<&str>,
    workspace_root: Option<&PathBuf>,
    cache: &mut ParseCache,
    handle: Option<&str>,
    files: Option<&Vec<String>>,
    subcommand: Option<&str>,
    query: Option<&str>,
    kind: Option<&str>,
    root: Option<&str>,
    max_results: Option<usize>,
) -> String {
    // Commands that don't need file/content parsing first.
    match command {
        "status" => {
            return commands::status_cmd(cache, id).to_json();
        }
        "clear-cache" => {
            return commands::clear_cache_cmd(cache, id).to_json();
        }
        "search-symbols" => {
            return commands::search_symbols_cmd(cache, query.unwrap_or(""), kind, max_results, id).to_json();
        }
        "repo-map" => {
            let root_path = root.map(PathBuf::from);
            let ws_root = workspace_root.map(|p| p.as_path());
            return commands::repo_map_cmd(root_path.as_deref(), ws_root, id).to_json();
        }
        "batch" => {
            let file_list: Vec<String> = files.cloned().unwrap_or_default();
            let sub = subcommand.unwrap_or("outline");
            let ws_root = workspace_root.map(|p| p.as_path());
            return commands::batch_cmd(file_list, sub.to_string(), id, ws_root).to_json();
        }
        "warm-cache" => {
            let file_list: Vec<String> = files.cloned().unwrap_or_default();
            let ws_root = workspace_root.map(|p| p.as_path());
            return commands::warm_cache_cmd(cache, file_list, id, ws_root).to_json();
        }
        "reparse" => {
            let ws_root = workspace_root.map(|p| p.as_path());
            return commands::reparse_cmd(cache, file, content, language_override, ws_root, id).to_json();
        }
        _ => {}
    }

    // For file-based commands, try to use the cache.
    let file_path = file.map(PathBuf::from);
    let file_path_ref = file_path.as_deref();
    let workspace_root_ref = workspace_root.map(|p| p.as_path());

    // Determine the canonical cache key if a file path was given.
    let cache_key: Option<PathBuf> = file_path_ref.and_then(|p| p.canonicalize().ok());

    // Check cache first for file-based requests (skip for content-based).
    let parsed_owned: Option<crate::parser::ParsedSource> = if let Some(ref key) = cache_key {
        if let Some(cached) = cache.get(key) {
            // We have it cached; but we need to pass a reference.
            // Since Tree is not Clone, we re-parse for now when the cache
            // holds the tree.  For the common case the command handlers
            // receive a reference and we drop the data afterwards.
            //
            // Strategy: re-parse from the cached source so we get a fresh Tree.
            // (tree_sitter::Tree cannot be cloned.)
            let mut parser = cached.language.parser();
            let tree = parser.parse(&cached.source, None);
            tree.map(|t| crate::parser::ParsedSource {
                source: cached.source.clone(),
                language: cached.language,
                tree: t,
            })
        } else {
            None
        }
    } else {
        None
    };

    // If not cached (or content-based), parse from scratch.
    let parsed = if let Some(ps) = parsed_owned {
        ps
    } else {
        match parser::parse_source(
            file_path_ref,
            content,
            language_override,
            workspace_root_ref,
        ) {
            Ok(p) => p,
            Err(e) => return e.to_json_response(id.as_ref()),
        }
    };

    // Insert into cache if it was a file parse.
    if let Some(ref key) = cache_key {
        // We need to re-parse to store the tree (since we just consumed it above).
        // Actually, the tree was just created — let's store a re-parsed version.
        let mut store_parser = parsed.language.parser();
        if let Some(store_tree) = store_parser.parse(&parsed.source, None) {
            let stored = crate::parser::ParsedSource {
                source: parsed.source.clone(),
                language: parsed.language,
                tree: store_tree,
            };
            cache.insert(key.clone(), stored);
        }
    }

    // expand-symbol needs special handling.
    if command == "expand-symbol" {
        return match handle {
            Some(h) => commands::expand_symbol_cmd(&parsed, h, id).to_json(),
            None => {
                let err = AnalyzerError::invalid_command("expand-symbol requires 'handle' field");
                err.to_json_response(id.as_ref())
            }
        };
    }

    match commands::dispatch(&parsed, command, id.clone()) {
        Ok(output) => output.to_json(),
        Err(e) => e.to_json_response(id.as_ref()),
    }
}

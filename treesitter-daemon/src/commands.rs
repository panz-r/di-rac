use crate::error::AnalyzerError;
use crate::extractor::{self, Import, Symbol};
use crate::parser::ParsedSource;
use crate::skeleton;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CommandOutput {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbols: Option<Vec<Symbol>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imports: Option<Vec<Import>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skeleton: Option<String>,
}

impl CommandOutput {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            r#"{"ok":false,"error":{"code":"INTERNAL_ERROR","message":"Serialization failed"}}"#.to_string()
        })
    }
}

fn make_output(
    id: Option<serde_json::Value>,
    symbols: Option<Vec<Symbol>>,
    imports: Option<Vec<Import>>,
    skeleton: Option<String>,
) -> CommandOutput {
    CommandOutput { ok: true, id, symbols, imports, skeleton }
}

pub fn outline(parsed: &ParsedSource, id: Option<serde_json::Value>) -> CommandOutput {
    let symbols = extractor::extract_symbols(&parsed.source, &parsed.tree, parsed.language);
    let imports = extractor::extract_imports(&parsed.source, &parsed.tree, parsed.language);
    make_output(id, Some(symbols), Some(imports), None)
}

pub fn symbols(parsed: &ParsedSource, id: Option<serde_json::Value>) -> CommandOutput {
    let symbols = extractor::extract_symbols(&parsed.source, &parsed.tree, parsed.language);
    let imports = extractor::extract_imports(&parsed.source, &parsed.tree, parsed.language);
    make_output(id, Some(symbols), Some(imports), None)
}

pub fn handles(parsed: &ParsedSource, id: Option<serde_json::Value>) -> CommandOutput {
    let symbols = extractor::extract_symbols(&parsed.source, &parsed.tree, parsed.language);
    make_output(id, Some(symbols), None, None)
}

pub fn skeleton_cmd(parsed: &ParsedSource, id: Option<serde_json::Value>) -> CommandOutput {
    let symbols = extractor::extract_symbols(&parsed.source, &parsed.tree, parsed.language);
    let skel = skeleton::generate_skeleton(&parsed.source, &parsed.tree, parsed.language);
    make_output(id, Some(symbols), None, Some(skel))
}

pub fn dispatch(
    parsed: ParsedSource,
    command: &str,
    id: Option<serde_json::Value>,
) -> Result<CommandOutput, AnalyzerError> {
    match command {
        "outline" => Ok(outline(&parsed, id)),
        "symbols" => Ok(symbols(&parsed, id)),
        "handles" => Ok(handles(&parsed, id)),
        "skeleton" => Ok(skeleton_cmd(&parsed, id)),
        other => Err(AnalyzerError::invalid_command(other)),
    }
}

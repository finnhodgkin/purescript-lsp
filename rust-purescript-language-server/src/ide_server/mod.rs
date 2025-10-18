pub mod commands;
pub mod process;
pub mod protocol;

use serde::{Deserialize, Serialize};

/// IDE server command types
#[derive(Debug, Serialize)]
pub struct IdeCommand {
    pub command: String,
    pub params: Option<serde_json::Value>,
}

/// IDE server response types
#[derive(Debug, Deserialize)]
pub struct IdeResponse {
    pub result: Option<serde_json::Value>,
    pub error: Option<IdeError>,
}

#[derive(Debug, Deserialize)]
pub struct IdeError {
    pub code: i32,
    pub message: String,
}

/// Rebuild result from purs ide server
#[derive(Debug, Deserialize)]
pub struct RebuildResult {
    pub result: String,
    pub errors: Option<Vec<RebuildError>>,
    pub warnings: Option<Vec<RebuildError>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RebuildError {
    #[serde(rename = "allSpans")]
    pub all_spans: Option<Vec<ErrorSpan>>,
    #[serde(rename = "errorCode")]
    pub error_code: String,
    #[serde(rename = "errorLink")]
    pub error_link: Option<String>,
    #[serde(default = "default_string")]
    pub filename: String,
    pub message: String,
    #[serde(rename = "moduleName")]
    pub module_name: Option<String>,
    pub position: ErrorPosition,
    pub suggestion: Option<ErrorSuggestion>,
}

fn default_string() -> String {
    "unknown".to_string()
}

#[derive(Debug, Deserialize, Clone)]
pub struct ErrorPosition {
    #[serde(rename = "startLine")]
    pub start_line: u32,
    #[serde(rename = "endLine")]
    pub end_line: u32,
    #[serde(rename = "startColumn")]
    pub start_column: u32,
    #[serde(rename = "endColumn")]
    pub end_column: u32,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ErrorSpan {
    pub end: [u32; 2],
    pub name: String,
    pub start: [u32; 2],
}

#[derive(Debug, Deserialize, Clone)]
pub struct ErrorSuggestion {
    pub replacement: String,
    pub replace_range: Option<ErrorPosition>,
}

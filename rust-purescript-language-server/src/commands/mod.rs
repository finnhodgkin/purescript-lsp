pub mod build;

use crate::types::ServerState;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp::Client;

/// Execute a command by name
pub async fn execute_command(
    command: &str,
    client: &Client,
    state: &Arc<Mutex<ServerState>>,
    _args: Option<Vec<serde_json::Value>>,
) -> Result<(), String> {
    match command {
        "purescript.build" => build::execute(client, state, false).await,
        "purescript.buildQuick" => build::execute(client, state, true).await,
        _ => Err(format!("Unknown command: {}", command)),
    }
}

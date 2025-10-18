use crate::ide_server::{IdeCommand, IdeResponse, RebuildResult};
use anyhow::Result;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Send a command to the IDE server via TCP
pub async fn send_command(port: u16, command: IdeCommand) -> Result<IdeResponse> {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).await?;

    // Create IDE server request (not JSON-RPC)
    let request = serde_json::json!({
        "command": command.command,
        "params": command.params
    });

    let request_json = serde_json::to_string(&request)?;
    let request_bytes = format!("{}\n", request_json);

    // Send request
    stream.write_all(request_bytes.as_bytes()).await?;

    // Read response
    let mut buffer = Vec::new();
    stream.read_to_end(&mut buffer).await?;
    let response_str = String::from_utf8(buffer)?;

    // Parse IDE server response
    let response: serde_json::Value = serde_json::from_str(&response_str)?;

    // Always return the result, whether it contains errors or not
    // The IDE server uses resultType: "error" to indicate compilation errors,
    // but the errors are still in the result field
    Ok(IdeResponse {
        result: response.get("result").cloned(),
        error: None,
    })
}

/// Rebuild a single file
pub async fn rebuild_file(port: u16, file_path: &str) -> Result<RebuildResult> {
    let command = IdeCommand {
        command: "rebuild".to_string(),
        params: Some(json!({
            "file": file_path
        })),
    };

    let response = send_command(port, command).await?;

    if let Some(result) = response.result {
        // The IDE server returns errors directly in the result array
        if let Some(errors) = result.as_array() {
            if !errors.is_empty() {
                let rebuild_errors: Vec<crate::ide_server::RebuildError> =
                    serde_json::from_value(result)?;
                return Ok(RebuildResult {
                    result: "rebuild completed".to_string(),
                    errors: Some(rebuild_errors),
                    warnings: None,
                });
            }
        }
    }

    // If no errors, return success
    Ok(RebuildResult {
        result: "rebuild completed".to_string(),
        errors: None,
        warnings: None,
    })
}

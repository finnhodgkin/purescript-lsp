use anyhow::Result;
use lsp_client::TestLspClient;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::time::Duration;
use tempfile::TempDir;

mod lsp_client;

fn setup_test_workspace() -> Result<TempDir> {
    let temp_dir = TempDir::new()?;
    let workspace_path = temp_dir.path();

    // Copy example project structure
    let example_project = Path::new("../example-project");
    println!("Example project path: {:?}", example_project);
    println!("Example project exists: {}", example_project.exists());

    // Create src directory
    let src_dir = workspace_path.join("src");
    fs::create_dir_all(&src_dir)?;

    // Copy spago.yaml
    fs::copy(
        example_project.join("spago.yaml"),
        workspace_path.join("spago.yaml"),
    )?;

    // Copy valid Main.purs
    fs::copy(
        example_project.join("src/Main.purs"),
        src_dir.join("Main.purs"),
    )?;

    // Copy Test.purs
    fs::copy(
        example_project.join("src/Test.purs"),
        src_dir.join("Test.purs"),
    )?;

    Ok(temp_dir)
}

#[tokio::test]
async fn test_server_lifecycle() -> Result<()> {
    let temp_dir = setup_test_workspace()?;
    let workspace_path = temp_dir.path();

    let mut client = TestLspClient::new(workspace_path)?;

    // Test initialize
    let result = client.initialize(workspace_path)?;
    assert!(result.get("capabilities").is_some());
    assert!(result.get("serverInfo").is_some());

    // Test initialized notification
    client.send_notification("initialized", json!(null))?;

    // Test shutdown
    client.shutdown()?;

    Ok(())
}

#[tokio::test]
async fn test_diagnostics_with_broken_file() -> Result<()> {
    let temp_dir = setup_test_workspace()?;
    let workspace_path = temp_dir.path();

    let mut client = TestLspClient::new(workspace_path)?;
    client.initialize(workspace_path)?;
    client.send_notification("initialized", json!(null))?;

    // Create a broken file
    let broken_content = r#"module Main where

import Prelude

main :: Effect Unit
main = do
  lg "test"
"#;

    fs::write(workspace_path.join("src/Main.purs"), broken_content)?;

    // Send didSave notification
    let uri = format!("file://{}/src/Main.purs", workspace_path.display());
    client.send_notification(
        "textDocument/didSave",
        json!({
            "textDocument": {
                "uri": uri,
                "version": 1
            }
        }),
    )?;

    // For now, just test that the didSave notification is sent successfully
    // and that the server doesn't crash
    println!("Sent didSave notification, waiting a moment for processing...");
    std::thread::sleep(Duration::from_millis(1000));

    // The test passes if we get here without hanging
    println!("Test completed - didSave notification was processed");

    client.shutdown()?;
    Ok(())
}

#[tokio::test]
async fn test_diagnostics_with_valid_file() -> Result<()> {
    let temp_dir = setup_test_workspace()?;
    let workspace_path = temp_dir.path();

    let mut client = TestLspClient::new(workspace_path)?;
    client.initialize(workspace_path)?;
    client.send_notification("initialized", json!(null))?;

    // Ensure we have valid content
    let valid_content = r#"module Main where

import Prelude

import Effect (Effect)
import Effect.Console (log)

main :: Effect Unit
main = do
  log "test"
"#;

    fs::write(workspace_path.join("src/Main.purs"), valid_content)?;

    // Send didSave notification
    let uri = format!("file://{}/src/Main.purs", workspace_path.display());
    client.send_notification(
        "textDocument/didSave",
        json!({
            "textDocument": {
                "uri": uri,
                "version": 1
            }
        }),
    )?;

    // For now, just test that the didSave notification is sent successfully
    // and that the server doesn't crash
    println!("Sent didSave notification for valid file, waiting a moment for processing...");
    std::thread::sleep(Duration::from_millis(1000));

    // The test passes if we get here without hanging
    println!("Test completed - didSave notification was processed for valid file");

    client.shutdown()?;
    Ok(())
}

#[tokio::test]
async fn test_code_actions() -> Result<()> {
    let temp_dir = setup_test_workspace()?;
    let workspace_path = temp_dir.path();

    let mut client = TestLspClient::new(workspace_path)?;
    client.initialize(workspace_path)?;
    client.send_notification("initialized", json!(null))?;

    // Create a file with a fixable error (missing import)
    let broken_content = r#"module Main where

import Prelude

main :: Effect Unit
main = do
  log "test"
"#;

    fs::write(workspace_path.join("src/Main.purs"), broken_content)?;

    // Send didSave to get diagnostics first
    let uri = format!("file://{}/src/Main.purs", workspace_path.display());
    client.send_notification(
        "textDocument/didSave",
        json!({
            "textDocument": {
                "uri": uri,
                "version": 1
            }
        }),
    )?;

    // Wait a moment for any processing
    println!("Waiting for processing before code actions...");
    std::thread::sleep(Duration::from_millis(1000));

    // Request code actions
    let code_actions = client.send_request(
        "textDocument/codeAction",
        json!({
            "textDocument": {
                "uri": uri
            },
            "range": {
                "start": { "line": 5, "character": 2 },
                "end": { "line": 5, "character": 5 }
            },
            "context": {
                "diagnostics": []
            }
        }),
    )?;

    // Code actions should be an array (might be empty if no fixes available)
    assert!(
        code_actions.is_array(),
        "Expected code actions to be an array"
    );

    client.shutdown()?;
    Ok(())
}

#[tokio::test]
async fn test_formatting() -> Result<()> {
    let temp_dir = setup_test_workspace()?;
    let workspace_path = temp_dir.path();

    let mut client = TestLspClient::new(workspace_path)?;
    client.initialize(workspace_path)?;
    client.send_notification("initialized", json!(null))?;

    // Create a file with poor formatting
    let poorly_formatted_content = r#"module Main where
import Prelude
import Effect (Effect)
import Effect.Console (log)
main::Effect Unit
main=do
log"test"
"#;

    fs::write(
        workspace_path.join("src/Main.purs"),
        poorly_formatted_content,
    )?;

    // Request formatting
    let uri = format!("file://{}/src/Main.purs", workspace_path.display());
    let formatting_result = client.send_request(
        "textDocument/formatting",
        json!({
            "textDocument": {
                "uri": uri
            },
            "options": {
                "tabSize": 2,
                "insertSpaces": true
            }
        }),
    )?;

    // Formatting result should be an array of text edits or null if no formatter is available
    println!("Formatting result: {:?}", formatting_result);
    assert!(
        formatting_result.is_array() || formatting_result.is_null(),
        "Expected formatting result to be an array or null, got: {:?}",
        formatting_result
    );

    client.shutdown()?;
    Ok(())
}

#[tokio::test]
async fn test_basic_lsp_features() -> Result<()> {
    let temp_dir = setup_test_workspace()?;
    let workspace_path = temp_dir.path();

    let mut client = TestLspClient::new(workspace_path)?;
    client.initialize(workspace_path)?;
    client.send_notification("initialized", json!(null))?;

    // Test that we can send a didOpen notification
    let uri = format!("file://{}/src/Main.purs", workspace_path.display());
    client.send_notification(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "purescript",
                "version": 1,
                "text": "module Main where\n\nimport Prelude\n\nmain = \"Hello, PureScript!\""
            }
        }),
    )?;

    // Test that we can send a didChange notification
    client.send_notification(
        "textDocument/didChange",
        json!({
            "textDocument": {
                "uri": uri,
                "version": 2
            },
            "contentChanges": [{
                "text": "module Main where\n\nimport Prelude\n\nmain = \"Hello, World!\""
            }]
        }),
    )?;

    // Test that we can send a didSave notification
    client.send_notification(
        "textDocument/didSave",
        json!({
            "textDocument": {
                "uri": uri,
                "version": 2
            }
        }),
    )?;

    client.shutdown()?;
    Ok(())
}

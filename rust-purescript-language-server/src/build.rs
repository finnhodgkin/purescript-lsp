use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tower_lsp::lsp_types::Url;

use crate::ide_server::RebuildError;

/// Result of a build operation
#[derive(Debug, Clone)]
pub struct BuildResult {
    pub success: bool,
    pub output: String,
    pub errors: HashMap<String, Vec<RebuildError>>, // file path -> errors
    pub warnings: HashMap<String, Vec<RebuildError>>, // file path -> warnings
}

/// JSON structure for PureScript compiler errors
#[derive(Debug, Deserialize)]
struct CompilerOutput {
    errors: Option<Vec<RebuildError>>,
    warnings: Option<Vec<RebuildError>>,
}

/// Run a full ragu build with streaming progress
/// Returns (progress_receiver, result_receiver) immediately so progress can be monitored
pub fn run_build(
    working_dir: String,
) -> (
    tokio::sync::mpsc::Receiver<(String, u32, u32, String)>,
    tokio::sync::oneshot::Receiver<Result<BuildResult>>,
) {
    run_ragu_build_streaming(
        working_dir,
        vec![
            "build".to_string(),
            "--".to_string(),
            "--json-errors".to_string(),
        ],
    )
}

/// Run a quick ragu build with streaming progress
/// Returns (progress_receiver, result_receiver) immediately so progress can be monitored
pub fn run_quick_build(
    working_dir: String,
) -> (
    tokio::sync::mpsc::Receiver<(String, u32, u32, String)>,
    tokio::sync::oneshot::Receiver<Result<BuildResult>>,
) {
    run_ragu_build_streaming(
        working_dir,
        vec![
            "build".to_string(),
            "-q".to_string(),
            "--".to_string(),
            "--json-errors".to_string(),
        ],
    )
}

/// Internal function to run ragu build with streaming progress
/// Returns (progress_receiver, result_receiver) immediately and spawns build in background
fn run_ragu_build_streaming(
    working_dir: String,
    args: Vec<String>,
) -> (
    tokio::sync::mpsc::Receiver<(String, u32, u32, String)>,
    tokio::sync::oneshot::Receiver<Result<BuildResult>>,
) {
    // Create channels
    let (progress_tx, progress_rx) = tokio::sync::mpsc::channel(100);
    let (result_tx, result_rx) = tokio::sync::oneshot::channel();

    // Spawn build process in background using async I/O
    tokio::spawn(async move {
        let result: Result<BuildResult> = async {
            // Create command
            let mut cmd = Command::new("ragu");
            cmd.args(&args);
            cmd.current_dir(&working_dir);
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            // Spawn the child process
            let mut child = cmd
                .spawn()
                .map_err(|e| anyhow::anyhow!("Failed to spawn ragu command: {}", e))?;

            // Get stdout and stderr handles
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| anyhow::anyhow!("Failed to capture stdout"))?;
            let stderr = child
                .stderr
                .take()
                .ok_or_else(|| anyhow::anyhow!("Failed to capture stderr"))?;

            // Read both stdout and stderr concurrently to avoid buffering issues
            // and to capture progress from whichever stream ragu writes to
            let progress_tx_clone = progress_tx.clone();

            // Spawn task to read stdout
            let stdout_handle = tokio::spawn(async move {
                let mut lines = Vec::new();
                let mut reader = BufReader::new(stdout).lines();

                while let Ok(Some(line)) = reader.next_line().await {
                    lines.push(line.clone());

                    // Parse progress and send immediately
                    if let Some((current, total, module_name)) = parse_single_progress_line(&line) {
                        let percentage = (current as f64 / total as f64 * 100.0) as u32;
                        let _ = progress_tx_clone
                            .send((
                                format!("[{}/{}] {}", current, total, module_name),
                                percentage,
                                current,
                                module_name,
                            ))
                            .await;
                    }
                }

                lines
            });

            // Spawn task to read stderr
            let stderr_handle = tokio::spawn(async move {
                let mut lines = Vec::new();
                let mut reader = BufReader::new(stderr).lines();

                while let Ok(Some(line)) = reader.next_line().await {
                    lines.push(line.clone());

                    // Also check stderr for progress (build tools often write there)
                    if let Some((current, total, module_name)) = parse_single_progress_line(&line) {
                        let percentage = (current as f64 / total as f64 * 100.0) as u32;
                        let _ = progress_tx
                            .send((
                                format!("[{}/{}] {}", current, total, module_name),
                                percentage,
                                current,
                                module_name,
                            ))
                            .await;
                    }
                }

                lines
            });

            // Wait for both streams to be fully read
            let stdout_lines = stdout_handle
                .await
                .map_err(|e| anyhow::anyhow!("Failed to join stdout task: {}", e))?;
            let stderr_lines = stderr_handle
                .await
                .map_err(|e| anyhow::anyhow!("Failed to join stderr task: {}", e))?;

            // Wait for process to complete
            let exit_status = child
                .wait()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to wait for child: {}", e))?;

            // Combine all output
            let stdout_output = stdout_lines.join("\n");
            let _stderr_output = stderr_lines.join("\n");

            // Parse JSON errors from stdout (ragu outputs JSON errors to stdout with --json-errors flag)
            let (errors, warnings) = parse_build_output(&stdout_output)?;

            Ok(BuildResult {
                success: exit_status.success(),
                output: stdout_output,
                errors,
                warnings,
            })
        }
        .await;

        let _ = result_tx.send(result);
    });

    (progress_rx, result_rx)
}

/// Parse PureScript compiler output for errors and warnings
fn parse_build_output(
    stderr_output: &str,
) -> Result<(
    HashMap<String, Vec<RebuildError>>,
    HashMap<String, Vec<RebuildError>>,
)> {
    let mut errors = HashMap::new();
    let mut warnings = HashMap::new();

    // Look for JSON error output in stderr (where ragu outputs JSON)
    for line in stderr_output.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('{')
            && (trimmed.contains("\"errors\"") || trimmed.contains("\"warnings\""))
        {
            // Try to parse the JSON line
            match serde_json::from_str::<CompilerOutput>(trimmed) {
                Ok(compiler_output) => {
                    // Process errors
                    if let Some(compiler_errors) = compiler_output.errors {
                        for error in compiler_errors {
                            let file_path = error.filename.clone();
                            errors.entry(file_path).or_insert_with(Vec::new).push(error);
                        }
                    }

                    // Process warnings
                    if let Some(compiler_warnings) = compiler_output.warnings {
                        for warning in compiler_warnings {
                            let file_path = warning.filename.clone();
                            warnings
                                .entry(file_path)
                                .or_insert_with(Vec::new)
                                .push(warning);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to parse JSON error output: {}", e);
                    eprintln!("JSON line was: {}", trimmed);
                }
            }
        }
    }

    Ok((errors, warnings))
}

/// Parse a single progress line
/// Returns (current, total, module_name) for lines like "[2 of 5] Compiling/Skipping Module.Name"
fn parse_single_progress_line(line: &str) -> Option<(u32, u32, String)> {
    let trimmed = line.trim();
    // Match pattern: [2 of 5] (Compiling|Skipping) Module.Name
    if let Some(captures) = regex::Regex::new(r"\[(\d+) of (\d+)\] (?:Compiling|Skipping) (.+)")
        .unwrap()
        .captures(trimmed)
    {
        if let (Ok(current), Ok(total), Some(module_name)) = (
            captures[1].parse::<u32>(),
            captures[2].parse::<u32>(),
            captures.get(3).map(|m| m.as_str().to_string()),
        ) {
            return Some((current, total, module_name));
        }
    }
    None
}

/// Convert file path to URI for diagnostics
pub fn file_path_to_uri(file_path: &str, workspace_root: &str) -> Option<Url> {
    let full_path = if Path::new(file_path).is_absolute() {
        file_path.to_string()
    } else {
        Path::new(workspace_root)
            .join(file_path)
            .to_string_lossy()
            .to_string()
    };

    match Url::from_file_path(&full_path) {
        Ok(uri) => {
            eprintln!("Converted file path '{}' to URI: {}", file_path, uri);
            Some(uri)
        }
        Err(_) => {
            eprintln!(
                "Failed to convert file path to URI: {} (full path: {})",
                file_path, full_path
            );
            None
        }
    }
}

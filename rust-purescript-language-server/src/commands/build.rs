use crate::build;
use crate::diagnostics;
use crate::types::ServerState;
use lsp_types::{
    MessageType, NumberOrString, ProgressParams, ProgressParamsValue, WorkDoneProgress,
    WorkDoneProgressBegin, WorkDoneProgressCreateParams, WorkDoneProgressEnd,
    WorkDoneProgressReport, notification::Progress, request::WorkDoneProgressCreate,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp::Client;

/// Execute a build command with progress reporting and streaming output
pub async fn execute(
    client: &Client,
    state: &Arc<Mutex<ServerState>>,
    quick: bool,
) -> Result<(), String> {
    // Get workspace root
    let workspace_root = {
        let state = state.lock().await;
        state.workspace_root.clone()
    };

    let Some(workspace_root) = workspace_root else {
        client
            .log_message(
                MessageType::ERROR,
                "No workspace root available for build command",
            )
            .await;
        return Err("No workspace root available".to_string());
    };

    // Create unique token for progress
    let token = NumberOrString::String(format!(
        "build-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));

    // Request client to create progress indicator
    if let Err(e) = client
        .send_request::<WorkDoneProgressCreate>(WorkDoneProgressCreateParams {
            token: token.clone(),
        })
        .await
    {
        client
            .log_message(
                MessageType::ERROR,
                format!("Failed to create progress token: {}", e),
            )
            .await;
        return Err(format!("Failed to create progress token: {}", e));
    }

    let build_type = if quick { "Quick Build" } else { "Full Build" };

    // Send begin notification
    client
        .send_notification::<Progress>(ProgressParams {
            token: token.clone(),
            value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(WorkDoneProgressBegin {
                title: "".into(),
                message: Some(format!("Starting {}...", build_type)),
                cancellable: Some(false),
                percentage: None,
            })),
        })
        .await;

    // Spawn async build task
    let client = client.clone();
    let state = state.clone();
    let token_clone = token.clone();

    tokio::spawn(async move {
        // Start build and get receivers immediately
        let (mut progress_rx, result_rx) = if quick {
            build::run_quick_build(workspace_root.clone())
        } else {
            build::run_build(workspace_root.clone())
        };

        // Handle progress updates in real-time
        let client_progress = client.clone();
        let token_progress = token_clone.clone();

        tokio::spawn(async move {
            while let Some((message, percentage, _current, _module_name)) = progress_rx.recv().await
            {
                client_progress
                    .send_notification::<Progress>(ProgressParams {
                        token: token_progress.clone(),
                        value: ProgressParamsValue::WorkDone(WorkDoneProgress::Report(
                            WorkDoneProgressReport {
                                message: Some(message),
                                cancellable: Some(false),
                                percentage: Some(percentage),
                            },
                        )),
                    })
                    .await;
            }
        });

        // Wait for build to complete
        let build_result = match result_rx.await {
            Ok(result) => result,
            Err(e) => {
                client
                    .log_message(
                        MessageType::ERROR,
                        format!("Build task was cancelled: {}", e),
                    )
                    .await;
                return;
            }
        };

        match build_result {
            Ok(build_result) => {
                // Log build summary
                client
                    .log_message(
                        MessageType::INFO,
                        format!(
                            "Build completed. Files with errors: {}, Files with warnings: {}",
                            build_result.errors.len(),
                            build_result.warnings.len()
                        ),
                    )
                    .await;

                // Update progress with completion message
                client
                    .send_notification::<Progress>(ProgressParams {
                        token: token_clone.clone(),
                        value: ProgressParamsValue::WorkDone(WorkDoneProgress::Report(
                            WorkDoneProgressReport {
                                message: Some(if build_result.success {
                                    "Build completed successfully".to_string()
                                } else {
                                    "Build completed with errors".to_string()
                                }),
                                cancellable: Some(false),
                                percentage: Some(100),
                            },
                        )),
                    })
                    .await;

                // Clear diagnostics for all .purs files from previous build
                // This is important for quick builds that don't touch all files
                {
                    let state = state.lock().await;
                    for uri in state.last_build_errors.keys() {
                        // Only clear .purs files
                        if uri.path().ends_with(".purs") {
                            client.publish_diagnostics(uri.clone(), vec![], None).await;
                        }
                    }
                }

                // Clear previous build errors
                {
                    let mut state = state.lock().await;
                    state.last_build_errors.clear();
                }

                // Publish diagnostics for all files with errors/warnings
                let mut all_uris = std::collections::HashSet::new();

                // Process errors
                for (file_path, errors) in &build_result.errors {
                    if let Some(uri) = build::file_path_to_uri(file_path, &workspace_root) {
                        all_uris.insert(uri.clone());

                        // Store errors in state
                        {
                            let mut state = state.lock().await;
                            state.last_build_errors.insert(uri.clone(), errors.clone());
                        }

                        // Publish diagnostics
                        let diagnostics = diagnostics::convert_rebuild_errors(errors, &uri);
                        client.publish_diagnostics(uri, diagnostics, None).await;
                    }
                }

                // Process warnings (only for local files, not deps)
                for (file_path, warnings) in &build_result.warnings {
                    // Only show warnings for files in the workspace, not deps
                    if !file_path.contains(".spago") {
                        if let Some(uri) = build::file_path_to_uri(file_path, &workspace_root) {
                            all_uris.insert(uri.clone());

                            // Store warnings in state
                            {
                                let mut state = state.lock().await;
                                let existing = state
                                    .last_build_errors
                                    .entry(uri.clone())
                                    .or_insert_with(Vec::new);
                                existing.extend(warnings.clone());
                            }

                            // Publish diagnostics
                            let diagnostics = diagnostics::convert_rebuild_errors(warnings, &uri);
                            client.publish_diagnostics(uri, diagnostics, None).await;
                        }
                    }
                }
            }
            Err(e) => {
                client
                    .log_message(MessageType::ERROR, format!("Build failed: {}", e))
                    .await;
            }
        }

        // Send end notification
        client
            .send_notification::<Progress>(ProgressParams {
                token: token_clone,
                value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(WorkDoneProgressEnd {
                    message: None,
                })),
            })
            .await;
    });

    Ok(())
}

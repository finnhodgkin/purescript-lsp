use crate::code_actions;
use crate::commands;
use crate::config;
use crate::diagnostics;
use crate::formatting;
use crate::ide_server::{commands as ide_commands, process};
use crate::types::ServerState;
use lsp_types::{
    ProgressParams, ProgressParamsValue, WorkDoneProgress, WorkDoneProgressBegin,
    WorkDoneProgressCreateParams, WorkDoneProgressEnd, notification::Progress,
    request::WorkDoneProgressCreate,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

pub struct Backend {
    client: Client,
    state: Arc<Mutex<ServerState>>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        let state = Arc::new(Mutex::new(ServerState::default()));

        Self { client, state }
    }

    /// Initialize the server with configuration from client and ragu
    async fn initialize_server(&self, workspace_root: &str) -> anyhow::Result<()> {
        self.client
            .log_message(
                MessageType::INFO,
                format!("Initializing workspace: {}", workspace_root),
            )
            .await;

        // Initialize configuration from client and ragu
        let config = config::init_from_client_and_ragu(&self.client, workspace_root).await?;

        // Log the configuration
        config::log_config(&self.client, &config).await;

        // Start the IDE server
        let (process, port) = process::start_ide_server_async(
            workspace_root,
            &config.output_dir,
            &config.source_globs,
        )
        .await?;

        // Update state
        let mut state = self.state.lock().await;
        state.config = Some(config);
        state.workspace_root = Some(workspace_root.to_string());
        state.ide_server.port = Some(port);
        state.ide_server.process = Some(process);
        state.ide_server.working_dir = Some(workspace_root.to_string());

        self.client
            .log_message(MessageType::INFO, format!("Purescript IDE port {}", port))
            .await;

        Ok(())
    }

    /// Restart the IDE server (used when configuration changes)
    async fn restart_server(&self) -> anyhow::Result<()> {
        let workspace_root = {
            let state = self.state.lock().await;
            state.workspace_root.clone()
        };

        if let Some(root) = workspace_root {
            self.client
                .log_message(
                    MessageType::INFO,
                    "Configuration changed, restarting IDE server...".to_string(),
                )
                .await;

            // Stop the current IDE server
            let mut process = {
                let mut state = self.state.lock().await;
                state.ide_server.process.take()
            };

            if let Some(ref mut child) = process {
                let _ = child.kill();
            }

            // Reinitialize with new config
            self.initialize_server(&root).await?;
        }

        Ok(())
    }

    /// Trigger fast rebuild for a file
    /// If content is provided, it will use the data: prefix format for in-memory rebuild
    async fn trigger_fast_rebuild(
        &self,
        port: u16,
        file_path: &str,
        uri: &Url,
        content: Option<String>,
    ) {
        // Extract filename for display
        let file_name = std::path::Path::new(file_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file");

        // End any previous active progress to prevent stuck indicators
        {
            let mut state = self.state.lock().await;
            if let Some(previous_token) = state.active_rebuild_token.take() {
                self.client
                    .send_notification::<Progress>(ProgressParams {
                        token: previous_token,
                        value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(
                            WorkDoneProgressEnd { message: None },
                        )),
                    })
                    .await;
            }
        }

        // Create unique token for progress
        let token = NumberOrString::String(format!(
            "rebuild-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));

        // Request client to create progress indicator
        if let Err(e) = self
            .client
            .send_request::<WorkDoneProgressCreate>(WorkDoneProgressCreateParams {
                token: token.clone(),
            })
            .await
        {
            self.client
                .log_message(
                    MessageType::ERROR,
                    format!("Failed to create progress token: {}", e),
                )
                .await;
            // Return early - don't try to use an invalid token
            return;
        }

        // Store the active token
        {
            let mut state = self.state.lock().await;
            state.active_rebuild_token = Some(token.clone());
        }

        // Send begin notification
        self.client
            .send_notification::<Progress>(ProgressParams {
                token: token.clone(),
                value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(
                    WorkDoneProgressBegin {
                        title: "".into(),
                        message: Some(file_name.into()),
                        cancellable: Some(false),
                        percentage: None,
                    },
                )),
            })
            .await;

        let result =
            ide_commands::rebuild_file_with_content(port, file_path, content.as_deref()).await;

        // Clear the active token and send end notification
        {
            let mut state = self.state.lock().await;
            state.active_rebuild_token = None;
        }

        self.client
            .send_notification::<Progress>(ProgressParams {
                token,
                value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(WorkDoneProgressEnd {
                    message: None,
                })),
            })
            .await;

        match result {
            Ok(rebuild_result) => {
                // Convert rebuild errors to diagnostics
                if let Some(errors) = rebuild_result.errors {
                    let diagnostics = diagnostics::convert_rebuild_errors(&errors, uri);

                    // Store errors in state for code actions
                    {
                        let mut state = self.state.lock().await;
                        state.document_errors.insert(uri.clone(), errors.clone());
                    }

                    if !diagnostics.is_empty() {
                        self.client
                            .publish_diagnostics(uri.clone(), diagnostics, None)
                            .await;
                    }
                } else {
                    // Clear diagnostics and errors for this file since there are no errors
                    {
                        let mut state = self.state.lock().await;
                        state.document_errors.remove(uri);
                    }
                    self.client
                        .publish_diagnostics(uri.clone(), vec![], None)
                        .await;
                }
            }
            Err(e) => {
                self.client
                    .log_message(MessageType::ERROR, format!("Fast rebuild failed: {}", e))
                    .await;
            }
        }
    }

    /// Handle document focus event - triggers rebuild when fast_rebuild_on_change is enabled
    pub async fn handle_document_focus(&self, uri: &Url) {
        // Get the document content and check if fast rebuild is enabled
        let (fast_rebuild_enabled, port, content) = {
            let state = self.state.lock().await;
            (
                state.fast_rebuild_on_change(),
                state.ide_server.port,
                state.document_contents.get(uri).cloned(),
            )
        };

        if fast_rebuild_enabled {
            if let Some(port) = port {
                if let Some(content) = content {
                    if let Ok(file_path) = uri.to_file_path() {
                        if let Some(file_path_str) = file_path.to_str() {
                            // Skip rebuild if file contains foreign imports
                            // (fast rebuild from content doesn't work with foreign modules)
                            if !content.contains("foreign import") {
                                // Pass the content for data: prefix rebuild
                                self.trigger_fast_rebuild(port, file_path_str, uri, Some(content))
                                    .await;
                            }
                        }
                    }
                }
            }
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
        // Store workspace root but don't initialize yet - wait for initialized notification
        if let Some(workspace_root) = params.root_uri.and_then(|uri| uri.to_file_path().ok()) {
            if let Some(root_str) = workspace_root.to_str() {
                let mut state = self.state.lock().await;
                state.workspace_root = Some(root_str.to_string());
            }
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                document_formatting_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![
                        "purescript.build".to_string(),
                        "purescript.buildQuick".to_string(),
                        "purescript.focusDocument".to_string(),
                    ],
                    ..Default::default()
                }),
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "rust-purescript-language-server".to_string(),
                version: Some("0.1.0".to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {}

    async fn did_change_configuration(&self, _params: DidChangeConfigurationParams) {
        // Check if we're already initialized
        let (is_initialized, workspace_root) = {
            let state = self.state.lock().await;
            (state.is_initialized(), state.workspace_root.clone())
        };

        if !is_initialized {
            // First time setup - initialize the server
            if let Some(root) = workspace_root {
                if let Err(e) = self.initialize_server(&root).await {
                    self.client
                        .log_message(
                            MessageType::ERROR,
                            format!("Failed to initialize server: {}", e),
                        )
                        .await;
                }
            }
        } else {
            // Already initialized - check if config actually changed
            let new_client_config = config::fetch_client_config(&self.client).await;

            let current_client_config = {
                let state = self.state.lock().await;
                state.config.as_ref().map(|c| config::ClientConfig {
                    formatter: Some(c.formatter.clone()),
                    fast_rebuild_on_save: Some(c.fast_rebuild_on_save),
                    fast_rebuild_on_change: Some(c.fast_rebuild_on_change),
                })
            };

            if new_client_config != current_client_config {
                self.client
                    .log_message(
                        MessageType::INFO,
                        "Client configuration changed, restarting IDE server",
                    )
                    .await;

                if let Err(e) = self.restart_server().await {
                    self.client
                        .log_message(
                            MessageType::ERROR,
                            format!("Failed to restart server after configuration change: {}", e),
                        )
                        .await;
                }
            } else {
                self.client
                    .log_message(
                        MessageType::INFO,
                        "Configuration unchanged, skipping restart",
                    )
                    .await;
            }
        }
    }

    async fn shutdown(&self) -> LspResult<()> {
        // Take the process from state to get ownership
        let mut process = {
            let mut state = self.state.lock().await;
            state.ide_server.process.take()
        };

        // Kill the process if it exists
        if let Some(ref mut child) = process {
            match child.kill() {
                Ok(_) => {
                    self.client
                        .log_message(MessageType::INFO, "PureScript IDE server stopped")
                        .await;
                }
                Err(e) => {
                    self.client
                        .log_message(
                            MessageType::WARNING,
                            format!("Failed to stop IDE server: {}", e),
                        )
                        .await;
                }
            }
        }

        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = &params.text_document.uri;
        let content = params.text_document.text.clone();

        // Store document content
        {
            let mut state = self.state.lock().await;
            state.document_contents.insert(uri.clone(), content.clone());
        }

        // Trigger fast rebuild on open when fast_rebuild_on_change is enabled
        let (fast_rebuild_enabled, port) = {
            let state = self.state.lock().await;
            (state.fast_rebuild_on_change(), state.ide_server.port)
        };

        if fast_rebuild_enabled {
            if let Some(port) = port {
                if let Ok(file_path) = uri.to_file_path() {
                    if let Some(file_path_str) = file_path.to_str() {
                        // Skip rebuild if file contains foreign imports
                        // (fast rebuild from content doesn't work with foreign modules)
                        if !content.contains("foreign import") {
                            // Pass the content for data: prefix rebuild
                            self.trigger_fast_rebuild(port, file_path_str, uri, Some(content))
                                .await;
                        }
                    }
                }
            }
        }
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = &params.text_document.uri;

        // Store updated document content
        if let Some(change) = params.content_changes.first() {
            let content = change.text.clone();

            {
                let mut state = self.state.lock().await;
                state.document_contents.insert(uri.clone(), content.clone());
            }

            // Optionally trigger fast rebuild on change using data: prefix
            let (fast_rebuild_enabled, port) = {
                let state = self.state.lock().await;
                (state.fast_rebuild_on_change(), state.ide_server.port)
            };

            if fast_rebuild_enabled {
                if let Some(port) = port {
                    if let Ok(file_path) = uri.to_file_path() {
                        if let Some(file_path_str) = file_path.to_str() {
                            // Skip rebuild if file contains foreign imports
                            // (fast rebuild from content doesn't work with foreign modules)
                            if !content.contains("foreign import") {
                                // Pass the content for data: prefix rebuild
                                self.trigger_fast_rebuild(port, file_path_str, uri, Some(content))
                                    .await;
                            }
                        }
                    }
                }
            }
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = &params.text_document.uri;

        // Get state values and immediately drop the lock
        let (fast_rebuild_enabled, port) = {
            let state = self.state.lock().await;
            (state.fast_rebuild_on_save(), state.ide_server.port)
        }; // Lock is dropped here

        if fast_rebuild_enabled {
            if let Some(port) = port {
                if let Ok(file_path) = uri.to_file_path() {
                    if let Some(file_path_str) = file_path.to_str() {
                        // For saves, rebuild from disk (no content passed)
                        self.trigger_fast_rebuild(port, file_path_str, uri, None)
                            .await;
                    } else {
                        self.client
                            .log_message(
                                MessageType::ERROR,
                                "Could not convert file path to string",
                            )
                            .await;
                    }
                } else {
                    self.client
                        .log_message(MessageType::ERROR, "Could not convert URI to file path")
                        .await;
                }
            } else {
                self.client
                    .log_message(MessageType::ERROR, "IDE server port not available")
                    .await;
            }
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = &params.text_document.uri;

        // Remove document content and errors when closed
        {
            let mut state = self.state.lock().await;
            state.document_contents.remove(uri);
            state.document_errors.remove(uri);
        }
    }

    async fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> LspResult<Option<Vec<TextEdit>>> {
        // Get formatter config and document content, then immediately drop the lock
        let (formatter_opt, document_content) = {
            let state = self.state.lock().await;
            (
                state.formatter(),
                state
                    .document_contents
                    .get(&params.text_document.uri)
                    .cloned(),
            )
        }; // Lock is dropped here

        let Some(formatter) = formatter_opt else {
            self.client
                .log_message(
                    MessageType::WARNING,
                    "Server not initialized, cannot format",
                )
                .await;
            return Ok(None);
        };

        let Some(content) = document_content else {
            self.client
                .log_message(
                    MessageType::ERROR,
                    "Document content not found in state, cannot format",
                )
                .await;
            return Ok(None);
        };

        match formatting::format_document_content(&content, &formatter).await {
            Ok(edits) => Ok(edits),
            Err(e) => {
                self.client
                    .log_message(MessageType::ERROR, format!("Formatting failed: {}", e))
                    .await;
                Ok(None)
            }
        }
    }

    async fn code_action(&self, params: CodeActionParams) -> LspResult<Option<CodeActionResponse>> {
        // Clone errors and immediately drop the lock to avoid deadlock
        let errors = {
            let state = self.state.lock().await;
            state
                .document_errors
                .get(&params.text_document.uri)
                .cloned()
                .unwrap_or_default()
        }; // Lock is dropped here

        if errors.is_empty() {
            return Ok(Some(vec![]));
        }

        // Generate code actions for errors that overlap with the requested range
        let mut code_actions = code_actions::generate_code_actions(&params, &errors);

        // Add "Apply all fixes" action if we have multiple fixable errors in the document
        let total_fixable_errors = errors
            .iter()
            .filter(|error| code_actions::has_fixable_suggestion(error))
            .count();

        if total_fixable_errors > 1 {
            if let Some(apply_all_action) = code_actions::create_apply_all_action(&params, &errors)
            {
                code_actions.push(apply_all_action);
            }
        }

        Ok(Some(
            code_actions
                .into_iter()
                .map(CodeActionOrCommand::CodeAction)
                .collect(),
        ))
    }

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> LspResult<Option<serde_json::Value>> {
        // Special handling for focusDocument command to call our method directly
        if params.command == "purescript.focusDocument" {
            if let Some(uri_value) = params.arguments.first() {
                if let Ok(uri_str) = serde_json::from_value::<String>(uri_value.clone()) {
                    if let Ok(uri) = Url::parse(&uri_str) {
                        self.handle_document_focus(&uri).await;
                    }
                }
            }
            return Ok(None);
        }

        let args = if params.arguments.is_empty() {
            None
        } else {
            Some(params.arguments.clone())
        };

        if let Err(e) =
            commands::execute_command(&params.command, &self.client, &self.state, args).await
        {
            self.client
                .log_message(MessageType::ERROR, format!("Command failed: {}", e))
                .await;
        }
        Ok(None)
    }
}

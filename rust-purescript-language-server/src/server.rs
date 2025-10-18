use crate::code_actions;
use crate::commands;
use crate::diagnostics;
use crate::formatting;
use crate::ide_server::{commands as ide_commands, process};
use crate::ragu;
use crate::types::{Config, ServerState};
use lsp_types::{
    ProgressParams, ProgressParamsValue, WorkDoneProgress, WorkDoneProgressBegin,
    WorkDoneProgressCreateParams, WorkDoneProgressEnd,
    notification::Progress, request::WorkDoneProgressCreate,
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
        let config = Config::default();
        let state = Arc::new(Mutex::new(ServerState::new(config)));

        Self { client, state }
    }

    /// Initialize the server with ragu configuration
    async fn initialize_server(&self, workspace_root: &str) -> anyhow::Result<()> {
        self.client
            .log_message(
                MessageType::INFO,
                format!("Initializing server for workspace: {}", workspace_root),
            )
            .await;
        // Get configuration from ragu
        let config = ragu::init_config(workspace_root)?;
        self.client
            .log_message(
                MessageType::INFO,
                format!("Output directory: {}", config.output_dir,),
            )
            .await;
        self.client
            .log_message(
                MessageType::INFO,
                format!("Number of source globs: {}", config.source_globs.len()),
            )
            .await;

        let (process, port) = process::start_ide_server_async(
            workspace_root,
            &config.output_dir,
            &config.source_globs,
        )
        .await?;

        // Update state
        let mut state = self.state.lock().await;
        state.config = config;
        state.workspace_root = Some(workspace_root.to_string());
        state.ide_server.port = Some(port);
        state.ide_server.process = Some(process);
        state.ide_server.working_dir = Some(workspace_root.to_string());

        self.client
            .log_message(
                MessageType::INFO,
                format!("Purescript IDE server started on port {}", port),
            )
            .await;

        Ok(())
    }

    /// Trigger fast rebuild for a file
    async fn trigger_fast_rebuild(&self, port: u16, file_path: &str, uri: &Url) {
        // Extract filename for display
        let file_name = std::path::Path::new(file_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file");

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
        }

        // Send begin notification
        self.client
            .send_notification::<Progress>(ProgressParams {
                token: token.clone(),
                value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(
                    WorkDoneProgressBegin {
                        title: "⏳".into(),
                        message: Some(file_name.into()),
                        cancellable: Some(false),
                        percentage: None,
                    },
                )),
            })
            .await;

        let result = ide_commands::rebuild_file(port, file_path).await;

        // Send end notification
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

}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
        // Initialize server if we have a workspace root
        if let Some(workspace_root) = params.root_uri.and_then(|uri| uri.to_file_path().ok()) {
            if let Some(root_str) = workspace_root.to_str() {
                if let Err(e) = self.initialize_server(root_str).await {
                    self.client
                        .log_message(
                            MessageType::ERROR,
                            format!("Failed to initialize server: {}", e),
                        )
                        .await;
                }
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
                    ],
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

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(
                MessageType::INFO,
                "Rust PureScript Language Server initialized",
            )
            .await;
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = &params.text_document.uri;
        // Store document content
        {
            let mut state = self.state.lock().await;
            state
                .document_contents
                .insert(uri.clone(), params.text_document.text.clone());
        }
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // Store updated document content
        if let Some(change) = params.content_changes.first() {
            let mut state = self.state.lock().await;
            state
                .document_contents
                .insert(params.text_document.uri.clone(), change.text.clone());
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = &params.text_document.uri;

        // Get state values and immediately drop the lock
        let (fast_rebuild_enabled, port) = {
            let state = self.state.lock().await;
            (state.config.fast_rebuild_on_save, state.ide_server.port)
        }; // Lock is dropped here

        if fast_rebuild_enabled {
            if let Some(port) = port {
                if let Ok(file_path) = uri.to_file_path() {
                    if let Some(file_path_str) = file_path.to_str() {
                        self.trigger_fast_rebuild(port, file_path_str, uri).await;
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
        let (formatter, document_content) = {
            let state = self.state.lock().await;
            (
                state.config.formatter.clone(),
                state
                    .document_contents
                    .get(&params.text_document.uri)
                    .cloned(),
            )
        }; // Lock is dropped here

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
        if let Err(e) = commands::execute_command(&params.command, &self.client, &self.state).await {
            self.client
                .log_message(
                    MessageType::ERROR,
                    format!("Command failed: {}", e),
                )
                .await;
        }
        Ok(None)
    }
}

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tower_lsp::Client;
use tower_lsp::lsp_types::{ConfigurationItem, MessageType};

/// Configuration for the language server
///
/// This is built programmatically from ragu (for project structure)
/// and ClientConfig (for user preferences), never deserialized directly.
///
/// If ragu fails, initialization will fail - there are no fallback defaults.
#[derive(Debug, Clone, Serialize)]
pub struct Config {
    pub output_dir: String,
    pub source_globs: Vec<String>,
    pub formatter: Formatter,
    pub fast_rebuild_on_save: bool,
    pub fast_rebuild_on_change: bool,
}

impl Config {
    /// Merge this config with values from a client config, preferring client values when present
    pub fn merge_with_client_config(&mut self, client_config: ClientConfig) {
        if let Some(formatter) = client_config.formatter {
            self.formatter = formatter;
        }
        if let Some(fast_rebuild_on_save) = client_config.fast_rebuild_on_save {
            self.fast_rebuild_on_save = fast_rebuild_on_save;
        }
        if let Some(fast_rebuild_on_change) = client_config.fast_rebuild_on_change {
            self.fast_rebuild_on_change = fast_rebuild_on_change;
        }
    }
}

/// Client-provided configuration (all fields optional to allow partial updates)
///
/// Note: output_dir and source_globs are intentionally not configurable here.
/// These are always sourced from ragu, which is the single source of truth
/// for project structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClientConfig {
    pub formatter: Option<Formatter>,
    pub fast_rebuild_on_save: Option<bool>,
    pub fast_rebuild_on_change: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Formatter {
    PursTidy,
    #[serde(alias = "pursfmt")]
    PursFmt,
}

impl Default for Formatter {
    fn default() -> Self {
        Formatter::PursTidy
    }
}

/// Initialize configuration using ragu for defaults
///
/// This queries ragu for the output directory and source globs,
/// which can then be optionally overridden by client configuration.
pub fn init_from_ragu(working_dir: &str) -> Result<Config> {
    let output_dir = crate::ragu::get_output_dir(working_dir)?;
    let source_globs = crate::ragu::get_sources(working_dir)?;

    Ok(Config {
        output_dir,
        source_globs,
        formatter: Formatter::PursFmt,
        fast_rebuild_on_save: true,
        fast_rebuild_on_change: true,
    })
}

/// Initialize configuration with optional client overrides
///
/// This is the main entry point for configuration initialization.
/// It gets defaults from ragu and merges them with client-provided settings.
pub fn init_with_client_config(
    working_dir: &str,
    client_config: Option<ClientConfig>,
) -> Result<Config> {
    let mut config = init_from_ragu(working_dir)?;

    if let Some(client_cfg) = client_config {
        config.merge_with_client_config(client_cfg);
    }

    Ok(config)
}

/// Fetch client configuration from the LSP client
///
/// Requests configuration from the client using the workspace/configuration request.
/// Returns None if the request fails or the configuration cannot be parsed.
pub async fn fetch_client_config(client: &Client) -> Option<ClientConfig> {
    match client
        .configuration(vec![ConfigurationItem {
            scope_uri: None,
            section: Some("purescriptRust".to_string()),
        }])
        .await
    {
        Ok(configs) => {
            if let Some(config_value) = configs.first() {
                match serde_json::from_value::<ClientConfig>(config_value.clone()) {
                    Ok(client_config) => Some(client_config),
                    Err(e) => {
                        client
                            .log_message(
                                MessageType::WARNING,
                                format!("Failed to parse client config: {}", e),
                            )
                            .await;
                        None
                    }
                }
            } else {
                None
            }
        }
        Err(e) => {
            client
                .log_message(
                    MessageType::WARNING,
                    format!("Failed to request configuration: {}", e),
                )
                .await;
            None
        }
    }
}

/// Initialize configuration by fetching from client and merging with ragu defaults
///
/// This is a high-level helper that combines fetching client config and initializing.
pub async fn init_from_client_and_ragu(client: &Client, working_dir: &str) -> Result<Config> {
    let client_config = fetch_client_config(client).await;

    if client_config.is_some() {
        client
            .log_message(
                MessageType::INFO,
                "Using client configuration merged with ragu defaults",
            )
            .await;
    } else {
        client
            .log_message(MessageType::INFO, "Using ragu defaults only")
            .await;
    }

    init_with_client_config(working_dir, client_config)
}

/// Log the current configuration to the client
pub async fn log_config(client: &Client, config: &Config) {
    client
        .log_message(
            MessageType::INFO,
            format!("Output directory: {}", config.output_dir),
        )
        .await;
    client
        .log_message(
            MessageType::INFO,
            format!("Number of source globs: {}", config.source_globs.len()),
        )
        .await;
    client
        .log_message(
            MessageType::INFO,
            format!("Formatter: {:?}", config.formatter),
        )
        .await;
    client
        .log_message(
            MessageType::INFO,
            format!("Fast rebuild on save: {}", config.fast_rebuild_on_save),
        )
        .await;
    client
        .log_message(
            MessageType::INFO,
            format!("Fast rebuild on change: {}", config.fast_rebuild_on_change),
        )
        .await;
}

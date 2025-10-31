use crate::config::Config;
use crate::ide_server::RebuildError;
use std::collections::HashMap;
use tower_lsp::lsp_types::Url;

/// IDE server state
#[derive(Debug)]
pub struct IdeServerState {
    pub port: Option<u16>,
    pub process: Option<std::process::Child>,
    pub working_dir: Option<String>,
}

impl Default for IdeServerState {
    fn default() -> Self {
        Self {
            port: None,
            process: None,
            working_dir: None,
        }
    }
}

/// Server state
#[derive(Debug)]
pub struct ServerState {
    pub config: Option<Config>,
    pub ide_server: IdeServerState,
    pub workspace_root: Option<String>,
    pub document_errors: HashMap<Url, Vec<RebuildError>>,
    pub last_build_errors: HashMap<Url, Vec<RebuildError>>,
    pub document_contents: HashMap<Url, String>,
    pub rebuild_counter: u64,
}

impl Default for ServerState {
    fn default() -> Self {
        Self {
            config: None,
            ide_server: IdeServerState::default(),
            workspace_root: None,
            document_errors: HashMap::new(),
            last_build_errors: HashMap::new(),
            document_contents: HashMap::new(),
            rebuild_counter: 0,
        }
    }
}

impl ServerState {
    /// Check if fast rebuild on save is enabled (returns false if not initialized)
    pub fn fast_rebuild_on_save(&self) -> bool {
        self.config
            .as_ref()
            .map(|c| c.fast_rebuild_on_save)
            .unwrap_or(false)
    }

    /// Check if fast rebuild on change is enabled (returns false if not initialized)
    pub fn fast_rebuild_on_change(&self) -> bool {
        self.config
            .as_ref()
            .map(|c| c.fast_rebuild_on_change)
            .unwrap_or(false)
    }

    /// Get the formatter (returns None if not initialized)
    pub fn formatter(&self) -> Option<crate::config::Formatter> {
        self.config.as_ref().map(|c| c.formatter.clone())
    }

    /// Check if the server is initialized with a valid config
    pub fn is_initialized(&self) -> bool {
        self.config.is_some()
    }
}

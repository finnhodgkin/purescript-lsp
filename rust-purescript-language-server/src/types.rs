use crate::ide_server::RebuildError;
use std::collections::HashMap;
use tower_lsp::lsp_types::{NumberOrString, Url};

/// Configuration for the language server
#[derive(Debug, Clone)]
pub struct Config {
    pub output_dir: String,
    pub source_globs: Vec<String>,
    pub formatter: Formatter,
    pub fast_rebuild_on_save: bool,
    pub fast_rebuild_on_change: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            output_dir: "output".to_string(),
            source_globs: vec!["src/**/*.purs".to_string()],
            formatter: Formatter::PursTidy,
            fast_rebuild_on_save: true,
            fast_rebuild_on_change: false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Formatter {
    PursTidy,
}

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
    pub config: Config,
    pub ide_server: IdeServerState,
    pub workspace_root: Option<String>,
    pub document_errors: HashMap<Url, Vec<RebuildError>>,
    pub last_build_errors: HashMap<Url, Vec<RebuildError>>,
    pub document_contents: HashMap<Url, String>,
    pub active_rebuild_token: Option<NumberOrString>,
}

impl ServerState {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            ide_server: IdeServerState::default(),
            workspace_root: None,
            document_errors: HashMap::new(),
            last_build_errors: HashMap::new(),
            document_contents: HashMap::new(),
            active_rebuild_token: None,
        }
    }
}

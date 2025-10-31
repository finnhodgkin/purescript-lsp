use anyhow::Result;
use tower_lsp::{LspService, Server};

mod build;
mod code_actions;
mod commands;
mod config;
mod diagnostics;
mod formatting;
mod ide_server;
mod ragu;
mod server;
mod types;

use server::Backend;

#[tokio::main]
async fn main() -> Result<()> {
    // Check for version flag
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "--version" {
        println!("rust-purescript-language-server 0.1.0");
        return Ok(());
    }

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend::new(client));
    Server::new(stdin, stdout, socket).serve(service).await;

    Ok(())
}

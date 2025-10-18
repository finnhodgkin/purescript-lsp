# Rust PureScript Language Server

A PureScript language server that provides diagnostics, code actions, and formatting.

## Features

- Fast rebuild diagnostics on save via `purs ide server`
- Full project build commands (`purescript.build` and `purescript.buildQuick`)
- Project-wide diagnostics from full builds
- Code action fixes for compiler suggestions
- Document formatting with purs-tidy
- Automatic configuration via ragu

## Requirements

- `purs` (PureScript compiler)
- `ragu` (for configuration)
- `purs-tidy` (for formatting)

## Building

```bash
cargo build --release
```

## Usage

The language server can be used with any LSP-compatible editor:

```bash
# Run the server
./target/release/rust-purescript-language-server
```

## Configuration

The server automatically configures itself using `ragu` for output directory and source globs.

## Commands

The language server provides the following commands that can be executed from your editor's command palette:

- **`purescript.build`** - Run a full project build (`ragu build -- --json-errors`)

  - Shows progress indicator with real-time output
  - Publishes diagnostics for all files with errors/warnings
  - Runs asynchronously without blocking the editor

- **`purescript.buildQuick`** - Run a quick build (`ragu build -q -- --json-errors`)
  - Only builds exact project sources (faster)
  - Same diagnostics and progress features as full build

Both commands stream compiler output to the LSP output window and display comprehensive diagnostics across all affected files.

## Development

The codebase is organized into modules:

- `src/server.rs` - Main LSP server implementation
- `src/build.rs` - Ragu build command execution
- `src/ide_server/` - IDE server communication
- `src/ragu.rs` - Ragu integration
- `src/diagnostics.rs` - Diagnostic conversion
- `src/code_actions.rs` - Code action generation
- `src/formatting.rs` - Formatting support
- `src/types.rs` - Common types

# PureScript Rust Language Server - VS Code Extension

A VS Code extension that provides PureScript language support using the Rust-based PureScript language server.

## Features

- **Fast Diagnostics**: Real-time error checking and warnings on save
- **Code Actions**: Quick fixes for compiler suggestions and import issues
- **Document Formatting**: Support for purs-tidy, pose, and purty formatters
- **Ragu Integration**: Automatic configuration using ragu
- **Syntax Highlighting**: PureScript syntax highlighting
- **IntelliSense**: Basic language features

## Requirements

- VS Code 1.74.0 or higher
- Rust (for building the language server)
- `purs` (PureScript compiler)
- `ragu` (for configuration)
- One of the supported formatters (purs-tidy, pose, or purty)

## Installation

### 1. Install the Rust Language Server

First, install the Rust language server globally:

```bash
# From the project root
./install-server.sh
```

Or manually:

```bash
cd rust-purescript-language-server
cargo install --path . --force
```

### 2. Install the VS Code Extension

```bash
cd vscode-purescript-rust-extension
npm install
npm run compile
npx vsce package
code --install-extension *.vsix
```

## Configuration

The extension can be configured through VS Code settings:

### `purescriptRust.formatter`

- **Type**: `string`
- **Options**: `"purs-tidy"`, `"pose"`, `"purty"`
- **Default**: `"purs-tidy"`
- **Description**: Formatter to use for PureScript files

### `purescriptRust.fastRebuildOnSave`

- **Type**: `boolean`
- **Default**: `true`
- **Description**: Enable fast rebuild diagnostics on save

### `purescriptRust.raguPath`

- **Type**: `string`
- **Default**: `"ragu"`
- **Description**: Path to the ragu executable for configuration

## Commands

The extension provides the following commands:

- **PureScript: Restart PureScript Language Server** - Restart the language server
- **PureScript: Show Language Server Output** - Show the language server output channel

## Usage

1. Open a PureScript project in VS Code
2. The extension will automatically activate when you open a `.purs` file
3. The language server will start and provide diagnostics, formatting, and code actions
4. Use `Ctrl+Shift+P` (or `Cmd+Shift+P` on macOS) to access the command palette and run PureScript commands

## Development

To develop the extension:

1. Clone the repository
2. Install dependencies:
   ```bash
   npm install
   ```
3. Compile the TypeScript:
   ```bash
   npm run compile
   ```
4. Open the project in VS Code
5. Press `F5` to run the extension in a new Extension Development Host window

## Troubleshooting

### Language Server Not Starting

1. Check that the Rust language server executable is in your PATH
2. Verify that `purs` and `ragu` are installed and accessible
3. Check the Output panel for error messages

### No Diagnostics

1. Ensure your project is a valid PureScript project (has `spago.dhall`, `spago.yaml`, `bower.json`, or `psc-package.json`)
2. Check that `ragu` can find your project configuration
3. Verify that the project builds successfully with `purs compile`

### Formatting Not Working

1. Ensure the selected formatter is installed and in your PATH
2. Check the formatter configuration in VS Code settings
3. Try restarting the language server

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Test thoroughly
5. Submit a pull request

## License

MIT License

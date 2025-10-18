import {
  workspace,
  ExtensionContext,
  window,
  commands,
  WorkspaceEdit,
} from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from 'vscode-languageclient/node';

let client: LanguageClient;

interface ApplyAndSaveCommand {
  // We need to define the structure that the server will send
  edit: WorkspaceEdit;
  uriToSave: string; // The server will send this as a string
}

export function activate(context: ExtensionContext) {
  const outputChannel = window.createOutputChannel('Purescript LSP');
  outputChannel.show(true);

  outputChannel.appendLine('Activating Purescript language server...');

  // The server is implemented in Rust
  // Use the binary from PATH instead of a hardcoded location
  const serverCommand = 'rust-purescript-language-server';

  const serverOptions: ServerOptions = {
    command: serverCommand,
    transport: TransportKind.stdio,
  };

  // Options to control the language client
  const clientOptions: LanguageClientOptions = {
    // Register the server for purescript documents
    documentSelector: [
      { scheme: 'file', language: 'purescript' },
      { scheme: 'file', pattern: '**/package.json' },
      { scheme: 'file', pattern: '**/spago.yaml' },
    ],
    synchronize: {
      // Notify the server about file changes
      fileEvents: workspace.createFileSystemWatcher('**/{*.purs,spago.yaml}'),
    },
    outputChannel,
  };

  // Create the language client and start the client.
  client = new LanguageClient(
    'purscriptLSP',
    'Purescript LSP',
    serverOptions,
    clientOptions
  );
  // Start the client. This will also launch the server
  client.start().catch((error) => {
    outputChannel.appendLine(`Failed to start language client: ${error}`);
    window.showErrorMessage('Failed to start Purescript LSP language client.');
  });
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}

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

function getConfiguration() {
  const config = workspace.getConfiguration('purescriptRust');

  return {
    formatter: config.get<string>('formatter'),
    fastRebuildOnSave: config.get<boolean>('fastRebuildOnSave'),
    fastRebuildOnChange: config.get<boolean>('fastRebuildOnChange'),
  };
}

async function startLanguageClient(
  context: ExtensionContext,
  outputChannel: any
): Promise<void> {
  outputChannel.appendLine('Starting PureScript language server...');

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
    documentSelector: [{ scheme: 'file', language: 'purescript' }],
    synchronize: {
      // Notify the server about file changes and configuration changes
      fileEvents: workspace.createFileSystemWatcher('**/*.purs'),
      configurationSection: 'purescriptRust',
    },
    outputChannel,
    initializationOptions: getConfiguration(),
  };

  // Create the language client and start the client.
  client = new LanguageClient(
    'purscriptLSP',
    'Purescript LSP',
    serverOptions,
    clientOptions
  );

  // Start the client. This will also launch the server
  await client.start();

  outputChannel.appendLine('Language client started successfully');

  // Handle document focus events - trigger quick build when a PureScript file becomes active
  // Only set up after client is ready
  context.subscriptions.push(
    window.onDidChangeActiveTextEditor((editor) => {
      if (editor && editor.document.languageId === 'purescript') {
        // Send focusDocument command to the language server
        client
          .sendRequest('workspace/executeCommand', {
            command: 'purescript.focusDocument',
            arguments: [editor.document.uri.toString()],
          })
          .then(
            () => {
              // Command executed successfully
            },
            (error) => {
              outputChannel.appendLine(`Failed to send focus event: ${error}`);
            }
          );
      }
    })
  );
}

export function activate(context: ExtensionContext) {
  const outputChannel = window.createOutputChannel('Purescript LSP');
  outputChannel.show(true);

  outputChannel.appendLine('Activating PureScript language server...');

  // Start the language client
  startLanguageClient(context, outputChannel).catch((error) => {
    outputChannel.appendLine(`Failed to start language client: ${error}`);
    window.showErrorMessage('Failed to start PureScript LSP language client.');
  });

  // Watch for configuration changes and restart the server
  context.subscriptions.push(
    workspace.onDidChangeConfiguration(async (e) => {
      if (e.affectsConfiguration('purescriptRust')) {
        outputChannel.appendLine(
          'Configuration changed, restarting language server...'
        );

        // Stop the current client
        if (client) {
          await client.stop();
        }

        // Start a new client with the updated configuration
        try {
          await startLanguageClient(context, outputChannel);
          outputChannel.appendLine('Language server restarted successfully');
        } catch (error) {
          outputChannel.appendLine(
            `Failed to restart language server: ${error}`
          );
          window.showErrorMessage(
            'Failed to restart PureScript LSP language client.'
          );
        }
      }
    })
  );
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}

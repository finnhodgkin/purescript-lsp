import * as assert from 'assert';
import * as vscode from 'vscode';

suite('Extension Test Suite', () => {
  vscode.window.showInformationMessage('Start all tests.');

  test('Extension should be present', () => {
    assert.ok(
      vscode.extensions.getExtension(
        'purescript-rust.purescript-rust-language-server'
      )
    );
  });

  test('Should activate', async () => {
    const extension = vscode.extensions.getExtension(
      'purescript-rust.purescript-rust-language-server'
    );
    if (extension) {
      await extension.activate();
      assert.ok(extension.isActive);
    }
  });
});

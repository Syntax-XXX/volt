// VS-Code-Addon für Volt: startet `volt lsp` (Fehler, Vervollständigung, Hover), formatiert über `volt fmt --stdin`
// und bringt Befehle zum Ausführen/Testen/Servieren.
const vscode = require('vscode');
const { execFile } = require('child_process');
const { LanguageClient } = require('vscode-languageclient/node');

let client;
const voltPath = () => vscode.workspace.getConfiguration('volt').get('pfad') || 'volt';

async function startClient(context) {
  client = new LanguageClient('volt', 'Volt', { command: voltPath(), args: ['lsp'] }, { documentSelector: [{ scheme: 'file', language: 'volt' }] });
  try { await client.start(); }
  catch (e) { vscode.window.showWarningMessage(`Volt: Language-Server startet nicht (${e.message}). Ist \`volt\` installiert? Pfad in den Einstellungen: volt.pfad`); client = undefined; }
}

function terminalFor(args) {
  const file = vscode.window.activeTextEditor?.document;
  if (!file || file.languageId !== 'volt') { vscode.window.showInformationMessage('Öffne zuerst eine .vt- oder .vts-Datei.'); return; }
  file.save().then(() => {
    const t = vscode.window.terminals.find((x) => x.name === 'Volt') || vscode.window.createTerminal('Volt');
    t.show(true);
    t.sendText(`${JSON.stringify(voltPath())} ${args} ${JSON.stringify(file.fileName)}`);
  });
}

function activate(context) {
  startClient(context);
  context.subscriptions.push(
    vscode.commands.registerCommand('volt.run', () => terminalFor('run')),
    vscode.commands.registerCommand('volt.test', () => terminalFor('test')),
    vscode.commands.registerCommand('volt.serve', () => terminalFor('serve')),
    vscode.commands.registerCommand('volt.restart', async () => { if (client) await client.stop(); await startClient(context); }),
    vscode.languages.registerDocumentFormattingEditProvider('volt', {
      provideDocumentFormattingEdits(doc) {
        return new Promise((resolve) => {
          const p = execFile(voltPath(), ['fmt', '--stdin'], { maxBuffer: 16 * 1024 * 1024 }, (err, out) => {
            if (err) return resolve([]);
            const full = new vscode.Range(doc.positionAt(0), doc.positionAt(doc.getText().length));
            resolve(out === doc.getText() ? [] : [vscode.TextEdit.replace(full, out)]);
          });
          p.stdin.end(doc.getText());
        });
      },
    }),
  );
}

function deactivate() { return client ? client.stop() : undefined; }
module.exports = { activate, deactivate };

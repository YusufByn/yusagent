// A tool's code or output, opened from the panel in an editor tab: read-only,
// never on disk, its language from its name.
import * as vscode from "vscode";

export class ToolTabs implements vscode.TextDocumentContentProvider, vscode.Disposable {
  static readonly scheme = "unharnessed-tool";
  private texts = new Map<string, string>();
  private changes = new vscode.EventEmitter<vscode.Uri>();
  readonly onDidChange = this.changes.event;
  private closing = vscode.workspace.onDidCloseTextDocument((document) => this.texts.delete(document.uri.toString()));

  dispose(): void {
    this.changes.dispose();
    this.closing.dispose();
  }

  provideTextDocumentContent(uri: vscode.Uri): string {
    return this.texts.get(uri.toString()) ?? "";
  }

  /** `text` in a tab named `name`, under the tool's id: a tab already open on it shows it. */
  async open(id: string, name: string, text: string): Promise<vscode.TextDocument> {
    const uri = vscode.Uri.from({ scheme: ToolTabs.scheme, path: `/${id.replace(/[^\w.-]/g, "_")}/${name}` });
    const key = uri.toString();
    const before = this.texts.get(key);
    this.texts.set(key, text);
    if (before !== undefined && before !== text) this.changes.fire(uri);
    const document = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(document, { preview: false });
    return document;
  }
}

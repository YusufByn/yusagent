// Onglet d'éditeur d'un chat (choix 1) : webview React groupée par esbuild, CSP stricte (E3).

import { randomBytes } from "node:crypto";
import * as vscode from "vscode";

import { ChatSession } from "./session";
import type { Probe, ToHost, ToWebview } from "./shared/messages";
import type { ChatState } from "./shared/state";

export const VIEW_TYPE = "yusagent.chat";

export class ChatPanel {
  readonly session: ChatSession;
  private readonly panel: vscode.WebviewPanel;
  private webviewReady = false;
  private flushTimer: NodeJS.Timeout | undefined;
  private probes: ((probe: Probe) => void)[] = [];

  constructor(
    context: vscode.ExtensionContext,
    title: string,
    output: vscode.OutputChannel,
    private readonly onDisposed: (panel: ChatPanel) => void,
  ) {
    const dist = vscode.Uri.joinPath(context.extensionUri, "dist");
    this.panel = vscode.window.createWebviewPanel(VIEW_TYPE, title, vscode.ViewColumn.Beside, {
      enableScripts: true,
      retainContextWhenHidden: true,
      localResourceRoots: [dist],
    });
    this.panel.webview.html = html(this.panel.webview, dist);

    const folder = vscode.workspace.workspaceFolders?.[0];
    this.session = new ChatSession({
      binaryPath: vscode.workspace.getConfiguration("yusagent").get<string>("binaryPath"),
      cwd: folder?.uri.scheme === "file" ? folder.uri.fsPath : undefined,
      workspaceName: folder?.name,
      env: process.env,
      log: (line) => output.appendLine(`[${title}] ${line}`),
      onState: () => this.scheduleFlush(),
    });

    this.panel.webview.onDidReceiveMessage((message: ToHost) => this.handle(message));
    this.panel.onDidDispose(() => {
      if (this.flushTimer) clearTimeout(this.flushTimer);
      void this.session.dispose();
      this.onDisposed(this);
    });
    void this.session.start();
  }

  get html(): string {
    return this.panel.webview.html;
  }

  get state(): ChatState {
    return this.session.state;
  }

  /** La webview a chargé son script et monté React (message `ready` reçu). */
  get rendered(): boolean {
    return this.webviewReady;
  }

  /** Texte affiché par la webview (tests e2e). */
  probe(): Promise<Probe> {
    return new Promise((resolveProbe) => {
      this.probes.push(resolveProbe);
      const message: ToWebview = { type: "probe" };
      void this.panel.webview.postMessage(message);
    });
  }

  /** Messages de la webview (et des tests e2e, par la même voie). */
  handle(message: ToHost): void {
    switch (message.type) {
      case "ready":
        this.webviewReady = true;
        this.flush();
        break;
      case "send":
        void this.session.send(message.text);
        break;
      case "stop":
        this.session.stop();
        break;
      case "setConfig":
        void this.session.setConfig(message.configId, message.value);
        break;
      case "openSetting":
        void vscode.commands.executeCommand("workbench.action.openSettings", "yusagent.binaryPath");
        break;
      case "retry":
        void this.session.retry();
        break;
      case "probeResult":
        this.probes.shift()?.(message.probe);
        break;
    }
  }

  dispose(): void {
    this.panel.dispose();
  }

  /** Le streaming envoie un état par morceau : regroupé sur ~16 ms. */
  private scheduleFlush(): void {
    if (this.flushTimer) return;
    this.flushTimer = setTimeout(() => {
      this.flushTimer = undefined;
      this.flush();
    }, 16);
  }

  private flush(): void {
    if (!this.webviewReady) return;
    const message: ToWebview = { type: "state", state: this.session.state };
    void this.panel.webview.postMessage(message);
  }
}

function html(webview: vscode.Webview, dist: vscode.Uri): string {
  const nonce = randomBytes(16).toString("base64");
  const script = webview.asWebviewUri(vscode.Uri.joinPath(dist, "webview.js"));
  const style = webview.asWebviewUri(vscode.Uri.joinPath(dist, "webview.css"));
  const csp = [
    "default-src 'none'",
    `style-src ${webview.cspSource}`,
    `font-src ${webview.cspSource}`,
    `img-src ${webview.cspSource}`,
    `script-src 'nonce-${nonce}'`,
  ].join("; ");
  return `<!DOCTYPE html>
<html lang="fr">
<head>
<meta charset="UTF-8">
<meta http-equiv="Content-Security-Policy" content="${csp}">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<link rel="stylesheet" href="${style.toString()}">
<title>yusAgent</title>
</head>
<body>
<div id="root"></div>
<script nonce="${nonce}" src="${script.toString()}"></script>
</body>
</html>`;
}

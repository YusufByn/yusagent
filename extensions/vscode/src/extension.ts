import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import * as vscode from "vscode";
import { Conversations } from "./conversations";
import { Files } from "./files";
import { History } from "./history";
import { Providers } from "./providers";
import { Settings } from "./settings";
import { ShownAgent } from "./shown";
import { ToolTabs } from "./tabs";
import type { WebviewMessage } from "./protocol";
import { ChatViewProvider } from "./view";

let conversations: Conversations | undefined;

export function activate(context: vscode.ExtensionContext): void {
  const log = vscode.window.createOutputChannel("Unharnessed", { log: true });
  // UNHARNESSED_BIN is set by the F5 launch config and the E2E; a packaged
  // extension carries its agent in bin/ (scripts/package.mjs); otherwise PATH.
  const packaged = join(context.extensionPath, "bin", process.platform === "win32" ? "unharnessed.exe" : "unharnessed");
  const binary = process.env.UNHARNESSED_BIN || (existsSync(packaged) ? packaged : "unharnessed");
  const cwd = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? homedir();
  const version = String(context.extension.packageJSON.version);
  const shown = new ShownAgent(log);
  // Before the first conversation: they listen to every agent's notifications.
  const providers = new Providers(shown, log);
  const settings = new Settings(shown, log);
  const files = new Files(cwd, log);
  // The E2E sleeps a conversation sooner.
  const sleepAfter = process.env.UNHARNESSED_E2E_LOGS ? Number(process.env.UNHARNESSED_E2E_SLEEP_MS) || undefined : undefined;
  conversations = new Conversations(binary, cwd, version, files, shown, log, sleepAfter);
  const history = new History(shown, conversations, cwd, log);
  const tabs = new ToolTabs();
  const view = new ChatViewProvider(context.extensionUri, conversations, providers, settings, history, files, tabs, log);

  context.subscriptions.push(
    log,
    tabs,
    vscode.workspace.registerTextDocumentContentProvider(ToolTabs.scheme, tabs),
    vscode.window.registerWebviewViewProvider(ChatViewProvider.viewId, view),
    // The button in the editor's title bar: the panel opens, its side bar with it.
    vscode.commands.registerCommand("unharnessed.open", () => vscode.commands.executeCommand(`${ChatViewProvider.viewId}.focus`)),
  );
  // The E2E drives the panel's messages from its suite (test/e2e/suite.cjs).
  if (process.env.UNHARNESSED_E2E_LOGS) {
    context.subscriptions.push(
      vscode.commands.registerCommand("unharnessed.e2e.post", (message: WebviewMessage) => view.receive(message)),
    );
  }
  conversations.start();
}

/** Every agent stops at once, saving its kernel; one VS Code no longer waits for goes on alone. */
export async function deactivate(): Promise<void> {
  await conversations?.stopAll();
}

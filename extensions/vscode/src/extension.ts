// yusAgent pour VS Code (YA-10, ticket 24, PR 2) : chat ACP avec le binaire `yusagent`.

import * as vscode from "vscode";

import { ChatPanel } from "./panel";
import type { ToHost } from "./shared/messages";

const panels: ChatPanel[] = [];
let opened = 0;

export function activate(context: vscode.ExtensionContext): void {
  const output = vscode.window.createOutputChannel("yusAgent");
  context.subscriptions.push(output);

  context.subscriptions.push(
    vscode.commands.registerCommand("yusagent.newChat", () => {
      opened += 1;
      const title = opened === 1 ? "yusAgent" : `yusAgent (${opened})`;
      const panel = new ChatPanel(context, title, output, (closed) => {
        const index = panels.indexOf(closed);
        if (index >= 0) panels.splice(index, 1);
      });
      panels.push(panel);
    }),
  );

  // Commandes des tests e2e seulement (variable posée par le lanceur de test), jamais déclarées
  // dans package.json.
  if (process.env.YUSAGENT_EXTENSION_TEST === "1") {
    context.subscriptions.push(
      vscode.commands.registerCommand("yusagent._test.chats", () =>
        panels.map((panel) => ({ state: panel.state, html: panel.html, rendered: panel.rendered })),
      ),
      vscode.commands.registerCommand("yusagent._test.message", (index: number, message: ToHost) => {
        panels[index]?.handle(message);
      }),
      vscode.commands.registerCommand("yusagent._test.probe", (index: number) => panels[index]?.probe()),
      vscode.commands.registerCommand("yusagent._test.close", (index: number) => {
        panels[index]?.dispose();
      }),
    );
  }
}

export function deactivate(): Promise<void> {
  return Promise.all(panels.map((panel) => panel.session.dispose())).then(() => undefined);
}

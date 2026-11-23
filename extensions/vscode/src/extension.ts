// Repris d'Unharnessed (MIT, 157dcdc1d, vscode/src/extension.ts). yusAgent (YA-10, ticket 24, PR I) :
// binaire du réglage `yusagent.binaryPath` (résolu au lancement, `src/binary.ts`), aucun dossier
// ouvert : pas de lancement (le panneau le dit), identifiants `yusagent.*`, commandes de test.
import * as vscode from "vscode";
import { Conversations } from "./conversations";
import { Files } from "./files";
import { History } from "./history";
import { Providers } from "./providers";
import { Settings } from "./settings";
import { ShownAgent } from "./shown";
import { ToolTabs } from "./tabs";
import type { ProbeInput, WebviewMessage } from "./protocol";
import { ChatViewProvider } from "./view";
import { setDiffFiles } from "./ya/diff-files";
import { DiffDocuments, vscodeDiffFiles } from "./ya/diff-files-vscode";

let conversations: Conversations | undefined;

export function activate(context: vscode.ExtensionContext): void {
  const log = vscode.window.createOutputChannel("yusAgent", { log: true });
  // yusAgent : le réglage, lu à l'activation (nom seul : cherché dans le PATH au lancement).
  const binary = vscode.workspace.getConfiguration("yusagent").get<string>("binaryPath") || "yusagent";
  // yusAgent : sans dossier ouvert, pas de cwd (l'agent ne démarre pas) ; Unharnessed prend le dossier personnel.
  const folder = vscode.workspace.workspaceFolders?.[0];
  const cwd = folder?.uri.scheme === "file" ? folder.uri.fsPath : "";
  const version = String(context.extension.packageJSON.version);
  // yusAgent (PR 3, étape 4) : diffs des cellules et « Rejeter » avec les fichiers de VS Code.
  const diffDocuments = new DiffDocuments();
  setDiffFiles(vscodeDiffFiles(diffDocuments));
  const shown = new ShownAgent(log);
  // Before the first conversation: they listen to every agent's notifications.
  const providers = new Providers(shown, log);
  const settings = new Settings(shown, log);
  const files = new Files(cwd, log);
  conversations = new Conversations(binary, cwd, version, files, shown, log);
  const history = new History(shown, conversations, cwd, log);
  const tabs = new ToolTabs();
  const view = new ChatViewProvider(context.extensionUri, conversations, providers, settings, history, files, tabs, log);

  context.subscriptions.push(
    log,
    tabs,
    vscode.workspace.registerTextDocumentContentProvider(ToolTabs.scheme, tabs),
    diffDocuments, // yusAgent (PR 3)
    vscode.workspace.registerTextDocumentContentProvider(DiffDocuments.scheme, diffDocuments),
    vscode.window.registerWebviewViewProvider(ChatViewProvider.viewId, view),
    // The button in the editor's title bar: the panel opens, its side bar with it.
    vscode.commands.registerCommand("yusagent.open", () => vscode.commands.executeCommand(`${ChatViewProvider.viewId}.focus`)),
  );
  // yusAgent : commandes des tests e2e seulement (variable posée par le lanceur), jamais déclarées.
  if (process.env.YUSAGENT_EXTENSION_TEST === "1") {
    const all = conversations;
    context.subscriptions.push(
      vscode.commands.registerCommand("yusagent._test.post", (message: WebviewMessage) => view.receive(message)),
      vscode.commands.registerCommand("yusagent._test.state", () => {
        const { agent, session, chat } = all.shown;
        return {
          agent: agent.current,
          session: session.current,
          chat: chat.current,
          // yusAgent (ticket 28) : processus de l'agent, fichier de la conversation, historique.
          pid: agent.pid,
          saved: session.saved,
          history: history.current,
          html: view.html,
          rendered: view.rendered,
          badge: view.badgeValue, // yusAgent (PR 3)
        };
      }),
      vscode.commands.registerCommand("yusagent._test.probe", () => view.probe()),
      // yusAgent (PR 3, étape 6) : arrêt de tous les agents (e2e vrai binaire, relevé des processus restants).
      vscode.commands.registerCommand("yusagent._test.stopAll", () => all.stopAll()),
      // yusAgent (PR 3) : saisie, focus, touches et clic dans la webview.
      vscode.commands.registerCommand("yusagent._test.probeInput", (input: ProbeInput) => view.probeInput(input)),
    );
  }
  conversations.start();
}

/** Every agent stops at once; one VS Code no longer waits for goes on alone. */
export async function deactivate(): Promise<void> {
  await conversations?.stopAll();
}

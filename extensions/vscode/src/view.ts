import { randomBytes } from "node:crypto";
import { posix } from "node:path";
import * as vscode from "vscode";
import type { Conversation, Conversations } from "./conversations";
import type { Files } from "./files";
import type { History } from "./history";
import { connectedModels } from "./instructions";
import type { HostMessage, Probe, WebviewMessage } from "./protocol";
import type { Providers } from "./providers";
import type { Settings } from "./settings";
import type { ToolTabs } from "./tabs";
// yusAgent (YA-10, ticket 24, PR I) : fonctions montrées selon les méthodes servies par le pont.
import { features } from "./ya/methods";

/**
 * The panel in the right side bar: hosts the React app from dist/webview. It
 * shows one conversation at a time; the others' changes wait until it shows
 * them again, whole.
 */
// Repris d'Unharnessed (MIT, 157dcdc1d, vscode/src/view.ts) ; adaptations yusAgent marquées.
export class ChatViewProvider implements vscode.WebviewViewProvider {
  static readonly viewId = "yusagent.chat";

  private view: vscode.WebviewView | undefined;
  /** yusAgent (tests e2e) : la webview a envoyé `ready`, et les sondes en attente. */
  rendered = false;
  private probes: ((probe: Probe | undefined) => void)[] = [];
  private splashPlayed = false;

  constructor(
    private readonly extensionUri: vscode.Uri,
    private readonly conversations: Conversations,
    private readonly providers: Providers,
    private readonly settings: Settings,
    private readonly history: History,
    private readonly files: Files,
    private readonly tabs: ToolTabs,
    private readonly log: vscode.LogOutputChannel,
  ) {
    conversations.onCreate((conversation) => this.follow(conversation));
    conversations.onShow(() => {
      const { agent, session, chat, kernel, draft } = this.conversations.shown;
      this.post({
        type: "conversation",
        agent: agent.current,
        session: session.current,
        chat: chat.current,
        kernel: kernel.current,
        draft,
      });
    });
    providers.onChange(() =>
      this.post({ type: "providers", providers: providers.providers, login: providers.signIn }),
    );
    settings.onChange(() => this.post({ type: "settings", settings: settings.current }));
    history.onChange(() => this.post({ type: "history", history: history.current }));
  }

  /** The conversation in the panel. */
  private get shown(): Conversation {
    return this.conversations.shown;
  }

  /** What a conversation sends the panel, while the panel shows it. */
  private follow(conversation: Conversation): void {
    const { agent, session, chat, kernel } = conversation;
    const shown = () => this.conversations.isShown(conversation);
    agent.onChange((state) => shown() && this.post({ type: "agent", agent: state }));
    session.onChange(() => shown() && this.post({ type: "session", session: session.current }));
    chat.onPost((message) => shown() && this.post(message));
    kernel.onChange(() => shown() && this.post({ type: "kernel", kernel: kernel.current }));
  }

  resolveWebviewView(view: vscode.WebviewView): void {
    const dist = vscode.Uri.joinPath(this.extensionUri, "dist", "webview");
    view.webview.options = { enableScripts: true, localResourceRoots: [dist] };
    view.webview.onDidReceiveMessage((message: WebviewMessage) => this.receive(message));
    view.onDidDispose(() => {
      if (this.view === view) this.view = undefined;
    });
    view.webview.html = renderHtml(view.webview, dist);
    this.view = view;
  }

  receive(message: WebviewMessage): void {
    switch (message.type) {
      case "probeResult":
        this.probes.shift()?.(message.probe);
        break;
      case "ready":
        this.rendered = true;
        this.log.info("webview ready");
        // A panel that comes back is not changing a waiting message.
        this.shown.chat.release();
        this.post({
          type: "init",
          // yusAgent : pas d'écran d'accueil (visuel plus tard).
          splash: false,
          features: features(),
          agent: this.shown.agent.current,
          providers: this.providers.providers,
          login: this.providers.signIn,
          session: this.shown.session.current,
          settings: this.settings.current,
          chat: this.shown.chat.current,
          history: this.history.current,
          kernel: this.shown.kernel.current,
        });
        this.splashPlayed = true;
        break;
      case "screen":
        this.log.info(`panel: ${message.screen}`);
        break;
      case "crashed":
        this.log.error(`panel: crashed: ${message.error}`);
        break;
      case "restartAgent":
        this.log.info("agent restart requested from the panel");
        void this.conversations.restart();
        break;
      case "setApiKey": {
        const { providerId } = message;
        void this.providers.setApiKey(providerId, message.apiKey).then((error) => {
          this.post({ type: "providerResult", action: "apiKey", providerId, error });
        });
        break;
      }
      case "logout": {
        const { providerId } = message;
        void this.providers.logout(providerId).then((error) => {
          this.post({ type: "providerResult", action: "logout", providerId, error });
        });
        break;
      }
      case "scanLocal":
        void this.providers.scanLocal().then((servers) => this.post({ type: "localScan", servers }));
        break;
      case "probeLocal": {
        const { baseUrl, apiKey } = message;
        void this.providers
          .probeLocal(baseUrl, apiKey)
          .then((probe) => this.post({ type: "localProbe", baseUrl, apiKey, probe }));
        break;
      }
      case "saveLocal": {
        const providerId = message.rowId;
        void this.providers.saveLocal(message.server).then((error) => {
          this.post({ type: "providerResult", action: "local", providerId, error });
        });
        break;
      }
      case "removeLocal": {
        const { providerId } = message;
        void this.providers.removeLocal(providerId).then((error) => {
          this.post({ type: "providerResult", action: "local", providerId, error });
        });
        break;
      }
      case "login": {
        // Try again after a failed MCP sign-in: the same server, by the same name.
        const { providerId } = message;
        if (providerId.startsWith("mcp:")) this.mcpSignIn(providerId.slice(4), this.providers.signIn?.name);
        else void this.providers.startLogin(providerId);
        break;
      }
      case "loginAnswer":
        void this.providers.answer(message.promptId, message.answer);
        break;
      case "loginOpen":
        void this.providers.openPage();
        break;
      case "loginCancel":
        this.providers.cancelLogin();
        break;
      case "loginClose":
        this.providers.closeLogin();
        break;
      case "setConfig": {
        const { type: _, ...change } = message;
        void this.shown.session.setOption(change);
        break;
      }
      case "setSetting": {
        const { key } = message;
        void this.settings.set(key, message.value).then((error) => {
          this.post({ type: "settingResult", key, error });
        });
        break;
      }
      case "setInstructions": {
        const session = this.shown.session.current;
        const choices = session?.kind === "open" ? session.config.model?.choices : undefined;
        const connected = choices ? new Set(connectedModels(choices, this.providers.providers ?? []).keys()) : null;
        void this.settings
          .setInstructions(message.index, message.group, connected)
          .then((error) => this.post({ type: "settingResult", key: "modelInstructions", error }));
        break;
      }
      case "mcpSignIn":
        this.mcpSignIn(message.id, message.label);
        break;
      case "mcpSetToken": {
        const key = `mcp:${message.id}`;
        void this.settings
          .setToken(message.id, message.token)
          .then((error) => this.post({ type: "settingResult", key, error }));
        break;
      }
      case "mcpSignOut": {
        const key = `mcp:${message.id}`;
        void this.settings.signOut(message.id).then((error) => this.post({ type: "settingResult", key, error }));
        break;
      }
      case "setSkill": {
        const key = `skill:${message.path}`;
        void this.settings
          .setSkill(message.path, message.enabled)
          .then((error) => this.post({ type: "settingResult", key, error }));
        break;
      }
      case "createSkill":
        void this.settings.createSkill(message.name, message.description).then(({ created, error }) => {
          this.post({ type: "settingResult", key: "skills/create", error });
          if (created) void this.open(created);
        });
        break;
      case "skillFolder":
        void this.settings
          .skillFolder(message.path, message.add)
          .then((error) => this.post({ type: "settingResult", key: "skills/folder", error }));
        break;
      case "openFile":
        // Only a skill on the list: the panel never names any other file.
        if (this.settings.skillFiles.includes(message.path)) void this.open(message.path);
        break;
      case "copy":
        void vscode.env.clipboard.writeText(message.text);
        break;
      case "openCell":
        void this.openTool(message.id, message.part);
        break;
      case "send":
        this.shown.chat.send(message.text, message.images, message.edit, message.now);
        break;
      case "edit":
        this.shown.chat.edit(message.id);
        break;
      case "release":
        this.shown.chat.release(message.id);
        break;
      case "listFiles":
        void this.files.list().then(({ files, open }) => this.post({ type: "files", files, open }));
        break;
      case "readImages":
        void this.readImages(message.uris);
        break;
      case "cancel":
        this.shown.chat.cancel();
        break;
      case "goal":
        if (message.action === "pause") this.shown.chat.pauseGoal();
        else if (message.action === "resume") this.shown.chat.resumeGoal();
        else this.shown.chat.endGoal();
        break;
      case "unqueue":
        this.shown.chat.unqueue(message.id);
        break;
      case "moveQueued":
        this.shown.chat.move(message.id, message.to);
        break;
      case "sendNow":
        this.shown.chat.sendNow(message.id);
        break;
      case "resumeQueue":
        this.shown.chat.resume();
        break;
      case "newChat":
        // A waiting message being changed stays as it was.
        this.shown.chat.release();
        this.conversations.newChat(message.draft);
        break;
      case "listChats":
        void this.history.list();
        break;
      case "openChat": {
        // The conversation in the panel is already there.
        const saved = this.history.saved(message.id);
        if (!saved) break;
        this.shown.chat.release();
        this.conversations.openSaved(saved, message.draft);
        break;
      }
      case "deleteChat": {
        // Its agent, if it is open, holds the file.
        const { id } = message;
        void this.conversations.release(id).then(() => this.history.remove(id));
        break;
      }
      case "openLink":
        // Web pages only: a message never opens anything else.
        if (/^https?:\/\//i.test(message.url)) void vscode.env.openExternal(vscode.Uri.parse(message.url));
        break;
      case "kernelTail": {
        const { id } = message;
        void this.shown.kernel.tail(id).then((tail) => this.post({ type: "kernelTail", id, tail }));
        break;
      }
      case "kernelKill":
        void this.shown.kernel.kill(message.id);
        break;
      case "kernelDismiss":
        this.shown.kernel.dismiss(message.id);
        break;
      case "kernelPreview": {
        const { name } = message;
        void this.shown.kernel.preview(name).then((preview) => this.post({ type: "kernelPreview", name, preview }));
        break;
      }
    }
  }

  /** An MCP server's sign-in runs on the sign-in screen, like a provider's. */
  private mcpSignIn(id: string, label: string | undefined): void {
    void this.providers.startLogin(`mcp:${id}`, label).then((done) => {
      if (done) this.settings.signedIn(id);
    });
  }

  /** A tool's code or whole output, in its own tab. */
  private async openTool(id: string, part: "input" | "output"): Promise<void> {
    const found = this.shown.chat.toolPart(id, part);
    if (!found) return;
    const name = part === "output" ? "output.txt" : found.python ? "cell.py" : "input.json";
    try {
      const document = await this.tabs.open(id, name, found.text);
      this.log.info(`opened tool ${id} ${part}: ${document.getText().length} chars`);
    } catch (error) {
      this.log.warn(`could not open tool ${id} ${part}: ${String(error)}`);
    }
  }

  private async open(path: string): Promise<void> {
    try {
      await vscode.window.showTextDocument(vscode.Uri.file(path), { preview: false });
      this.log.info(`opened ${path}`);
    } catch (error) {
      this.log.warn(`could not open ${path}: ${String(error)}`);
    }
  }

  /** The images among files dropped from VS Code, for the message box: the panel can't read a file. */
  private async readImages(uris: string[]): Promise<void> {
    const images: { name: string; mimeType: string; data: string }[] = [];
    const errors: string[] = [];
    for (const text of uris) {
      const uri = vscode.Uri.parse(text);
      const name = posix.basename(uri.path);
      const mimeType = IMAGE_MIMES[posix.extname(name).toLowerCase()];
      if (!mimeType) continue;
      try {
        images.push({ name, mimeType, data: Buffer.from(await vscode.workspace.fs.readFile(uri)).toString("base64") });
      } catch (error) {
        this.log.warn(`could not read dropped ${uri.toString(true)}: ${String(error)}`);
        errors.push(`${name} could not be read.`);
      }
    }
    this.log.info(`dropped ${uris.length} files, images read: ${images.map(({ name }) => name).join(", ") || "none"}`);
    this.post({ type: "droppedImages", images, errors });
  }

  /** yusAgent (tests e2e) : la page de la vue. */
  get html(): string {
    return this.view?.webview.html ?? "";
  }

  /** yusAgent (tests e2e) : ce que la webview affiche ; `undefined` sans réponse en 1 s. */
  probe(): Promise<Probe | undefined> {
    if (!this.view) return Promise.resolve(undefined);
    return new Promise((resolve) => {
      const answer = (probe: Probe | undefined) => {
        clearTimeout(timer);
        resolve(probe);
      };
      const timer = setTimeout(() => {
        const at = this.probes.indexOf(answer);
        if (at >= 0) this.probes.splice(at, 1);
        resolve(undefined);
      }, 1_000);
      this.probes.push(answer);
      this.post({ type: "probe" });
    });
  }

  private post(message: HostMessage): void {
    void this.view?.webview.postMessage(message);
  }
}

/** The images the providers read, by extension. */
const IMAGE_MIMES: Record<string, string> = {
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".gif": "image/gif",
  ".webp": "image/webp",
};

/** Waiting messages back in a message box: they come first, they were written before. */
function renderHtml(webview: vscode.Webview, dist: vscode.Uri): string {
  const nonce = randomBytes(16).toString("base64");
  const asset = (name: string) => webview.asWebviewUri(vscode.Uri.joinPath(dist, name)).toString();
  // yusAgent (E3) : CSP stricte, ni `'unsafe-inline'` ni `data:` (Unharnessed les autorise).
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
  <link rel="stylesheet" href="${asset("main.css")}">
</head>
<body>
  <div id="root"></div>
  <script nonce="${nonce}" src="${asset("main.js")}"></script>
</body>
</html>`;
}

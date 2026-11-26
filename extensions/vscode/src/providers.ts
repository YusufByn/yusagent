import * as vscode from "vscode";
import { errorText } from "./agent";
import type { LocalProbe, LocalSave, LocalServer, LoginAnswer, LoginState, LoginUpdate, Provider } from "./protocol";
import type { ShownAgent } from "./shown";

// The agent's `_yus/providers/*` extension methods: Unharnessed's
// (crates/uh-daemon/src/acp/providers.rs) under the yusAgent prefix (ticket 32), served by the
// bridge since ticket 40 (R5), but `local_*` (screen masked, decision 6).
const LIST = "_yus/providers/list";
const RELOAD = "_yus/providers/reload";
const SET_API_KEY = "_yus/providers/set_api_key";
const LOGOUT = "_yus/providers/logout";
const LOGIN = "_yus/providers/login";
const LOGIN_ANSWER = "_yus/providers/login_answer";
const LOGIN_OPEN = "_yus/providers/login_open";
const LOGIN_CANCEL = "_yus/providers/login_cancel";
const LOGIN_UPDATE = "_yus/providers/login_update";
const LOCAL_SCAN = "_yus/providers/local_scan";
const LOCAL_PROBE = "_yus/providers/local_probe";
const LOCAL_SAVE = "_yus/providers/local_save";
const LOCAL_REMOVE = "_yus/providers/local_remove";

/** What `set_api_key`, `logout` and `login` answer. */
type Settled = {
  providers: Provider[];
  outcome: "done" | "failed" | "cancelled";
  /** The status line, or the error to show. */
  message?: string;
};

/**
 * The agent's providers and the sign-in in progress. They live here, not in
 * the webview, so a panel that is hidden and shown again finds them. Every
 * agent reads the same credentials: the list goes through the one in the
 * panel, and after a change there, the others read them again.
 */
export class Providers {
  private list: Provider[] | null = null;
  private login: LoginState | null = null;
  /** Bumped per sign-in, so a late answer never lands on the next one. */
  private loginRun = 0;
  private readonly listeners = new Set<() => void>();

  constructor(
    private readonly agent: ShownAgent,
    private readonly log: vscode.LogOutputChannel,
  ) {
    agent.onNotification(LOGIN_UPDATE, (params) => this.onUpdate(params as { providerId: string; update: LoginUpdate }));
    agent.onChange((state) => {
      // A sign-in runs in its process: it ends with it.
      if (state.kind !== "ready") {
        this.login = null;
        this.loginRun++;
      }
      this.changed();
      if (state.kind === "ready") void this.refresh();
    });
  }

  get providers(): Provider[] | null {
    return this.list;
  }

  get signIn(): LoginState | null {
    return this.login;
  }

  onChange(listener: () => void): { dispose(): void } {
    this.listeners.add(listener);
    return { dispose: () => this.listeners.delete(listener) };
  }

  async refresh(): Promise<void> {
    try {
      const { providers } = await this.agent.request<{ providers: Provider[] }>(LIST);
      this.list = providers;
      this.log.info(`providers: ${connectedNames(providers)}`);
      this.changed();
    } catch (error) {
      this.log.warn(`providers: list failed: ${errorText(error)}`);
    }
  }

  /** Resolves the error to show on the row, if any. */
  setApiKey(providerId: string, apiKey: string): Promise<string | undefined> {
    return this.settle(`API key for ${providerId}`, SET_API_KEY, { providerId, apiKey });
  }

  logout(providerId: string): Promise<string | undefined> {
    return this.settle(`logout of ${providerId}`, LOGOUT, { providerId });
  }

  /** The local servers at their usual addresses; `null` when the agent could not look. */
  async scanLocal(): Promise<LocalServer[] | null> {
    try {
      const { servers } = await this.agent.request<{ servers: LocalServer[] }>(LOCAL_SCAN);
      const found = servers.map((server) => `${server.name} at ${server.baseUrl} (${count(server.models.length)})`);
      this.log.info(`local: scan found ${found.join(", ") || "no server"}`);
      return servers;
    } catch (error) {
      this.log.warn(`local: scan failed: ${errorText(error)}`);
      return null;
    }
  }

  async probeLocal(baseUrl: string, apiKey: string): Promise<LocalProbe> {
    try {
      const probe = await this.agent.request<LocalProbe>(LOCAL_PROBE, { baseUrl, ...(apiKey ? { apiKey } : {}) });
      if ("server" in probe) {
        const { server } = probe;
        const models = server.models.map((model) => model.id).join(", ");
        this.log.info(`local: ${baseUrl} is ${server.name} at ${server.baseUrl}, ${count(server.models.length)}${models ? `: ${models}` : ""}`);
      } else {
        this.log.info(`local: ${baseUrl}: ${probe.error.kind} (${probe.error.message})`);
      }
      return probe;
    } catch (error) {
      this.log.warn(`local: probe of ${baseUrl} failed: ${errorText(error)}`);
      return { error: { kind: "invalid", message: errorText(error) } };
    }
  }

  saveLocal(server: LocalSave): Promise<string | undefined> {
    return this.settle(`local server ${server.providerId ?? server.name}`, LOCAL_SAVE, server);
  }

  removeLocal(providerId: string): Promise<string | undefined> {
    return this.settle(`removal of local server ${providerId}`, LOCAL_REMOVE, { providerId });
  }

  /**
   * `name` names what is signed in to when it is not a provider (an MCP
   * server, `mcp:<id>`). Resolves `true` once the sign-in is done.
   */
  async startLogin(providerId: string, name?: string): Promise<boolean> {
    if (this.login && !this.login.error) return false;
    const run = ++this.loginRun;
    name ??= this.list?.find((p) => p.id === providerId && p.login === "browser")?.name ?? providerId;
    this.setLogin({ providerId, name });
    this.log.info(`sign-in to ${providerId}: started`);
    let error: string | undefined;
    let done = false;
    try {
      const result = await this.agent.request<Settled>(LOGIN, { providerId });
      this.log.info(`sign-in to ${providerId}: ${result.outcome}${result.message ? ` (${result.message})` : ""}`);
      if (run !== this.loginRun) return false;
      this.list = result.providers;
      if (result.outcome === "failed") error = result.message ?? "Sign-in failed.";
      done = result.outcome === "done";
      if (done) this.agent.broadcast(RELOAD);
    } catch (failure) {
      this.log.warn(`sign-in to ${providerId} failed: ${errorText(failure)}`);
      if (run !== this.loginRun) return false;
      error = errorText(failure);
    }
    this.setLogin(error === undefined ? null : { providerId, name, error });
    return done;
  }

  async answer(promptId: number, answer: LoginAnswer): Promise<void> {
    const login = this.login;
    if (!login || login.error) return;
    // The prompt leaves at once; the flow's next step replaces it.
    const { prompt, team } = login;
    this.setLogin({ ...login, prompt: undefined, team: undefined });
    try {
      await this.agent.request(LOGIN_ANSWER, { promptId, answer });
    } catch (error) {
      this.log.warn(`sign-in answer refused: ${errorText(error)}`);
      const current = this.login;
      if (current?.providerId === login.providerId && !current.prompt && !current.team) {
        this.setLogin({ ...current, prompt, team });
      }
    }
  }

  /**
   * The agent opens the page on its machine, which is this one unless VS Code
   * runs remote; then VS Code opens it on the user's machine (it may first
   * ask whether to trust the site).
   */
  async openPage(): Promise<void> {
    const url = this.login?.url?.url;
    if (!url) return;
    if (vscode.env.remoteName) {
      await vscode.env.openExternal(vscode.Uri.parse(url));
      return;
    }
    try {
      await this.agent.request(LOGIN_OPEN);
    } catch (error) {
      this.log.warn(`sign-in page not opened: ${errorText(error)}`);
    }
  }

  cancelLogin(): void {
    if (!this.login || this.login.error) return;
    this.log.info(`sign-in to ${this.login.providerId}: cancel requested`);
    this.agent.notify(LOGIN_CANCEL);
  }

  /** Leaves a failed sign-in's screen. */
  closeLogin(): void {
    if (this.login?.error) this.setLogin(null);
  }

  private onUpdate({ providerId, update }: { providerId: string; update: LoginUpdate }): void {
    const login = this.login;
    if (!login || login.error || login.providerId !== providerId) return;
    switch (update.kind) {
      case "url":
        this.setLogin({ ...login, url: { url: update.url, instructions: update.instructions } });
        break;
      case "progress":
      case "waiting":
        this.setLogin({ ...login, status: { message: update.message, waiting: update.kind === "waiting" } });
        break;
      case "prompt":
        this.setLogin({ ...login, prompt: update, team: undefined });
        break;
      case "team":
        this.setLogin({ ...login, team: update, prompt: undefined });
        break;
    }
  }

  private async settle(what: string, method: string, params: Record<string, unknown>): Promise<string | undefined> {
    try {
      const result = await this.agent.request<Settled>(method, params);
      this.log.info(`${what}: ${result.outcome}${result.message ? ` (${result.message})` : ""}`);
      this.list = result.providers;
      this.changed();
      if (result.outcome === "done") this.agent.broadcast(RELOAD);
      return result.outcome === "failed" ? (result.message ?? "Failed.") : undefined;
    } catch (error) {
      this.log.warn(`${what} failed: ${errorText(error)}`);
      return errorText(error);
    }
  }

  private setLogin(login: LoginState | null): void {
    this.login = login;
    this.changed();
  }

  private changed(): void {
    for (const listener of this.listeners) listener();
  }
}

function count(models: number): string {
  return `${models} model${models === 1 ? "" : "s"}`;
}

function connectedNames(providers: Provider[]): string {
  const connected = providers.filter((p) => p.connected).map((p) => `${p.id} (${p.method})`);
  return `${providers.length} listed, connected: ${connected.join(", ") || "none"}`;
}

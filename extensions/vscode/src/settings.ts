import type * as vscode from "vscode";
import { errorText } from "./agent";
import type { McpServer, SettingsState, SettingsView, SkillsView } from "./protocol";
import type { ShownAgent } from "./shown";

// yusAgent (YA-10, ticket 31, X3) : the bridge's `_yus/settings/*`, `_yus/mcp/*` and `_yus/skills/*`
// methods (crates/pa-daemon/src/acp/ya_resources.rs), with the answers of the code taken from
// Unharnessed. They are read when the settings screen opens, never when the agent starts: nothing
// is sent at the opening of a chat (E7, the frames fixture).
const GET = "_yus/settings/get";
const SET = "_yus/settings/set";
const MCP_LIST = "_yus/mcp/list";
const MCP_SET_TOKEN = "_yus/mcp/set_token";
const MCP_LOGOUT = "_yus/mcp/logout";
const SKILLS_LIST = "_yus/skills/list";
const SKILLS_SET = "_yus/skills/set";
const SKILLS_CREATE = "_yus/skills/create";
const SKILLS_ADD_FOLDER = "_yus/skills/add_folder";
const SKILLS_REMOVE_FOLDER = "_yus/skills/remove_folder";

/**
 * The agent's global settings, its MCP servers and its skills, for the
 * settings screen: read when the screen opens, again once a new agent process
 * is ready while it is open. yusAgent (ticket 31) : nothing waits for a
 * restart; a change reaches the agent with the next message (the engine
 * announces skills and MCP changes; the screen's settings are read at each
 * use).
 */
export class Settings {
  private view: SettingsView | null = null;
  private mcp: McpServer[] | null = null;
  private skills: SkillsView | null = null;
  /** Bumped per agent process, so a late answer never lands on the next one. */
  private run = 0;
  /** yusAgent (ticket 31) : the settings screen is on display: the agent is read then, not at its start. */
  private onScreen = false;
  private readonly listeners = new Set<() => void>();

  constructor(
    private readonly agent: ShownAgent,
    private readonly log: vscode.LogOutputChannel,
  ) {
    agent.onChange((state) => {
      this.run++;
      this.changed();
      if (state.kind === "ready" && this.onScreen) void this.refresh();
    });
  }

  /** yusAgent (ticket 31) : the panel shows the settings screen, or leaves it; opening it reads the agent. */
  screen(open: boolean): void {
    const opening = open && !this.onScreen;
    this.onScreen = open;
    if (opening && this.agent.current.kind === "ready") void this.refresh();
  }

  get current(): SettingsState {
    return { view: this.view, mcp: this.mcp, skills: this.skills };
  }

  /** The skill files on the list: the only files the panel may open. */
  get skillFiles(): string[] {
    return this.skills?.skills.map((skill) => skill.path) ?? [];
  }

  onChange(listener: () => void): { dispose(): void } {
    this.listeners.add(listener);
    return { dispose: () => this.listeners.delete(listener) };
  }

  async refresh(): Promise<void> {
    await Promise.all([this.read(), this.refreshMcp(), this.ask(SKILLS_LIST, {}, "skills: read")]);
  }

  /** Resolves the error to show under the setting, if any. */
  async set(key: string, value: unknown): Promise<string | undefined> {
    const run = this.run;
    try {
      const view = await this.agent.request<SettingsView>(SET, { key, value });
      // yusAgent (ticket 31, règle 9) : la clé seule pour `mcpServers`, dont une entrée peut porter
      // un en-tête ou une variable d'environnement secrète.
      const shown = key === "mcpServers" ? "set" : `set to ${JSON.stringify(value)}`;
      this.log.info(`settings: ${key} ${value === null ? "reset" : shown}`);
      if (run !== this.run) return undefined;
      this.view = view;
      this.changed();
      // A server added, changed or removed changes the list.
      if (key === "mcpServers") void this.refreshMcp();
      return undefined;
    } catch (error) {
      this.log.warn(`settings: ${key} refused: ${errorText(error)}`);
      return errorText(error);
    }
  }

  async refreshMcp(): Promise<void> {
    const run = this.run;
    try {
      const { servers } = await this.agent.request<{ servers: McpServer[] }>(MCP_LIST);
      if (run !== this.run) return;
      this.mcp = servers;
      // Yours by name; the Marketplace, the catalog's other services, by count.
      const yours = servers.filter((server) => server.source === "user" || server.signedIn);
      const listed = yours.map((server) => `${server.id} (${server.status})`).join(", ") || "none";
      this.log.info(`mcp: ${listed}; ${servers.length - yours.length} in the Marketplace`);
      this.changed();
    } catch (error) {
      this.log.warn(`mcp: list failed: ${errorText(error)}`);
    }
  }

  /** After a sign-in to an MCP server: the list shows it. */
  signedIn(id: string): void {
    this.log.info(`mcp: signed in to ${id}`);
    void this.refreshMcp();
  }

  /** Add a catalog service by its token: the list shows it. */
  async setToken(id: string, token: string): Promise<string | undefined> {
    const run = this.run;
    try {
      const { servers } = await this.agent.request<{ servers: McpServer[] }>(MCP_SET_TOKEN, { id, token });
      this.log.info(`mcp: token stored for ${id}`);
      if (run !== this.run) return undefined;
      this.mcp = servers;
      this.changed();
      return undefined;
    } catch (error) {
      this.log.warn(`mcp: token for ${id} refused: ${errorText(error)}`);
      return errorText(error);
    }
  }

  async signOut(id: string): Promise<string | undefined> {
    const run = this.run;
    try {
      const { servers } = await this.agent.request<{ servers: McpServer[] }>(MCP_LOGOUT, { id });
      this.log.info(`mcp: signed out of ${id}`);
      if (run !== this.run) return undefined;
      this.mcp = servers;
      this.changed();
      return undefined;
    } catch (error) {
      this.log.warn(`mcp: sign-out of ${id} refused: ${errorText(error)}`);
      return errorText(error);
    }
  }

  setSkill(path: string, enabled: boolean): Promise<string | undefined> {
    return this.ask(SKILLS_SET, { path, enabled }, `skills: ${path} turned ${enabled ? "on" : "off"}`);
  }

  /** Resolves the new file, or the error to show. */
  async createSkill(name: string, description: string): Promise<{ created?: string; error?: string }> {
    let created: string | undefined;
    const error = await this.ask(SKILLS_CREATE, { name, description }, `skills: ${name} created`, (answer) => {
      created = answer.created;
    });
    return { created, error };
  }

  skillFolder(path: string, add: boolean): Promise<string | undefined> {
    return add
      ? this.ask(SKILLS_ADD_FOLDER, { path }, `skills: folder ${path} added`)
      : this.ask(SKILLS_REMOVE_FOLDER, { path }, `skills: folder ${path} removed`);
  }

  private async read(): Promise<void> {
    const run = this.run;
    try {
      const view = await this.agent.request<SettingsView>(GET);
      if (run !== this.run) return;
      this.view = view;
      this.log.info(`settings: read, ${Object.values(view.values).filter((v) => v !== null).length} set`);
      this.changed();
    } catch (error) {
      this.log.warn(`settings: read failed: ${errorText(error)}`);
    }
  }

  /** A skills method: every one answers the list. Resolves the error to show, if any. */
  private async ask(
    method: string,
    params: Record<string, unknown>,
    done: string,
    then?: (answer: SkillsView & { created?: string }) => void,
  ): Promise<string | undefined> {
    const run = this.run;
    try {
      const answer = await this.agent.request<SkillsView & { created?: string }>(method, params);
      const on = answer.skills.filter((skill) => skill.enabled).length;
      this.log.info(`${done}, ${on} of ${answer.skills.length} on`);
      then?.(answer);
      if (run !== this.run) return undefined;
      const { created: _, ...skills } = answer;
      this.skills = skills;
      this.changed();
      return undefined;
    } catch (error) {
      this.log.warn(`${done.split(":")[0]}: ${method} refused: ${errorText(error)}`);
      return errorText(error);
    }
  }

  private changed(): void {
    for (const listener of this.listeners) listener();
  }
}

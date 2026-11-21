import type * as vscode from "vscode";
import { errorText } from "./agent";
import { savedGroups, withGroup } from "./instructions";
import type { McpServer, ModelInstructions, SettingsState, SettingsView, SkillsView } from "./protocol";
import type { ShownAgent } from "./shown";

// The agent's `_unharnessed/settings/*`, `mcp/*` and `skills/*` extension
// methods (crates/uh-daemon/src/acp/{settings,mcp_servers,skills}.rs).
const GET = "_unharnessed/settings/get";
const SET = "_unharnessed/settings/set";
const MCP_LIST = "_unharnessed/mcp/list";
const MCP_SET_TOKEN = "_unharnessed/mcp/set_token";
const MCP_LOGOUT = "_unharnessed/mcp/logout";
const SKILLS_LIST = "_unharnessed/skills/list";
const SKILLS_SET = "_unharnessed/skills/set";
const SKILLS_CREATE = "_unharnessed/skills/create";
const SKILLS_ADD_FOLDER = "_unharnessed/skills/add_folder";
const SKILLS_REMOVE_FOLDER = "_unharnessed/skills/remove_folder";

/**
 * The agent's global settings, its MCP servers and its skills, for the
 * settings screen. They outlive an agent restart, so the screen stays put
 * while the new process starts; they are read again once it is ready, and
 * when the panel shows another conversation. What a restart would apply is
 * each process's own: what it started with.
 */
export class Settings {
  private view: SettingsView | null = null;
  private mcp: McpServer[] | null = null;
  private skills: SkillsView | null = null;
  /** Each process's restart keys when it started, and the MCP sign-ins and sign-outs before it. */
  private readonly started = new WeakMap<object, { values: Record<string, unknown>; mcp: number }>();
  /** MCP sign-ins and sign-outs: an agent keeps the servers it started with. */
  private mcpChanges = 0;
  /** Bumped per agent process, so a late answer never lands on the next one. */
  private run = 0;
  /** What a restart would apply, as last logged. */
  private logged = "";
  private readonly listeners = new Set<() => void>();

  constructor(
    private readonly agent: ShownAgent,
    private readonly log: vscode.LogOutputChannel,
  ) {
    agent.onChange((state) => {
      this.run++;
      // A process starting again starts with what the files hold now.
      if (state.kind === "starting" && agent.process) this.started.delete(agent.process);
      this.changed();
      if (state.kind === "ready") void this.refresh();
    });
  }

  get current(): SettingsState {
    return { view: this.view, pending: this.pending(), mcp: this.mcp, skills: this.skills };
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
      this.log.info(`settings: ${key} ${value === null ? "reset" : `set to ${JSON.stringify(value)}`}`);
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

  /**
   * Save the group of instructions at `index` (`null`: a new one), or
   * delete it (`group` null). `connected`: the models the panel shows; the
   * group keeps its others, whose provider is not connected now. Resolves
   * the error to show, if any.
   */
  async setInstructions(
    index: number | null,
    group: ModelInstructions | null,
    connected: ReadonlySet<string> | null,
  ): Promise<string | undefined> {
    const refuse = (reason: string) => {
      this.log.warn(`settings: modelInstructions refused: ${reason}`);
      return reason;
    };
    // Written over what the file holds: it must be read first.
    if (!this.view) return refuse("The settings are not read yet.");
    const groups = savedGroups(this.view);
    if (index !== null && !groups[index]) return refuse("This group changed meanwhile; open it again.");
    if (group === null) {
      if (index === null) return undefined;
      const rest = groups.filter((_, at) => at !== index);
      return this.set("modelInstructions", rest.length > 0 ? rest : null);
    }
    if (!connected) return refuse("The models show once the session is open.");
    const checked = [...new Set(group.models)].filter((model) => connected.has(model));
    const text = group.text.trim();
    if (checked.length === 0) return refuse("Check at least one model.");
    if (!text) return refuse("Write the instructions.");
    return this.set("modelInstructions", withGroup(groups, index, checked, text, connected));
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

  /** After a sign-in to an MCP server: the list shows it, a restart applies it. */
  signedIn(id: string): void {
    this.log.info(`mcp: signed in to ${id}`);
    this.mcpChanges++;
    void this.refreshMcp();
  }

  /** Add a Marketplace service by its token: the list shows it, a restart applies it. */
  async setToken(id: string, token: string): Promise<string | undefined> {
    const run = this.run;
    try {
      const { servers } = await this.agent.request<{ servers: McpServer[] }>(MCP_SET_TOKEN, { id, token });
      this.log.info(`mcp: token stored for ${id}`);
      if (run !== this.run) return undefined;
      this.mcp = servers;
      this.mcpChanges++;
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
      this.mcpChanges++;
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
    const process = this.agent.process;
    try {
      const view = await this.agent.request<SettingsView>(GET);
      if (run !== this.run) return;
      this.view = view;
      if (process && !this.started.has(process)) this.started.set(process, { values: pick(view), mcp: this.mcpChanges });
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

  /** What a restart of the agent in the panel would apply. */
  private pending(): string[] {
    const view = this.view;
    const process = this.agent.process;
    const started = process && this.started.get(process);
    const keys =
      view && started
        ? view.restart.filter((key) => JSON.stringify(view.values[key]) !== JSON.stringify(started.values[key]))
        : [];
    if (this.skills?.restart) keys.push("skills");
    if (started && this.mcpChanges > started.mcp) keys.push("mcp");
    return keys;
  }

  private changed(): void {
    const pending = this.pending().join(", ");
    if (pending !== this.logged) {
      this.logged = pending;
      this.log.info(`settings: a restart would apply ${pending || "nothing"}`);
    }
    for (const listener of this.listeners) listener();
  }
}

/** The restart keys' values. */
function pick(view: SettingsView): Record<string, unknown> {
  return Object.fromEntries(view.restart.map((key) => [key, view.values[key]]));
}

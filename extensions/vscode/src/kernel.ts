import type * as vscode from "vscode";
import { errorText, type Agent } from "./agent";
import type { KernelCommand, KernelState, KernelVariable, VariablePreview } from "./protocol";
import type { Session } from "./session";

// The agent's `_unharnessed/kernel/*` (crates/uh-daemon/src/acp/kernel.rs).
const CHANGED = "_unharnessed/kernel/changed";
const TAIL = "_unharnessed/kernel/tail";
const KILL = "_unharnessed/kernel/kill";
const PREVIEW = "_unharnessed/kernel/preview";
/** A command's lines the panel shows. */
const TAIL_LINES = 7;

/**
 * The agent's kernel, as the panel shows it over the message box: the
 * commands it left running in the background, and the variables its cells
 * made. The agent sends each list again when it changes; a finished command
 * stays until the panel lets it go. Both empty with each agent process.
 */
export class Kernel {
  private commands: KernelCommand[] = [];
  private variables: KernelVariable[] = [];
  /** Finished commands the panel let go. */
  private readonly dismissed = new Set<string>();
  private readonly listeners = new Set<() => void>();

  constructor(
    private readonly agent: Agent,
    private readonly session: Session,
    private readonly log: vscode.LogOutputChannel,
  ) {
    agent.onNotification(CHANGED, (params) => this.onChanged(params));
    agent.onChange(() => {
      this.commands = [];
      this.variables = [];
      this.dismissed.clear();
      this.changed();
    });
  }

  get current(): KernelState {
    return { commands: this.commands.filter((command) => !this.dismissed.has(command.id)), variables: this.variables };
  }

  onChange(listener: () => void): { dispose(): void } {
    this.listeners.add(listener);
    return { dispose: () => this.listeners.delete(listener) };
  }

  /** A command the agent left running in the background has not ended. */
  get running(): boolean {
    return this.commands.some((command) => command.status === "running");
  }

  async tail(id: string): Promise<string | null> {
    const sessionId = this.session.sessionId;
    if (!sessionId) return null;
    try {
      const { tail } = await this.agent.request<{ tail: string }>(TAIL, { sessionId, id, lines: TAIL_LINES });
      return tail;
    } catch (error) {
      this.log.warn(`kernel: no tail for ${id}: ${errorText(error)}`);
      return null;
    }
  }

  async kill(id: string): Promise<void> {
    const sessionId = this.session.sessionId;
    if (!sessionId) return;
    try {
      const { killed } = await this.agent.request<{ killed: boolean }>(KILL, { sessionId, id });
      this.log.info(`kernel: ${id} ${killed ? "stopped" : "had already ended"}`);
    } catch (error) {
      this.log.warn(`kernel: ${id} not stopped: ${errorText(error)}`);
    }
  }

  dismiss(id: string): void {
    if (!this.commands.some((command) => command.id === id && command.status === "finished")) return;
    this.dismissed.add(id);
    this.changed();
  }

  async preview(name: string): Promise<VariablePreview> {
    const sessionId = this.session.sessionId;
    if (!sessionId) return { error: "The agent is not running" };
    try {
      const { preview, style, truncated } = await this.agent.request<{
        preview: string;
        style: "code" | "text";
        truncated: boolean;
      }>(PREVIEW, { sessionId, name });
      this.log.info(`kernel: preview of ${name} (${style}, ${preview.length} chars${truncated ? ", cut" : ""})`);
      return { text: preview, style, truncated };
    } catch (error) {
      this.log.info(`kernel: no preview of ${name}: ${errorText(error)}`);
      return { error: errorText(error) };
    }
  }

  private onChanged(params: unknown): void {
    const { sessionId, commands, variables } = params as {
      sessionId: string;
      commands?: KernelCommand[];
      variables?: KernelVariable[];
    };
    if (sessionId !== this.session.updatesFrom) return;
    if (commands) {
      this.describe(commands);
      this.commands = commands;
      // A command the agent no longer lists cannot come back.
      for (const id of this.dismissed) if (!commands.some((command) => command.id === id)) this.dismissed.delete(id);
    }
    if (variables) {
      const names = variables.map((variable) => variable.name);
      const shown = names.length > 8 ? [...names.slice(0, 8), "…"] : names;
      this.log.info(`kernel: ${names.length} variables${names.length ? `: ${shown.join(", ")}` : ""}`);
      this.variables = variables;
    }
    this.changed();
  }

  /** For the log: what started, what it listens on, how it ended. */
  private describe(commands: KernelCommand[]): void {
    for (const command of commands) {
      const before = this.commands.find((known) => known.id === command.id);
      if (!before) this.log.info(`kernel: command ${command.id} ${command.status}: ${command.command}`);
      if (command.ports.join() !== (before?.ports.join() ?? "") && command.ports.length) {
        this.log.info(`kernel: command ${command.id} listens on ${command.ports.map((port) => `:${port}`).join(" ")}`);
      }
      if (before?.status === "running" && command.status === "finished") {
        const how = command.killed ? "stopped" : `exit ${command.exitCode}`;
        this.log.info(`kernel: command ${command.id} ended (${how})`);
      }
    }
  }

  private changed(): void {
    for (const listener of this.listeners) listener();
  }
}

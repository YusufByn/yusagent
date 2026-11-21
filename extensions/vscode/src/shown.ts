import type * as vscode from "vscode";
import { errorText, type Agent } from "./agent";
import type { AgentState } from "./protocol";

/**
 * The agent of the conversation in the panel, for what all conversations
 * share (the providers, the settings, the history): they ask it, and follow
 * it when the panel shows another conversation. Every agent's notifications
 * reach them, so a sign-in hears from the agent that runs it.
 */
export class ShownAgent {
  private agent: Agent | undefined;
  private watch: { dispose(): void } | undefined;
  private readonly agents = new Set<Agent>();
  private readonly listeners = new Set<(state: AgentState) => void>();
  private readonly notificationListeners = new Map<string, Set<(params: unknown) => void>>();

  constructor(private readonly log: vscode.LogOutputChannel) {}

  get current(): AgentState {
    return this.agent?.current ?? { kind: "starting" };
  }

  /** The process itself: what it started with is its own (the settings a restart would apply). */
  get process(): Agent | undefined {
    return this.agent;
  }

  /** The shown agent's state changes, and the panel shows another agent. */
  onChange(listener: (state: AgentState) => void): { dispose(): void } {
    this.listeners.add(listener);
    return { dispose: () => this.listeners.delete(listener) };
  }

  /** Listens to every agent's notification; register before the first agent. */
  onNotification(method: string, listener: (params: unknown) => void): { dispose(): void } {
    const listeners = this.notificationListeners.get(method) ?? new Set();
    this.notificationListeners.set(method, listeners.add(listener));
    return { dispose: () => listeners.delete(listener) };
  }

  request<Response>(method: string, params: Record<string, unknown> = {}): Promise<Response> {
    if (!this.agent) return Promise.reject(new Error("the agent is not running"));
    return this.agent.request<Response>(method, params);
  }

  notify(method: string, params: Record<string, unknown> = {}): void {
    this.agent?.notify(method, params);
  }

  /** A new agent, before it starts: its notifications reach the listeners. */
  attach(agent: Agent): void {
    this.agents.add(agent);
    for (const [method, listeners] of this.notificationListeners) {
      agent.onNotification(method, (params) => {
        for (const listener of listeners) listener(params);
      });
    }
  }

  detach(agent: Agent): void {
    this.agents.delete(agent);
  }

  show(agent: Agent): void {
    this.watch?.dispose();
    this.agent = agent;
    this.watch = agent.onChange((state) => this.emit(state));
    this.emit(agent.current);
  }

  /** Tells the other running agents, which read what changed on disk (a sign-in). */
  broadcast(method: string): void {
    for (const agent of this.agents) {
      if (agent === this.agent || agent.current.kind !== "ready") continue;
      agent.request(method).catch((error: unknown) => this.log.warn(`${method} not sent: ${errorText(error)}`));
    }
  }

  private emit(state: AgentState): void {
    for (const listener of this.listeners) listener(state);
  }
}

// Repris d'Unharnessed (MIT, 157dcdc1d, vscode/src/agent.ts). yusAgent (YA-10, ticket 24, PR I) :
// - notre client ACP (`./acp/connection`) à la place de `@agentclientprotocol/sdk` ;
// - binaire du réglage `yusagent.binaryPath`, résolu en chemin absolu ; refus sans dossier ouvert ;
// - `initialize` sans `clientInfo` (fixture des trames, §5.4) ;
// - méthodes que le pont ne sert pas : rejetées sur place, sans trame (`./ya/methods`) ;
// - `session/request_permission` (`./ya/permissions`) : `cancelled` après « Arrêter » ;
// - journal : méthode et id des trames, jamais leurs paramètres (règle 9) ;
// - arrêt : fin de stdin, 10 s au plus, puis SIGKILL (notre pont sort aussitôt, ticket 24, point 1).
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createInterface } from "node:readline";
import type * as vscode from "vscode";
import { INITIALIZE_PARAMS } from "./acp/client";
import { Connection, METHOD_NOT_FOUND, RpcError } from "./acp/connection";
import { PROTOCOL_VERSION } from "./acp/types";
import { describeFailure, resolveBinary } from "./binary";
import type { AgentState } from "./protocol";
import { notServed, served } from "./ya/methods";
import { Permissions } from "./ya/permissions";
import { agentEnv } from "./ya/env"; // yusAgent (PR 3)

const STDERR_TAIL = 20;
const INITIALIZE_TIMEOUT_MS = 30_000;
/**
 * How long `stop` waits for the agent to exit once its stdin is closed. yusAgent: our bridge
 * cancels the turn and detaches at once (0.01 s measured, ticket 24, point 1); the session and its
 * kernel stay in the daemon until it passivates them.
 */
const EXIT_WAIT_MS = 10_000;
/** A process that dies closes the connection a moment before its exit event. */
const EXIT_EVENT_MS = 2_000;
const NO_FOLDER = "Ouvrez un dossier : yusAgent démarre dans le dossier du workspace.";

/** A saved conversation: its session id, and the file the agent resumes. */
export type SavedSession = { id: string; path: string };

/**
 * One `yusagent --mode acp` process and its ACP connection. The state
 * moves starting → ready → failed (crash), and back to starting on restart.
 * A process starts on a new conversation, or on a saved one (`--resume`).
 */
export class Agent {
  private state: AgentState;
  private proc: ChildProcessWithoutNullStreams | undefined;
  private saved: SavedSession | undefined;
  private connection: Connection | undefined;
  private stderr: string[] = [];
  private restarting: Promise<void> | undefined;
  private resolved: string | undefined;
  private readonly listeners = new Set<(state: AgentState) => void>();
  private readonly notificationListeners = new Map<string, Set<(params: unknown) => void>>();
  /** yusAgent: the bridge's permission requests, open until answered or closed. */
  readonly permissions = new Permissions();

  constructor(
    private readonly binary: string | undefined,
    private readonly cwd: string,
    private readonly version: string,
    private readonly log: vscode.LogOutputChannel,
    private readonly env: NodeJS.ProcessEnv = process.env,
  ) {
    this.state = { kind: "starting" };
  }

  get current(): AgentState {
    return this.state;
  }

  /** The saved conversation the running process was started on, if any. */
  get resumed(): SavedSession | undefined {
    return this.saved;
  }

  /** yusAgent: the binary as launched (the `/login` message names it). */
  get binaryPath(): string {
    return this.resolved ?? this.binary ?? "yusagent";
  }

  onChange(listener: (state: AgentState) => void): { dispose(): void } {
    this.listeners.add(listener);
    return { dispose: () => this.listeners.delete(listener) };
  }

  /** Listens to an agent notification; register before `start`. */
  onNotification(method: string, listener: (params: unknown) => void): { dispose(): void } {
    const listeners = this.notificationListeners.get(method) ?? new Set();
    this.notificationListeners.set(method, listeners.add(listener));
    return { dispose: () => listeners.delete(listener) };
  }

  /** A request to the running agent; rejects when it is not ready, or when the bridge does not serve it. */
  request<Response>(method: string, params: Record<string, unknown> = {}): Promise<Response> {
    if (!served(method)) return Promise.reject(notServed(method));
    if (this.state.kind !== "ready" || !this.connection) {
      return Promise.reject(new Error("the agent is not running"));
    }
    return this.connection.request<Response>(method, params);
  }

  notify(method: string, params: Record<string, unknown> = {}): void {
    if (!served(method)) {
      this.log.info(`${method} not sent: not served by yusAgent yet`);
      return;
    }
    if (this.state.kind !== "ready" || !this.connection) return;
    this.connection.notify(method, params);
    // ACP v1: after `session/cancel`, every open permission request is answered `cancelled`.
    if (method === "session/cancel") this.permissions.closeAll();
  }

  /** On a new conversation, or on `saved`. */
  start(saved?: SavedSession): void {
    this.stderr = [];
    this.saved = saved;
    this.proc = undefined;
    this.set({ kind: "starting" });
    if (!this.cwd) {
      this.set({ kind: "failed", error: NO_FOLDER, stderr: [] });
      return;
    }
    const resolution = resolveBinary(this.binary, this.env, this.cwd);
    if (!resolution.ok) {
      this.log.error(`agent failed: ${describeFailure(resolution)}`);
      this.set({ kind: "failed", error: describeFailure(resolution), stderr: [] });
      return;
    }
    this.resolved = resolution.path;
    const args = ["--mode", "acp", ...(saved ? ["--resume", saved.path] : [])];
    this.log.info(`starting agent: ${resolution.path} ${args.join(" ")} (cwd ${this.cwd})`);

    // yusAgent (PR 3) : sans PRIME_AGENT_KERNEL_PYTHON (src/ya/env.ts).
    const proc = spawn(resolution.path, args, { cwd: this.cwd, env: agentEnv(this.env), stdio: "pipe" });
    this.proc = proc;
    // ENOENT / EACCES arrive here, without an exit event.
    proc.on("error", (error) => this.fail(proc, error.message));
    proc.on("exit", (code, signal) => this.fail(proc, `agent exited (${signal ?? `code ${code}`})`));
    // Writing to a process that already died raises EPIPE on stdin.
    proc.stdin.on("error", (error) => this.log.warn(`agent stdin: ${error.message}`));
    createInterface({ input: proc.stderr }).on("line", (line) => this.onStderr(proc, line));

    const connection = new Connection(
      (line) => {
        const head = JSON.parse(line) as { id?: unknown; method?: string };
        this.log.debug(`→ ${head.method ?? "response"}${head.id !== undefined ? ` #${String(head.id)}` : ""}`);
        if (!proc.stdin.destroyed) proc.stdin.write(line);
      },
      {
        onRequest: (method, params, context) => {
          this.log.debug(`← ${method} #${String(context.id)}`);
          if (method === "session/request_permission") return this.permissions.request(params, context);
          throw new RpcError(METHOD_NOT_FOUND, "Method not found");
        },
        onNotification: (method, params) => this.onAgentNotification(method, params),
        onProtocolError: (message) => this.log.warn(`acp: ${message}`),
      },
    );
    this.connection = connection;
    proc.stdout.setEncoding("utf8");
    proc.stdout.on("data", (chunk: string) => connection.feed(chunk));

    // Nothing is written before the process exists: a missing binary never
    // sees a write (which would raise SIGPIPE in the extension host).
    proc.once("spawn", () => {
      this.log.info(`agent started (pid ${proc.pid})`);
      this.initialize(proc, connection);
    });
  }

  /** On a new conversation, or on `saved`. A second call while one is stopping the agent joins it. */
  restart(saved?: SavedSession): Promise<void> {
    this.restarting ??= this.stop()
      .then(() => this.start(saved))
      .finally(() => (this.restarting = undefined));
    return this.restarting;
  }

  /**
   * Closes stdin: our bridge cancels the running turn and exits. SIGKILL if
   * it has not after `EXIT_WAIT_MS`.
   */
  async stop(): Promise<void> {
    const proc = this.proc;
    this.proc = undefined;
    this.connection?.close("stopped");
    this.connection = undefined;
    this.permissions.closeAll();
    if (!proc || proc.pid === undefined || hasExited(proc)) return;

    const began = Date.now();
    proc.stdin.end();
    if (await exitsWithin(proc, EXIT_WAIT_MS)) {
      this.log.info(`agent ${proc.pid} exited ${Date.now() - began} ms after stdin closed`);
      return;
    }
    this.log.warn(`agent ${proc.pid} still running ${EXIT_WAIT_MS / 1000} s after stdin closed: SIGKILL`);
    proc.kill("SIGKILL");
  }

  private onAgentNotification(method: string, params: unknown): void {
    this.log.debug(`← ${method}`);
    if (method === "session/update") {
      // E9, par défense : un appel sorti de `pending` a sa demande réglée ailleurs.
      const update = (params as { update?: { sessionUpdate?: string; toolCallId?: string; status?: string } })?.update;
      if (update?.sessionUpdate === "tool_call_update" && update.toolCallId && update.status && update.status !== "pending") {
        this.permissions.settled(update.toolCallId);
      }
    }
    for (const listener of this.notificationListeners.get(method) ?? []) listener(params);
  }

  private initialize(proc: ChildProcessWithoutNullStreams, connection: Connection): void {
    const initialize = connection.request<{ protocolVersion?: number; agentInfo?: { name?: string; version?: string } }>(
      "initialize",
      INITIALIZE_PARAMS,
    );
    withTimeout(initialize, INITIALIZE_TIMEOUT_MS, "initialize").then(
      (result) => {
        if (proc !== this.proc || this.state.kind !== "starting") return;
        if (result?.protocolVersion !== PROTOCOL_VERSION) {
          this.fail(proc, `ACP version ${String(result?.protocolVersion)} announced, ${PROTOCOL_VERSION} expected`);
          void this.stop();
          return;
        }
        const { name = "agent", version = "unknown" } = result.agentInfo ?? {};
        this.log.info(`agent ready: ${name} ${version} (protocol ${result.protocolVersion}, pid ${proc.pid})`);
        this.set({ kind: "ready" });
      },
      async (error: unknown) => {
        // A process that dies closes the connection before its exit event:
        // let the exit handler report the exit code instead.
        if (await exitsWithin(proc, EXIT_EVENT_MS)) return;
        this.fail(proc, `initialize failed: ${errorText(error)}`);
        void this.stop();
      },
    );
  }

  private onStderr(proc: ChildProcessWithoutNullStreams, line: string): void {
    this.log.info(`[agent] ${line}`);
    if (proc !== this.proc) return;
    this.stderr = [...this.stderr, line].slice(-STDERR_TAIL);
  }

  /** Ignores events from a process that was already replaced or stopped. */
  private fail(proc: ChildProcessWithoutNullStreams, error: string): void {
    if (proc !== this.proc || this.state.kind === "failed") return;
    this.log.error(`agent failed: ${error}`);
    this.connection?.close("failed");
    this.permissions.closeAll();
    this.set({ kind: "failed", error, stderr: this.stderr });
  }

  private set(state: AgentState): void {
    this.state = state;
    for (const listener of this.listeners) listener(state);
  }
}

function withTimeout<T>(promise: Promise<T>, ms: number, what: string): Promise<T> {
  let timer: NodeJS.Timeout | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(`no ${what} answer after ${ms / 1000} s`)), ms);
  });
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
}

function hasExited(proc: ChildProcessWithoutNullStreams): boolean {
  return proc.exitCode !== null || proc.signalCode !== null;
}

/** Whether `proc` has exited, or exits within `ms`. */
function exitsWithin(proc: ChildProcessWithoutNullStreams, ms: number): Promise<boolean> {
  if (hasExited(proc)) return Promise.resolve(true);
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      proc.off("exit", onExit);
      resolve(false);
    }, ms);
    const onExit = () => {
      clearTimeout(timer);
      resolve(true);
    };
    proc.once("exit", onExit);
  });
}

/** A JSON-RPC error's `data.details` (the agent's reason), else its message. */
export function errorText(error: unknown): string {
  if (typeof error === "object" && error) {
    const { data } = error as { data?: unknown };
    if (typeof data === "object" && data && "details" in data) return String(data.details);
    if ("message" in error) return String(error.message);
  }
  return String(error);
}

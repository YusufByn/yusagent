import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createInterface } from "node:readline";
import { Readable, Writable } from "node:stream";
import * as acp from "@agentclientprotocol/sdk";
import type * as vscode from "vscode";
import type { AgentState } from "./protocol";

const STDERR_TAIL = 20;
const INITIALIZE_TIMEOUT_MS = 30_000;
/**
 * How long `stop` waits for the agent to exit once its stdin is closed:
 * past the agent's own bounds (its turn 10 s, its kernel's save and
 * shutdown about 25 s).
 */
const EXIT_WAIT_MS = 40_000;
/** A process that dies closes the connection a moment before its exit event. */
const EXIT_EVENT_MS = 2_000;

/** Request params whose values never reach the log (API keys, pasted codes). */
const SECRET_PARAMS: Record<string, string> = {
  "_unharnessed/providers/set_api_key": "apiKey",
  "_unharnessed/providers/login_answer": "answer",
};

/** A saved conversation: its session id, and the file the agent resumes. */
export type SavedSession = { id: string; path: string };

/**
 * One `unharnessed --mode acp` process and its ACP connection. The state
 * moves starting → ready → failed (crash), and back to starting on restart.
 * A process starts on a new conversation, or on a saved one (`--resume`).
 */
export class Agent {
  private state: AgentState;
  private proc: ChildProcessWithoutNullStreams | undefined;
  private saved: SavedSession | undefined;
  private connection: acp.ClientConnection | undefined;
  private stderr: string[] = [];
  private restarting: Promise<void> | undefined;
  private readonly listeners = new Set<(state: AgentState) => void>();
  private readonly notificationListeners = new Map<string, Set<(params: unknown) => void>>();

  constructor(
    private readonly binary: string,
    private readonly cwd: string,
    private readonly version: string,
    private readonly log: vscode.LogOutputChannel,
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

  /** A request to the running agent; rejects when it is not ready. */
  request<Response>(method: string, params: Record<string, unknown> = {}): Promise<Response> {
    if (this.state.kind !== "ready" || !this.connection) {
      return Promise.reject(new Error("the agent is not running"));
    }
    return this.connection.agent.request<Response>(method, params);
  }

  notify(method: string, params: Record<string, unknown> = {}): void {
    if (this.state.kind !== "ready" || !this.connection) return;
    this.connection.agent.notify(method, params).catch((error: unknown) => {
      this.log.warn(`${method} not sent: ${errorText(error)}`);
    });
  }

  /** On a new conversation, or on `saved`. */
  start(saved?: SavedSession): void {
    this.stderr = [];
    this.saved = saved;
    this.set({ kind: "starting" });
    const args = ["--mode", "acp", ...(saved ? ["--resume", saved.path] : [])];
    this.log.info(`starting agent: ${this.binary} ${args.join(" ")} (cwd ${this.cwd})`);

    const proc = spawn(this.binary, args, { cwd: this.cwd, stdio: "pipe" });
    this.proc = proc;
    // ENOENT / EACCES arrive here, without an exit event.
    proc.on("error", (error) => this.fail(proc, error.message));
    proc.on("exit", (code, signal) => this.fail(proc, `agent exited (${signal ?? `code ${code}`})`));
    // Writing to a process that already died raises EPIPE on stdin.
    proc.stdin.on("error", (error) => this.log.warn(`agent stdin: ${error.message}`));
    createInterface({ input: proc.stderr }).on("line", (line) => this.onStderr(proc, line));

    const stream = acp.ndJsonStream(
      Writable.toWeb(proc.stdin) as WritableStream<Uint8Array>,
      Readable.toWeb(proc.stdout) as ReadableStream<Uint8Array>,
    );
    const client = acp.client({ name: "unharnessed-vscode" });
    for (const [method, listeners] of this.notificationListeners) {
      client.onNotification(method, (params) => params, ({ params }) => {
        for (const listener of listeners) listener(params);
      });
    }
    const connection = client.connect(logged(stream, this.log));
    this.connection = connection;

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
   * Closes stdin: the agent stops its turn, saves its kernel and exits.
   * SIGKILL if it has not after `EXIT_WAIT_MS`. When VS Code closes it
   * waits less; the agent goes on alone and saves all the same.
   */
  async stop(): Promise<void> {
    const proc = this.proc;
    this.proc = undefined;
    this.connection?.close();
    this.connection = undefined;
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

  private initialize(proc: ChildProcessWithoutNullStreams, connection: acp.ClientConnection): void {
    const initialize = connection.agent.request("initialize", {
      protocolVersion: acp.PROTOCOL_VERSION,
      clientCapabilities: { fs: { readTextFile: false, writeTextFile: false }, terminal: false },
      clientInfo: { name: "unharnessed-vscode", title: "Unharnessed for VS Code", version: this.version },
    });
    withTimeout(initialize, INITIALIZE_TIMEOUT_MS, "initialize").then(
      (result) => {
        if (proc !== this.proc || this.state.kind !== "starting") return;
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
    this.connection?.close();
    this.set({ kind: "failed", error, stderr: this.stderr });
  }

  private set(state: AgentState): void {
    this.state = state;
    for (const listener of this.listeners) listener(state);
  }
}

/** Writes every JSON-RPC frame to the log: `→` sent, `←` received. */
function logged(stream: acp.Stream, log: vscode.LogOutputChannel): acp.Stream {
  const tap = (arrow: string) =>
    new TransformStream<acp.AnyMessage, acp.AnyMessage>({
      transform(message, controller) {
        log.info(`${arrow} ${JSON.stringify(redacted(message))}`);
        controller.enqueue(message);
      },
    });
  const outgoing = tap("→");
  outgoing.readable.pipeTo(stream.writable).catch(() => {});
  return { readable: stream.readable.pipeThrough(tap("←")), writable: outgoing.writable };
}

function redacted(message: acp.AnyMessage): acp.AnyMessage {
  const secret = "method" in message ? SECRET_PARAMS[message.method] : undefined;
  if (!secret || !("params" in message) || typeof message.params !== "object" || !message.params) {
    return message;
  }
  return { ...message, params: { ...message.params, [secret]: "<redacted>" } };
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

// Un chat = un processus `yusagent --mode acp` lancé dans le dossier du workspace (ticket 24 §1.2).
// Aucune dépendance à `vscode` : testé avec `node --test` contre l'agent factice.

import { spawn, type ChildProcess } from "node:child_process";

import { AcpClient } from "./acp/client";
import { ConnectionClosed, RpcError, type IncomingContext, METHOD_NOT_FOUND } from "./acp/connection";
import {
  PROTOCOL_VERSION,
  type PermissionOutcome,
  type RequestPermissionParams,
  type SessionNotification,
  type TurnFailureData,
} from "./acp/types";
import { describeFailure, resolveBinary } from "./binary";
import { initialState, reduce, type Action, type ChatState } from "./shared/state";

export const ACP_ARGS = ["--mode", "acp"];

/** Texte du pont quand aucun identifiant n'existe (`crates/pa-daemon/src/agent_engine/turn/model.rs:68`). */
const MISSING_CREDENTIAL = "No API key found for ";

/**
 * Échec d'authentification : `kind: "authentication"` (`crates/pa-daemon/src/acp/mod.rs:42-67`),
 * ou le texte d'identifiant absent, que le classifieur range en `other` (constaté sur le binaire de
 * `main` 1b96bb645 : aucune règle de `error_classify.rs:260-357` ne le reconnaît).
 */
export function isAuthFailure(data: TurnFailureData | undefined): boolean {
  if (!data) return false;
  return data.kind === "authentication" || (typeof data.details === "string" && data.details.includes(MISSING_CREDENTIAL));
}
/** Deux arrêts inattendus dans cette fenêtre : plus de relance automatique (choix 10). */
export const CRASH_WINDOW_MS = 60_000;
const CLOSE_TIMEOUT_MS = 2_000;
const KILL_TIMEOUT_MS = 2_000;

export interface SessionOptions {
  /** Valeur du réglage `yusagent.binaryPath`. */
  binaryPath: string | undefined;
  /** Dossier du workspace ; `undefined` : aucun dossier ouvert. */
  cwd: string | undefined;
  workspaceName: string | undefined;
  env: NodeJS.ProcessEnv;
  /** Lignes de stderr du binaire et erreurs de protocole (canal de sortie), jamais l'environnement. */
  log(line: string): void;
  onState(state: ChatState): void;
  now?(): number;
}

interface Running {
  generation: number;
  child: ChildProcess;
  client: AcpClient;
  sessionId: string | undefined;
  binaryPath: string;
}

export class ChatSession {
  private current: ChatState;
  private running: Running | undefined;
  private generation = 0;
  private crashes: number[] = [];
  private disposed = false;
  private readonly permissions = new Map<
    string,
    { toolCallId: string; respond(outcome: PermissionOutcome): void }
  >();

  constructor(private readonly options: SessionOptions) {
    this.current = initialState(options.workspaceName);
  }

  get state(): ChatState {
    return this.current;
  }

  private dispatch(action: Action): void {
    this.current = reduce(this.current, action);
    this.options.onState(this.current);
  }

  private now(): number {
    return this.options.now ? this.options.now() : Date.now();
  }

  /** Lance le processus et ouvre la session. Rien d'autre n'est envoyé (E7). */
  async start(): Promise<void> {
    if (this.disposed) return;
    if (!this.options.cwd) {
      this.dispatch({ type: "noWorkspace" });
      return;
    }
    const resolution = resolveBinary(this.options.binaryPath, this.options.env, this.options.cwd);
    if (!resolution.ok) {
      this.dispatch({ type: "spawnFailed", text: describeFailure(resolution) });
      return;
    }
    this.dispatch({ type: "starting" });
    const generation = ++this.generation;
    let child: ChildProcess;
    try {
      child = spawn(resolution.path, ACP_ARGS, {
        cwd: this.options.cwd,
        env: this.options.env,
        stdio: ["pipe", "pipe", "pipe"],
      });
    } catch (error) {
      this.dispatch({
        type: "spawnFailed",
        text: `Lancement impossible de ${resolution.path} : ${(error as Error).message}`,
      });
      return;
    }
    const client = new AcpClient(
      (line) => {
        if (child.stdin && !child.stdin.destroyed) child.stdin.write(line);
      },
      {
        onRequest: (method, params, context) => this.onRequest(generation, method, params, context),
        onNotification: (method, params) => this.onNotification(generation, method, params),
        onProtocolError: (message) => this.options.log(`[acp] ${message}`),
      },
    );
    const running: Running = {
      generation,
      child,
      client,
      sessionId: undefined,
      binaryPath: resolution.path,
    };
    this.running = running;
    child.stdout?.setEncoding("utf8");
    child.stdout?.on("data", (chunk: string) => client.feed(chunk));
    child.stderr?.setEncoding("utf8");
    let stderrRest = "";
    child.stderr?.on("data", (chunk: string) => {
      const lines = (stderrRest + chunk).split("\n");
      stderrRest = lines.pop() ?? "";
      for (const line of lines) this.options.log(line);
    });
    child.stdin?.on("error", () => {
      /* EPIPE : la fin du processus est traitée par `exit`. */
    });
    child.on("error", (error: NodeJS.ErrnoException) => {
      if (this.running?.generation !== generation) return;
      this.running = undefined;
      client.connection.close("spawn error");
      this.dispatch({
        type: "spawnFailed",
        text: `Lancement impossible de ${resolution.path} : ${error.code ?? error.message}`,
      });
    });
    child.on("exit", (code, signal) => this.onExit(generation, code, signal));

    try {
      const init = await client.initialize();
      if (init?.protocolVersion !== PROTOCOL_VERSION) {
        throw new Error(
          `version ACP ${String(init?.protocolVersion)} annoncée, ${PROTOCOL_VERSION} attendue`,
        );
      }
      const session = await client.newSession(this.options.cwd);
      if (this.running?.generation !== generation) return;
      running.sessionId = session.sessionId;
      this.dispatch({ type: "started", configOptions: session.configOptions ?? [] });
    } catch (error) {
      if (error instanceof ConnectionClosed || this.running?.generation !== generation) return;
      this.options.log(`[acp] ouverture de session : ${(error as Error).message}`);
      this.dispatch({
        type: "startFailed",
        text: `Ouverture de la session impossible : ${(error as Error).message}`,
      });
      void this.terminate(running);
    }
  }

  async send(text: string): Promise<void> {
    const running = this.running;
    const trimmed = text.trim();
    if (!running?.sessionId || this.current.phase !== "ready" || !trimmed) return;
    this.dispatch({ type: "userPrompt", text });
    try {
      const result = await running.client.prompt(running.sessionId, text);
      if (this.running !== running) return;
      this.dispatch({ type: "turnEnded", stopReason: result?.stopReason ?? "end_turn" });
    } catch (error) {
      if (this.running !== running || error instanceof ConnectionClosed) return;
      const data = error instanceof RpcError ? (error.data as TurnFailureData | undefined) : undefined;
      if (isAuthFailure(data)) {
        this.dispatch({ type: "authFailed", binaryPath: running.binaryPath });
        return;
      }
      const details = data?.details ? ` : ${data.details}` : "";
      this.dispatch({ type: "turnFailed", text: `Échec du tour (${(error as Error).message}${details})` });
    } finally {
      this.closeAllPermissions();
    }
  }

  /** Bouton d'arrêt : `session/cancel`, puis `cancelled` à chaque demande d'accord ouverte (spec v1). */
  stop(): void {
    const running = this.running;
    if (!running?.sessionId || this.current.phase !== "turn") return;
    this.dispatch({ type: "stopRequested" });
    running.client.cancel(running.sessionId);
    this.closeAllPermissions();
  }

  async setConfig(configId: string, value: string): Promise<void> {
    const running = this.running;
    if (!running?.sessionId || this.current.phase !== "ready") return;
    const option = this.current.configOptions.find((candidate) => candidate.id === configId);
    if (!option || option.currentValue === value) return;
    try {
      const options = await running.client.setConfigOption(running.sessionId, configId, value);
      if (this.running !== running) return;
      if (options) this.dispatch({ type: "configOptions", configOptions: options });
    } catch (error) {
      if (this.running !== running || error instanceof ConnectionClosed) return;
      this.dispatch({ type: "configFailed", text: `Réglage refusé (${configId}) : ${(error as Error).message}` });
    }
  }

  /** Bouton « Réessayer » / « Relancer » : compteur d'arrêts remis à zéro. */
  async retry(): Promise<void> {
    if (this.disposed || this.running) return;
    this.crashes = [];
    await this.start();
  }

  /** Onglet fermé : `session/close`, fin de stdin, SIGTERM, puis SIGKILL. */
  async dispose(): Promise<void> {
    if (this.disposed) return;
    this.disposed = true;
    const running = this.running;
    this.running = undefined;
    this.dispatch({ type: "closed" });
    if (!running) return;
    if (running.sessionId && running.child.exitCode === null) {
      await Promise.race([
        running.client.close(running.sessionId).catch(() => undefined),
        delay(CLOSE_TIMEOUT_MS),
      ]);
    }
    await this.terminate(running);
  }

  private async terminate(running: Running): Promise<void> {
    if (this.running === running) this.running = undefined;
    running.client.connection.close("terminated");
    const child = running.child;
    if (child.exitCode !== null || child.signalCode !== null) return;
    const exited = new Promise<void>((resolveExit) => child.once("exit", () => resolveExit()));
    child.stdin?.end();
    child.kill("SIGTERM");
    await Promise.race([exited, delay(KILL_TIMEOUT_MS)]);
    if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
  }

  private onExit(generation: number, code: number | null, signal: NodeJS.Signals | null): void {
    const running = this.running;
    if (this.disposed || running?.generation !== generation) return;
    this.running = undefined;
    running.client.connection.close("exited");
    this.permissions.clear();
    const description = signal ? `signal ${signal}` : `code ${String(code)}`;
    this.options.log(`[yusagent] processus arrêté (${description})`);
    const now = this.now();
    this.crashes = this.crashes.filter((at) => now - at < CRASH_WINDOW_MS);
    this.crashes.push(now);
    const restarting = this.crashes.length < 2;
    this.dispatch({ type: "processExited", description, restarting });
    if (restarting) void this.start();
  }

  private onNotification(generation: number, method: string, params: unknown): void {
    if (this.running?.generation !== generation) return;
    if (method !== "session/update") return;
    const notification = params as SessionNotification | undefined;
    const update = notification?.update;
    if (!update || typeof update.sessionUpdate !== "string") return;
    if (notification.sessionId !== this.running.sessionId) return;
    this.dispatch({ type: "update", update });
    // E9, par défense : un appel sorti de `pending` ferme sa demande d'accord.
    if (update.sessionUpdate === "tool_call_update") {
      const call = update as { toolCallId?: string; status?: string };
      if (call.status && call.status !== "pending") {
        for (const [key, pending] of this.permissions) {
          if (pending.toolCallId === call.toolCallId) this.closePermission(key);
        }
      }
    }
  }

  private onRequest(
    generation: number,
    method: string,
    params: unknown,
    context: IncomingContext,
  ): unknown {
    if (this.running?.generation !== generation) {
      throw new RpcError(METHOD_NOT_FOUND, "Method not found");
    }
    if (method !== "session/request_permission") {
      throw new RpcError(METHOD_NOT_FOUND, "Method not found");
    }
    // Choix 7 : pas de boîte avant la PR 3. Aucune réponse avant « Arrêter » (cancelled),
    // `$/cancel_request` ou la sortie de l'appel de `pending`.
    const request = params as RequestPermissionParams;
    const key = String(context.id);
    const toolCallId = request?.toolCall?.toolCallId ?? "";
    return new Promise<{ outcome: PermissionOutcome }>((resolveAnswer) => {
      this.permissions.set(key, {
        toolCallId,
        respond: (outcome) => resolveAnswer({ outcome }),
      });
      context.onCancel(() => this.closePermission(key));
      this.dispatch({ type: "permissionRequested", requestKey: key, toolCallId });
    });
  }

  private closePermission(key: string): void {
    const pending = this.permissions.get(key);
    if (!pending) return;
    this.permissions.delete(key);
    pending.respond({ outcome: "cancelled" });
    this.dispatch({ type: "permissionClosed", requestKey: key });
  }

  private closeAllPermissions(): void {
    for (const key of [...this.permissions.keys()]) this.closePermission(key);
  }
}

function delay(ms: number): Promise<void> {
  return new Promise((resolveDelay) => setTimeout(resolveDelay, ms).unref?.());
}

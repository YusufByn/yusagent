// JSON-RPC 2.0, une trame JSON par ligne (ACP sur stdio), écrit à la main (E2, ticket 24).
// Aucune dépendance à `vscode` : testé seul avec `node --test`.

export type Id = number | string;

export interface RpcErrorShape {
  code: number;
  message: string;
  data?: unknown;
}

/** Erreur rendue par l'autre côté (réponse `error`), ou levée par un gestionnaire local. */
export class RpcError extends Error {
  constructor(
    readonly code: number,
    message: string,
    readonly data?: unknown,
  ) {
    super(message);
    this.name = "RpcError";
  }
}

/** La connexion s'est fermée avant la réponse (processus arrêté, fin de flux). */
export class ConnectionClosed extends Error {
  constructor(reason: string) {
    super(reason);
    this.name = "ConnectionClosed";
  }
}

export const METHOD_NOT_FOUND = -32601;
export const INTERNAL_ERROR = -32603;
/** `$/cancel_request` : code d'une requête annulée (RFD request-cancellation). */
export const REQUEST_CANCELLED = -32800;

/** Contexte d'une requête reçue : `onCancel` est appelé sur `$/cancel_request` de son id. */
export interface IncomingContext {
  id: Id;
  onCancel(callback: () => void): void;
}

export interface Handlers {
  /** Requête reçue. Le résultat (ou la promesse) devient la réponse ; une `RpcError` devient `error`. */
  onRequest(method: string, params: unknown, context: IncomingContext): unknown;
  onNotification(method: string, params: unknown): void;
  /** Ligne illisible ou trame inattendue : signalée, jamais fatale. */
  onProtocolError?(message: string): void;
}

interface Pending {
  method: string;
  resolve(value: unknown): void;
  reject(error: Error): void;
}

export class Connection {
  private nextId = 1;
  private buffer = "";
  private closed: string | undefined;
  private readonly pending = new Map<Id, Pending>();
  private readonly cancels = new Map<string, () => void>();
  private readonly answered = new Set<string>();

  constructor(
    private readonly write: (line: string) => void,
    private readonly handlers: Handlers,
  ) {}

  /** Morceau lu sur la sortie de l'agent ; les lignes complètes sont traitées dans l'ordre. */
  feed(chunk: string): void {
    this.buffer += chunk;
    let newline = this.buffer.indexOf("\n");
    while (newline >= 0) {
      const line = this.buffer.slice(0, newline).trim();
      this.buffer = this.buffer.slice(newline + 1);
      if (line) this.receive(line);
      newline = this.buffer.indexOf("\n");
    }
  }

  request<T = unknown>(method: string, params: unknown): Promise<T> {
    if (this.closed !== undefined) return Promise.reject(new ConnectionClosed(this.closed));
    const id = this.nextId++;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { method, resolve: resolve as (value: unknown) => void, reject });
      this.send({ jsonrpc: "2.0", id, method, params });
    });
  }

  notify(method: string, params: unknown): void {
    if (this.closed !== undefined) return;
    this.send({ jsonrpc: "2.0", method, params });
  }

  /** Ferme la connexion : toutes les requêtes en attente sont rejetées avec `ConnectionClosed`. */
  close(reason: string): void {
    if (this.closed !== undefined) return;
    this.closed = reason;
    for (const [, pending] of this.pending) pending.reject(new ConnectionClosed(reason));
    this.pending.clear();
    this.cancels.clear();
  }

  get isClosed(): boolean {
    return this.closed !== undefined;
  }

  private send(message: object): void {
    this.write(JSON.stringify(message) + "\n");
  }

  private receive(line: string): void {
    let message: Record<string, unknown>;
    try {
      const parsed: unknown = JSON.parse(line);
      if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
        this.handlers.onProtocolError?.(`trame non objet ignorée : ${line.slice(0, 200)}`);
        return;
      }
      message = parsed as Record<string, unknown>;
    } catch {
      this.handlers.onProtocolError?.(`ligne non JSON ignorée : ${line.slice(0, 200)}`);
      return;
    }
    const method = typeof message.method === "string" ? message.method : undefined;
    const hasId = typeof message.id === "number" || typeof message.id === "string";
    if (method !== undefined && hasId) {
      this.receiveRequest(message.id as Id, method, message.params);
    } else if (method !== undefined) {
      this.receiveNotification(method, message.params);
    } else if (hasId) {
      this.receiveResponse(message);
    } else {
      this.handlers.onProtocolError?.(`trame sans méthode ni id ignorée : ${line.slice(0, 200)}`);
    }
  }

  private receiveResponse(message: Record<string, unknown>): void {
    const id = message.id as Id;
    const pending = this.pending.get(id);
    if (!pending) {
      this.handlers.onProtocolError?.(`réponse sans requête ignorée (id ${String(id)})`);
      return;
    }
    this.pending.delete(id);
    const error = message.error as RpcErrorShape | undefined;
    if (error && typeof error === "object") {
      pending.reject(new RpcError(error.code, error.message, error.data));
    } else {
      pending.resolve(message.result);
    }
  }

  private receiveNotification(method: string, params: unknown): void {
    if (method === "$/cancel_request") {
      const requestId = (params as { requestId?: Id } | undefined)?.requestId;
      const cancel = requestId === undefined ? undefined : this.cancels.get(key(requestId));
      if (cancel) {
        this.cancels.delete(key(requestId as Id));
        cancel();
      }
      return;
    }
    this.handlers.onNotification(method, params);
  }

  private receiveRequest(id: Id, method: string, params: unknown): void {
    const context: IncomingContext = {
      id,
      onCancel: (callback) => {
        if (!this.answered.has(key(id))) this.cancels.set(key(id), callback);
      },
    };
    let outcome: unknown;
    try {
      outcome = this.handlers.onRequest(method, params, context);
    } catch (error) {
      this.answer(id, undefined, error);
      return;
    }
    Promise.resolve(outcome).then(
      (result) => this.answer(id, result, undefined),
      (error: unknown) => this.answer(id, undefined, error),
    );
  }

  /** Une seule réponse par requête reçue ; rien après la fermeture. */
  private answer(id: Id, result: unknown, error: unknown): void {
    if (this.answered.has(key(id))) return;
    this.answered.add(key(id));
    this.cancels.delete(key(id));
    if (this.closed !== undefined) return;
    if (error === undefined) {
      this.send({ jsonrpc: "2.0", id, result: result ?? null });
      return;
    }
    const rpc =
      error instanceof RpcError
        ? error
        : new RpcError(INTERNAL_ERROR, error instanceof Error ? error.message : String(error));
    const body: RpcErrorShape = { code: rpc.code, message: rpc.message };
    if (rpc.data !== undefined) body.data = rpc.data;
    this.send({ jsonrpc: "2.0", id, error: body });
  }
}

function key(id: Id): string {
  return typeof id === "number" ? `n:${id}` : `s:${id}`;
}

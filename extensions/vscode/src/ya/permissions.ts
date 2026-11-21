// yusAgent (YA-10, ticket 24) : les demandes d'accord du pont (`session/request_permission`,
// `crates/pa-daemon/src/acp/ya_permissions.rs:262-271`). PR 3 : la boîte de la webview y répond
// (`answer`, une des options de la demande, `allow-once` ou `reject-once`, D3). Une demande se ferme
// aussi sans réponse de la boîte, avec `cancelled` : « Arrêter » (`session/cancel` puis `cancelled`,
// spec v1), `$/cancel_request` du pont (E9), la sortie de son appel de `pending`, la fin de la
// connexion.

import type { IncomingContext } from "../acp/connection";
import type { PermissionAsk, PermissionOption } from "../protocol";

type Outcome = { outcome: { outcome: "cancelled" } | { outcome: "selected"; optionId: string } };

type Open = { toolCallId: string; options: PermissionOption[]; answer(outcome: Outcome): void };

export class Permissions {
  /** Par identifiant de requête du pont, dans l'ordre d'arrivée. */
  private readonly open = new Map<string, Open>();
  private readonly listeners = new Set<(toolCallId: string, waiting: boolean) => void>();

  /** Une demande s'ouvre ou se ferme (l'appel visé, et si elle attend encore). */
  onChange(listener: (toolCallId: string, waiting: boolean) => void): { dispose(): void } {
    this.listeners.add(listener);
    return { dispose: () => this.listeners.delete(listener) };
  }

  /** Gestionnaire de `session/request_permission` : la réponse part quand la demande se ferme. */
  request(params: unknown, context: IncomingContext): Promise<Outcome> {
    const key = String(context.id);
    const given = params as { toolCall?: { toolCallId?: string }; options?: unknown } | undefined;
    const toolCallId = given?.toolCall?.toolCallId ?? "";
    const options = readOptions(given?.options);
    return new Promise((answer) => {
      this.open.set(key, { toolCallId, options, answer });
      context.onCancel(() => this.close(key, { outcome: { outcome: "cancelled" } }));
      this.emit(toolCallId, true);
    });
  }

  /** Les demandes ouvertes, dans l'ordre : la boîte montre la première. */
  get asks(): PermissionAsk[] {
    return [...this.open.values()].map(({ toolCallId, options }) => ({ toolCallId, options }));
  }

  /**
   * La réponse de la boîte à la demande de `toolCallId` : une de ses options. `false` si aucune
   * demande ouverte ne correspond (déjà fermée ailleurs) ou si l'option n'est pas la sienne.
   */
  answer(toolCallId: string, optionId: string): boolean {
    for (const [key, open] of this.open) {
      if (open.toolCallId !== toolCallId) continue;
      if (!open.options.some((option) => option.optionId === optionId)) return false;
      this.close(key, { outcome: { outcome: "selected", optionId } });
      return true;
    }
    return false;
  }

  /** L'appel est sorti de `pending` : sa demande est réglée ailleurs (E9, par défense). */
  settled(toolCallId: string): void {
    for (const [key, open] of this.open) if (open.toolCallId === toolCallId) this.close(key, CANCELLED);
  }

  /** « Arrêter », ou la connexion qui se ferme : `cancelled` à chaque demande ouverte. */
  closeAll(): void {
    for (const key of [...this.open.keys()]) this.close(key, CANCELLED);
  }

  get waiting(): number {
    return this.open.size;
  }

  private close(key: string, outcome: Outcome): void {
    const open = this.open.get(key);
    if (!open) return;
    this.open.delete(key);
    open.answer(outcome);
    this.emit(open.toolCallId, false);
  }

  private emit(toolCallId: string, waiting: boolean): void {
    for (const listener of this.listeners) listener(toolCallId, waiting);
  }
}

const CANCELLED: Outcome = { outcome: { outcome: "cancelled" } };

/** Les options de la demande (`optionId`, `name`, `kind`) ; les entrées mal formées sont ignorées. */
function readOptions(options: unknown): PermissionOption[] {
  if (!Array.isArray(options)) return [];
  return options.flatMap((option: Record<string, unknown>) =>
    typeof option?.optionId === "string" && typeof option.name === "string" && typeof option.kind === "string"
      ? [{ optionId: option.optionId, name: option.name, kind: option.kind }]
      : [],
  );
}

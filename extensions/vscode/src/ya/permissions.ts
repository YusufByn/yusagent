// yusAgent (YA-10, ticket 24) : les demandes d'accord du pont (`session/request_permission`,
// `crates/pa-daemon/src/acp/ya_permissions.rs:211-242`). Sans boîte avant la PR 3 : la demande
// reste ouverte, sans réponse, jusqu'à « Arrêter » (`session/cancel` puis `cancelled`, spec v1),
// `$/cancel_request` du pont (E9), ou la sortie de son appel de `pending`.

import type { IncomingContext } from "../acp/connection";

type Outcome = { outcome: { outcome: "cancelled" } | { outcome: "selected"; optionId: string } };

export class Permissions {
  private readonly open = new Map<string, { toolCallId: string; answer(outcome: Outcome): void }>();
  private readonly listeners = new Set<(toolCallId: string, waiting: boolean) => void>();

  /** Une demande s'ouvre ou se ferme (l'appel visé, et si elle attend encore). */
  onChange(listener: (toolCallId: string, waiting: boolean) => void): { dispose(): void } {
    this.listeners.add(listener);
    return { dispose: () => this.listeners.delete(listener) };
  }

  /** Gestionnaire de `session/request_permission` : la réponse part quand la demande se ferme. */
  request(params: unknown, context: IncomingContext): Promise<Outcome> {
    const key = String(context.id);
    const toolCallId = (params as { toolCall?: { toolCallId?: string } } | undefined)?.toolCall?.toolCallId ?? "";
    return new Promise((answer) => {
      this.open.set(key, { toolCallId, answer });
      context.onCancel(() => this.close(key));
      this.emit(toolCallId, true);
    });
  }

  /** L'appel est sorti de `pending` : sa demande est réglée ailleurs (E9, par défense). */
  settled(toolCallId: string): void {
    for (const [key, open] of this.open) if (open.toolCallId === toolCallId) this.close(key);
  }

  /** « Arrêter », ou la connexion qui se ferme : `cancelled` à chaque demande ouverte. */
  closeAll(): void {
    for (const key of [...this.open.keys()]) this.close(key);
  }

  get waiting(): number {
    return this.open.size;
  }

  private close(key: string): void {
    const open = this.open.get(key);
    if (!open) return;
    this.open.delete(key);
    open.answer({ outcome: { outcome: "cancelled" } });
    this.emit(open.toolCallId, false);
  }

  private emit(toolCallId: string, waiting: boolean): void {
    for (const listener of this.listeners) listener(toolCallId, waiting);
  }
}

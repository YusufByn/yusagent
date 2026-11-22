// yusAgent (YA-10, ticket 24, PR I) : méthodes ACP que notre pont sert
// (`crates/pa-daemon/src/acp/daemon.rs:1052-1068` ; ticket 28 : `ya_history.rs`). Le code repris d'Unharnessed en appelle
// d'autres (`_unharnessed/*`, `session/load`, `session/list`) : elles ne partent pas, l'appel est
// rejeté sur place comme le ferait le pont (`-32601`), sans trame (fixture des trames, §5.4).

import { METHOD_NOT_FOUND, RpcError } from "../acp/connection";
import type { Features } from "../protocol";

// Ticket 28 (R1, `crates/pa-daemon/src/acp/ya_history.rs`) : reprise, historique et suppression.
const REQUESTS = new Set([
  "initialize",
  "session/new",
  "session/prompt",
  "session/set_config_option",
  "session/close",
  "session/load",
  "session/list",
  "_unharnessed/sessions/delete",
]);
const NOTIFICATIONS = new Set(["session/cancel"]);

export function served(method: string): boolean {
  return REQUESTS.has(method) || NOTIFICATIONS.has(method);
}

export function notServed(method: string): RpcError {
  return new RpcError(METHOD_NOT_FOUND, `${method} : méthode pas encore servie par yusAgent`);
}

/** Ce que le panneau montre : une fonction n'apparaît que si le pont sert sa méthode. */
export function features(): Features {
  return {
    history: served("session/list"),
    steer: served("_unharnessed/session/steer"),
    rewind: served("_unharnessed/session/rewind"),
    kernel: served("_unharnessed/kernel/tail"),
    providers: served("_unharnessed/providers/list"),
    settings: served("_unharnessed/settings/get"),
    // Blocs texte seulement (§5.4) : ni liens `@fichier` ni images dans le prompt.
    attachments: false,
  };
}

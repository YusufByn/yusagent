// yusAgent (YA-10, ticket 24, PR I) : méthodes ACP que notre pont sert
// (`crates/pa-daemon/src/acp/daemon.rs:1052-1068` ; ticket 28 : `ya_history.rs`). Le code repris d'Unharnessed en appelle
// d'autres (ticket 32 : `_yus/session/rewind`, `_yus/kernel/*`, `_yus/providers/local_*`) : elles ne partent pas, l'appel est
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
  "_yus/sessions/delete", // ticket 32 : nom yusAgent, l'ancien n'est plus servi
  // Ticket 39 (X4, `crates/pa-daemon/src/acp/ya_turns/steer.rs`) : un message pendant un tour.
  "_yus/session/steer",
  // Ticket 31 (X3, `crates/pa-daemon/src/acp/ya_resources.rs`) : écrans skills, MCP et réglages.
  "_yus/settings/get",
  "_yus/settings/set",
  "_yus/skills/list",
  "_yus/skills/set",
  "_yus/skills/create",
  "_yus/skills/add_folder",
  "_yus/skills/remove_folder",
  "_yus/mcp/list",
  "_yus/mcp/set_token",
  "_yus/mcp/logout",
  // Ticket 40 (X5, `crates/pa-daemon/src/acp/daemon/ya_providers.rs`) : écran Providers et login ;
  // pas `local_*` (écran Local masqué, décision 6).
  "_yus/providers/list",
  "_yus/providers/reload",
  "_yus/providers/set_api_key",
  "_yus/providers/logout",
  "_yus/providers/login",
  "_yus/providers/login_answer",
  "_yus/providers/login_open",
]);
const NOTIFICATIONS = new Set(["session/cancel", "_yus/providers/login_cancel"]);

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
    steer: served("_yus/session/steer"),
    rewind: served("_yus/session/rewind"),
    kernel: served("_yus/kernel/tail"),
    providers: served("_yus/providers/list"),
    settings: served("_yus/settings/get"),
    // Blocs texte seulement (§5.4) : ni liens `@fichier` ni images dans le prompt.
    attachments: false,
  };
}

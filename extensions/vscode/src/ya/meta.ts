// yusAgent (YA-10, ticket 24, PR I ; ticket 25) : le `_meta` de nos `session/update` dans la forme que
// lit le code repris d'Unharnessed (`_meta.unharnessed`, `crates/uh-daemon/src/acp/meta.rs` chez lui).
// Notre pont met ses champs sous `ai.primeintellect.prime-agent` (`crates/pa-daemon/src/acp/meta.rs`),
// avec les noms d'Unharnessed (décision du 21/11/2026, ticket 25, T1) :
// - début de chaque tour : `turnStart` (`origin: "prompt"` ou `"wake"`, `wake` le genre), publié par le
//   pont avant le contenu du tour (`crates/pa-daemon/src/acp/ya_turns.rs`) ; il remplace la déduction
//   faite ici depuis la PR I ;
// - fin d'un tour lancé par l'agent : `phase: "responseBoundary"` (`outcome` `result` ou `error`), lue
//   par `chat.ts` (`info`). Un tour de l'extension finit à la réponse de son `session/prompt`.

export const PRIME_AGENT_META = "ai.primeintellect.prime-agent";

/** `M` : le type `Meta` de `chat.ts` (forme d'Unharnessed). */
export function yaMeta<M>(update: { sessionUpdate?: string; _meta?: unknown }): M {
  const meta = update._meta as Record<string, unknown> | undefined;
  if (!meta || typeof meta !== "object") return {} as M;
  if (meta.unharnessed && typeof meta.unharnessed === "object") return meta.unharnessed as M;
  const ours = meta[PRIME_AGENT_META];
  return (ours && typeof ours === "object" ? { ...ours } : {}) as M;
}

/**
 * Ticket 28 (C2) : les champs du pont dans le `_meta` d'une réponse ou d'une ligne de `session/list`
 * (`saved` de `session/new` et `session/load`, `path` et `current` d'une ligne), sous les noms
 * d'Unharnessed (`_meta.unharnessed`, lu tel quel s'il est là). `ya_history.rs` : `add_saved`,
 * `list_rows`.
 */
export type YaResultMeta = {
  saved?: { sessionId: string; path?: string };
  path?: string;
  current?: boolean;
};

export function yaResultMeta(result: unknown): YaResultMeta {
  const meta = (result as { _meta?: unknown } | undefined)?._meta as Record<string, unknown> | undefined;
  if (!meta || typeof meta !== "object") return {};
  const found = meta.unharnessed ?? meta[PRIME_AGENT_META];
  return found && typeof found === "object" ? (found as YaResultMeta) : {};
}

// yusAgent (YA-10, ticket 24, PR I) : le `_meta` de nos `session/update` dans la forme que lit le
// code repris d'Unharnessed (`_meta.unharnessed`, `crates/uh-daemon/src/acp/meta.rs` chez lui).
// Notre pont met `promptTurnId`, `phase`, `outcome` sous `ai.primeintellect.prime-agent`
// (`crates/pa-daemon/src/acp/meta.rs`) et n'annonce ni le début ni la fin d'un tour :
// - début : le premier événement **de contenu** d'un `promptTurnId` encore inconnu
//   (`origin: "prompt"` quand un prompt de l'extension attend sa réponse, `"wake"` sinon : un tour
//   lancé par l'agent, avis `[bash-done]`). Un `config_option_update` ou un `session_info_update`
//   n'ouvre jamais de tour (constaté sur le binaire de `main` 1b96bb645 : le `config_option_update`
//   d'un changement d'effort porte un `promptTurnId`) ;
// - fin d'un tour lancé par l'agent : `phase: "responseBoundary"` (`outcome` `result` ou `error`).
//   Un tour de l'extension finit à la réponse de son `session/prompt`, comme chez Unharnessed.

export const PRIME_AGENT_META = "ai.primeintellect.prime-agent";

const CONTENT = new Set(["agent_message_chunk", "agent_thought_chunk", "tool_call", "tool_call_update", "user_message_chunk"]);

type Meta = {
  promptTurnId?: number;
  phase?: "event" | "responseBoundary" | "terminalQuiescence";
  outcome?: "result" | "error";
  turnStart?: { origin: "prompt" | "wake" | "steer"; wake?: "bash" | "quota" | "goal"; rewindTo?: string };
  turnEnd?: { state: "done" | "cancelled" | "failed"; error?: string };
  /** Début de tour déduit : l'événement porte aussi son propre contenu, à traiter ensuite. */
  yaDeduced?: boolean;
  [field: string]: unknown;
};

export interface TurnContext {
  /** Le chat connaît déjà ce tour (ou rejoue une conversation). */
  known(promptTurnId: number): boolean;
  /** Un prompt de l'extension attend le début de son tour. */
  prompting: boolean;
  /** Ce tour est celui d'un prompt de l'extension. */
  ours(promptTurnId: number): boolean;
}

/** `M` : le type `Meta` de `chat.ts` (forme d'Unharnessed). */
export function yaMeta<M>(update: { sessionUpdate?: string; _meta?: unknown }, context: TurnContext): M {
  return readMeta(update, context) as M;
}

function readMeta(update: { sessionUpdate?: string; _meta?: unknown }, context: TurnContext): Meta {
  const meta = update._meta as Record<string, unknown> | undefined;
  if (!meta || typeof meta !== "object") return {};
  if (meta.unharnessed && typeof meta.unharnessed === "object") return meta.unharnessed as Meta;
  const ours = meta[PRIME_AGENT_META];
  if (!ours || typeof ours !== "object") return {};
  const out: Meta = { ...(ours as Meta) };
  const turn = out.promptTurnId;
  if (typeof turn !== "number") return out;
  if (CONTENT.has(update.sessionUpdate ?? "") && !context.known(turn)) {
    out.turnStart = context.prompting ? { origin: "prompt" } : { origin: "wake", wake: "bash" };
    out.yaDeduced = true;
  } else if (
    update.sessionUpdate === "session_info_update" &&
    out.phase === "responseBoundary" &&
    context.known(turn) &&
    !context.ours(turn)
  ) {
    out.turnEnd = out.outcome === "error" ? { state: "failed", error: "The turn failed." } : { state: "done" };
  }
  return out;
}

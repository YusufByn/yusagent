// yusAgent (YA-10, ticket 24) : échec d'authentification d'un prompt, et le message fixe de
// l'extension (« lancer yusagent puis /login », avec le chemin réel du binaire).

/** Texte du pont quand aucun identifiant n'existe (`crates/pa-daemon/src/agent_engine/turn/model.rs:68`). */
const MISSING_CREDENTIAL = "No API key found for ";

/**
 * `kind: "authentication"` (`crates/pa-daemon/src/acp/mod.rs:42-67`), ou le texte d'identifiant
 * absent, que le classifieur range en `other` (constaté sur le binaire de `main` 1b96bb645 ; aucune
 * règle de `error_classify.rs:260-357` ne le reconnaît).
 */
export function isAuthFailure(error: unknown): boolean {
  const data = (error as { data?: { kind?: unknown; details?: unknown } } | undefined)?.data;
  if (!data || typeof data !== "object") return false;
  return data.kind === "authentication" || (typeof data.details === "string" && data.details.includes(MISSING_CREDENTIAL));
}

export function authText(binaryPath: string): string {
  return `Authentification requise : lancez ${binaryPath} dans un terminal, puis /login.`;
}

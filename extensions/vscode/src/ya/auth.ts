// yusAgent (YA-10, ticket 24 ; ticket 40, X5) : échec d'authentification d'un prompt, et le texte fixe
// de l'extension, qui mène à l'écran Providers (décision 7, réponse 2 de Yusuf du 26/11/2026).

import type { Provider } from "../protocol";

/**
 * `kind: "authentication"` du pont (`crates/pa-daemon/src/acp/mod.rs`, `turn_failure_error`) : depuis
 * R5 (H2, `ya_providers.rs`, `auth_failure_kind`), les textes de la garde y sont classés. Le texte seul
 * (`details`) ne compte plus (réponse 4).
 */
export function isAuthFailure(error: unknown): boolean {
  const data = (error as { data?: { kind?: unknown } } | undefined)?.data;
  return typeof data === "object" && data !== null && data.kind === "authentication";
}

/**
 * Le nom à donner au fournisseur `provider` (id) dans le texte, d'après la liste : la ligne connectée,
 * sinon la première ligne de ce fournisseur, sinon l'id. `undefined` sans fournisseur, ou quand aucun
 * fournisseur n'est connecté : la session tourne alors sur le modèle de repli du démon, le premier du
 * catalogue (`crates/pa-daemon/src/agent_engine/model.rs:41`), que le texte ne nomme pas (défaut 2 du
 * contrôle de Yusuf, ticket 40, §22).
 */
export function authProviderName(providers: Provider[], provider: string | null): string | undefined {
  if (!provider || !providers.some((row) => row.connected)) return undefined;
  const rows = providers.filter((row) => row.id === provider);
  return (rows.find((row) => row.connected) ?? rows[0])?.name ?? provider;
}

/** Le texte fixe ; `provider` : le nom du fournisseur, absent quand il n'est pas connu. */
export function authText(provider?: string): string {
  return provider ? `Not signed in to ${provider}, or the sign-in has expired.` : "Not signed in, or the sign-in has expired.";
}

// yusAgent (YA-10, ticket 31, X3) : la notice d'une annonce du moteur, `session_info_update` avec
// `_meta["_yus/notice"] = { kind: "resources", details }` (`crates/pa-daemon/src/acp/ya_notice.rs`),
// en direct comme au rejeu de `session/load`. Elle devient des lignes visibles du tour qu'elle
// accompagne, au-dessus de son message, calculées de `details` seulement (jamais du texte reçu par
// le modèle). Textes du §3.2 (décision C2 du 24/11/2026) : un seul texte partout, skills d'abord,
// puis MCP ; dans chacun `added`, `changed`, `removed`, une partie vide n'est pas écrite.

export const NOTICE_META = "_yus/notice";

type Names = unknown;

function names(list: Names): string[] {
  return Array.isArray(list) ? list.filter((name): name is string => typeof name === "string") : [];
}

function line(what: string, parts: [string, string[]][]): string | undefined {
  const written = parts.filter(([, list]) => list.length > 0).map(([verb, list]) => `${verb} ${list.join(", ")}`);
  return written.length > 0 ? `${what} updated: ${written.join("; ")}. Sent to the agent.` : undefined;
}

/** Les lignes visibles d'une notice du `_meta` d'une mise à jour ; `undefined` sans notice. */
export function noticeLines(meta: unknown): string[] | undefined {
  const notice = (meta as Record<string, unknown> | undefined)?.[NOTICE_META] as
    | { kind?: unknown; details?: { skills?: Record<string, Names>; mcp?: Record<string, Names> } }
    | undefined;
  if (!notice || typeof notice !== "object" || notice.kind !== "resources") return undefined;
  const skills = notice.details?.skills ?? {};
  const mcp = notice.details?.mcp ?? {};
  return [
    line("Skills", [
      ["added", names(skills.added)],
      ["changed", names(skills.changed)],
      ["removed", names(skills.removed)],
    ]),
    line("MCP servers", [
      ["added", names(mcp.added)],
      ["removed", names(mcp.removed)],
    ]),
  ].filter((text): text is string => text !== undefined);
}

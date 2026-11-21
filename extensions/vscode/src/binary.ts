// Résolution du réglage `yusagent.binaryPath` (E4) en chemin absolu : `~/` développé, nom nu
// cherché dans le `PATH` du processus de VS Code. Le chemin résolu est celui qu'on lance et
// celui qu'affichent les messages (binaire absent, `/login`).

import { accessSync, constants, statSync } from "node:fs";
import { homedir } from "node:os";
import { delimiter, isAbsolute, join, resolve } from "node:path";

export const DEFAULT_BINARY = "yusagent";

export type Resolution =
  | { ok: true; path: string }
  | { ok: false; reason: "not_found" | "not_executable"; tried: string };

function executable(path: string): "ok" | "missing" | "not_executable" {
  try {
    if (!statSync(path).isFile()) return "missing";
  } catch {
    return "missing";
  }
  try {
    accessSync(path, constants.X_OK);
    return "ok";
  } catch {
    return "not_executable";
  }
}

export function resolveBinary(
  configured: string | undefined,
  env: NodeJS.ProcessEnv,
  cwd: string | undefined,
): Resolution {
  const raw = (configured ?? "").trim() || DEFAULT_BINARY;
  const expanded = raw === "~" ? homedir() : raw.startsWith("~/") ? join(homedir(), raw.slice(2)) : raw;
  if (expanded.includes("/")) {
    const path = isAbsolute(expanded) ? expanded : resolve(cwd ?? process.cwd(), expanded);
    const state = executable(path);
    if (state === "ok") return { ok: true, path };
    return { ok: false, reason: state === "missing" ? "not_found" : "not_executable", tried: path };
  }
  for (const dir of (env.PATH ?? "").split(delimiter)) {
    if (!dir) continue;
    const candidate = join(dir, expanded);
    if (executable(candidate) === "ok") return { ok: true, path: candidate };
  }
  return { ok: false, reason: "not_found", tried: `${expanded} (cherché dans le PATH)` };
}

export function describeFailure(resolution: Exclude<Resolution, { ok: true }>): string {
  return resolution.reason === "not_executable"
    ? `Binaire yusAgent non exécutable : ${resolution.tried}. Vérifiez le réglage yusagent.binaryPath.`
    : `Binaire yusAgent introuvable : ${resolution.tried}. Vérifiez le réglage yusagent.binaryPath.`;
}

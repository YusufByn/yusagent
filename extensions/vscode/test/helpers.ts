// Outils communs aux tests unitaires et e2e : agent factice, fixture des trames, attente.

import { chmodSync, mkdtempSync, readFileSync, readdirSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

/** Racine de l'extension (les tests compilés sont sous `out/test/`). */
export const EXTENSION_ROOT = resolve(__dirname, "..", "..");
export const FAKE_AGENT = join(EXTENSION_ROOT, "test", "fake-agent", "fake-agent.mjs");
export const FIXTURE = join(EXTENSION_ROOT, "test", "fixtures", "acp-frames.json");

export function fakeAgentPath(): string {
  chmodSync(FAKE_AGENT, 0o755);
  return FAKE_AGENT;
}

export function newControlDir(scenario: object = {}): string {
  const dir = mkdtempSync(join(tmpdir(), "yusagent-fake-"));
  writeFileSync(join(dir, "scenario.json"), JSON.stringify(scenario));
  return dir;
}

export function setScenario(dir: string, scenario: object): void {
  writeFileSync(join(dir, "scenario.json"), JSON.stringify(scenario));
}

export interface Start {
  pid: number;
  cwd: string;
  argv: string[];
  /** PR 3 : `PRIME_AGENT_KERNEL_PYTHON` et `PRIME_AGENT_KERNEL_VENV` reçues par l'agent factice. */
  kernelPython?: string | null;
  kernelVenv?: string | null;
}

export function starts(dir: string): Start[] {
  const path = join(dir, "starts.log");
  if (!existsSync(path)) return [];
  return readFileSync(path, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line) as Start);
}

/** Trames reçues par l'agent factice lancé avec ce pid (lignes brutes analysées). */
export function received(dir: string, pid: number): Record<string, unknown>[] {
  const path = join(dir, `received-${pid}.jsonl`);
  if (!existsSync(path)) return [];
  return readFileSync(path, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line) as Record<string, unknown>);
}

export function receivedAll(dir: string): Record<string, unknown>[] {
  return readdirSync(dir)
    .filter((name) => name.startsWith("received-"))
    .flatMap((name) =>
      readFileSync(join(dir, name), "utf8")
        .split("\n")
        .filter(Boolean)
        .map((line) => JSON.parse(line) as Record<string, unknown>),
    );
}

export function fixtureFrames(): unknown[] {
  return (JSON.parse(readFileSync(FIXTURE, "utf8")) as { frames: unknown[] }).frames;
}

/** Remplace les valeurs propres au lancement par les jetons du fixture. */
export function normalize(
  frames: unknown[],
  values: { cwd: string; sessionId: string; prompt: string },
): unknown[] {
  const tokens = new Map<string, string>([
    [values.cwd, "<cwd>"],
    [values.sessionId, "<sessionId>"],
    [values.prompt, "<prompt>"],
  ]);
  const walk = (value: unknown): unknown => {
    if (typeof value === "string") return tokens.get(value) ?? value;
    if (Array.isArray(value)) return value.map(walk);
    if (value && typeof value === "object") {
      return Object.fromEntries(Object.entries(value).map(([key, inner]) => [key, walk(inner)]));
    }
    return value;
  };
  return frames.map(walk);
}

export async function waitFor<T>(
  probe: () => T | undefined | false | Promise<T | undefined | false>,
  what: string,
  timeoutMs = 10_000,
): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const value = await probe();
    if (value) return value;
    if (Date.now() > deadline) throw new Error(`délai dépassé : ${what}`);
    await new Promise((resolveWait) => setTimeout(resolveWait, 25));
  }
}

/**
 * Ticket 31 (X3, C5) : la valeur de l'option `permissions` d'une configuration de session, qu'elle
 * soit une chaîne (avant le sélecteur) ou `{ current, choices }` (libellés lus dans l'option du pont).
 */
export function permissionsOf(config: unknown): string | null {
  const value = (config as { permissions?: unknown } | null | undefined)?.permissions;
  if (typeof value === "string") return value;
  const current = (value as { current?: unknown } | null | undefined)?.current;
  return typeof current === "string" ? current : null;
}

/** Ticket 31 (X3) : les lignes visibles des annonces d'un tour (`_yus/notice`), au-dessus de son message. */
export function noticesOf(turn: unknown): string[] | undefined {
  return (turn as { notices?: string[] } | null | undefined)?.notices;
}

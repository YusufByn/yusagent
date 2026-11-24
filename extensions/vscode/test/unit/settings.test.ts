// yusAgent (YA-10, ticket 31, X3) : les écrans skills, MCP et réglages contre l'agent factice, sur les
// méthodes `_yus/*` du pont (`crates/pa-daemon/src/acp/ya_resources.rs`), formes relevées par
// `bench/tools/capture/acp_frames.py` (cas C-resources). Rien n'attend un redémarrage ; le journal
// ne garde jamais la valeur de `mcpServers` (règle 9).

import assert from "node:assert/strict";
import { mkdtempSync, realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, test } from "node:test";

import { Conversations } from "../../src/conversations";
import type { Files } from "../../src/files";
import { Settings } from "../../src/settings";
import { ShownAgent } from "../../src/shown";
import { fakeAgentPath, newControlDir, received, starts, waitFor } from "../helpers";

const PERMISSIONS_REFUSAL =
  "permissions is not set here: change it for this conversation with the permissions menu, or in ~/.yusagent/settings.json for every new session.";

const opened: Conversations[] = [];
afterEach(async () => {
  await Promise.all(opened.splice(0).map((conversations) => conversations.stopAll()));
});

interface Harness {
  settings: Settings;
  conversations: Conversations;
  dir: string;
  lines: string[];
}

function open(): Harness {
  const dir = newControlDir({});
  process.env.FAKE_ACP_DIR = dir;
  const cwd = realpathSync(mkdtempSync(join(tmpdir(), "yusagent-ws-")));
  const lines: string[] = [];
  // Le journal de l'extension : chaque ligne gardée, pour vérifier ce qui y est écrit.
  const log = new Proxy({} as Record<string, unknown>, {
    get: (_target, level) => (...args: unknown[]) => lines.push(`${String(level)}: ${args.map(String).join(" ")}`),
  }) as never;
  const shown = new ShownAgent(log);
  const settings = new Settings(shown, log);
  const files = { cwd, links: () => [], list: async () => ({ files: [], open: [] }) } as unknown as Files;
  const conversations = new Conversations(fakeAgentPath(), cwd, "0", files, shown, log);
  opened.push(conversations);
  conversations.start();
  return { settings, conversations, dir, lines };
}

const ready = (h: Harness) => waitFor(() => h.conversations.shown.session.current?.kind === "open", "session ouverte");
const frames = (h: Harness) => received(h.dir, starts(h.dir).at(-1)!.pid);
const methods = (h: Harness) => frames(h).map((frame) => frame.method);

test("écrans (ticket 31) : refresh lit _yus/settings/get, _yus/skills/list et _yus/mcp/list, aux formes du pont", async () => {
  const h = open();
  await ready(h);
  await h.settings.refresh();
  for (const method of ["_yus/settings/get", "_yus/skills/list", "_yus/mcp/list"]) {
    assert.ok(methods(h).includes(method), `${method} envoyée`);
  }
  const current = h.settings.current;
  assert.equal(current.view?.defaults["compaction.enabled"], true, "réglages lus");
  assert.ok(current.skills?.skills.some((skill) => skill.name === "edit"), "skills lus");
  assert.ok(current.mcp !== null, "serveurs MCP lus");
});

test("interrupteur d'un skill (ticket 31) : _yus/skills/set, réponse appliquée, aucun « restart »", async () => {
  const h = open();
  await ready(h);
  await h.settings.refresh();
  const path = join(h.dir, "builtin", "edit", "SKILL.md");
  const error = await h.settings.setSkill(path, false);
  assert.equal(error, undefined, "accepté");
  const sets = frames(h).filter((frame) => frame.method === "_yus/skills/set");
  assert.deepEqual(sets.map((frame) => frame.params), [{ path, enabled: false }]);
  assert.equal(h.settings.current.skills?.skills.find((skill) => skill.path === path)?.enabled, false);
  assert.deepEqual(h.lines.filter((line) => /restart/i.test(line)), [], "rien n'attend un redémarrage");
});

test("journal (ticket 31, règle 9) : mcpServers écrit sans sa valeur", async () => {
  const h = open();
  await ready(h);
  const servers = { "srv-x3": { type: "http", url: "http://127.0.0.1:9/mcp", headers: { Authorization: "Bearer SECRET-X3" } } };
  const error = await h.settings.set("mcpServers", servers);
  assert.equal(error, undefined, "accepté");
  assert.ok(h.lines.some((line) => line.includes("settings: mcpServers set")), "écriture journalisée");
  assert.deepEqual(h.lines.filter((line) => line.includes("SECRET-X3") || line.includes("127.0.0.1")), [], "valeur absente du journal");
});

test("refus de permissions (ticket 31, D1) : le texte du pont sous le réglage", async () => {
  const h = open();
  await ready(h);
  assert.equal(await h.settings.set("permissions", "manual"), PERMISSIONS_REFUSAL);
});

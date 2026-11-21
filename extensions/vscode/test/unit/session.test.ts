// ChatSession contre l'agent factice (vrai processus, vraies trames sur stdio), sans VS Code.

import assert from "node:assert/strict";
import { mkdtempSync, realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, test } from "node:test";

import { ChatSession, isAuthFailure } from "../../src/session";
import { autoWithoutSandbox, TEXTS, type ChatState } from "../../src/shared/state";
import {
  fakeAgentPath,
  fixtureFrames,
  newControlDir,
  normalize,
  received,
  starts,
  waitFor,
} from "../helpers";

interface Harness {
  session: ChatSession;
  dir: string;
  cwd: string;
  logs: string[];
  clock: { now: number };
}

// Chaque session est fermée après son test, même en échec : sinon l'agent factice vivant
// empêche `node --test` de rendre la main.
const opened: ChatSession[] = [];
afterEach(async () => {
  await Promise.all(opened.splice(0).map((session) => session.dispose()));
});

function open(
  scenario: object = {},
  overrides: { binaryPath?: string; cwd?: string | null; autostart?: boolean } = {},
): Harness {
  const dir = newControlDir(scenario);
  const cwd = overrides.cwd === null ? undefined : (overrides.cwd ?? realpathSync(mkdtempSync(join(tmpdir(), "yusagent-ws-"))));
  const logs: string[] = [];
  const clock = { now: 1_000_000 };
  const session = new ChatSession({
    binaryPath: overrides.binaryPath ?? fakeAgentPath(),
    cwd,
    workspaceName: "ws",
    env: { ...process.env, FAKE_ACP_DIR: dir },
    log: (line) => logs.push(line),
    onState: () => undefined,
    now: () => clock.now,
  });
  opened.push(session);
  if (overrides.autostart !== false) void session.start();
  return { session, dir, cwd: cwd ?? "", logs, clock };
}

const ready = (h: Harness) => waitFor(() => h.session.state.phase === "ready", "session prête");
const methods = (h: Harness, pid: number) => received(h.dir, pid).map((frame) => frame.method ?? `réponse:${String(frame.id)}`);
const lastStart = (h: Harness) => starts(h.dir).at(-1)!;
const texts = (state: ChatState, kind: string) =>
  state.items.filter((item) => item.kind === kind).map((item) => ("text" in item ? item.text : ""));

test("ouverture puis prompt : exactement les trames du fixture (preuve 1), streaming affiché", async () => {
  const h = open();
  await ready(h);
  const { pid, cwd, argv } = lastStart(h);
  assert.deepEqual(argv, ["--mode", "acp"]);
  assert.equal(cwd, h.cwd, "processus lancé dans le dossier du workspace");
  assert.deepEqual(methods(h, pid), ["initialize", "session/new"], "rien d'autre à l'ouverture (E7)");
  await h.session.send("bonjour");
  assert.deepEqual(texts(h.session.state, "agent"), ["Bonjour"]);
  assert.deepEqual(
    normalize(received(h.dir, pid), { cwd: h.cwd, sessionId: "fake-session-1", prompt: "bonjour" }),
    fixtureFrames(),
  );
  await h.session.dispose();
});

test("Arrêter : session/cancel puis cancelled à la demande d'accord ouverte", async () => {
  const h = open();
  await ready(h);
  const turn = h.session.send("lent");
  await waitFor(() => h.session.state.items.some((item) => item.kind === "permission"), "demande affichée");
  h.session.stop();
  await turn;
  const frames = received(h.dir, lastStart(h).pid);
  assert.ok(frames.some((frame) => frame.method === "session/cancel"));
  assert.deepEqual(
    frames.find((frame) => frame.id === "ya-perm-1"),
    { jsonrpc: "2.0", id: "ya-perm-1", result: { outcome: { outcome: "cancelled" } } },
  );
  const state = h.session.state;
  assert.equal(state.phase, "ready");
  assert.ok(state.items.some((item) => item.kind === "tool" && item.status === "cancelled"));
  assert.ok(!state.items.some((item) => item.kind === "permission"));
  assert.ok(state.items.some((item) => item.kind === "notice" && item.text === TEXTS.cancelled));
  await h.session.dispose();
});

test("$/cancel_request de l'agent : réponse cancelled, ligne retirée, le tour continue", async () => {
  const h = open();
  await ready(h);
  await h.session.send("retire");
  const frames = received(h.dir, lastStart(h).pid);
  assert.deepEqual(
    frames.find((frame) => frame.id === "ya-perm-1"),
    { jsonrpc: "2.0", id: "ya-perm-1", result: { outcome: { outcome: "cancelled" } } },
  );
  assert.ok(!h.session.state.items.some((item) => item.kind === "permission"));
  assert.ok(h.session.state.items.some((item) => item.kind === "tool" && item.status === "completed"));
  assert.deepEqual(texts(h.session.state, "agent"), ["retirée"]);
  await h.session.dispose();
});

test("sélecteurs : set_config_option seulement sur un changement", async () => {
  const h = open();
  await ready(h);
  await h.session.setConfig("thought_level", "high");
  await h.session.setConfig("thought_level", "xhigh");
  await h.session.setConfig("model", "capture/model-b");
  const sets = received(h.dir, lastStart(h).pid).filter((frame) => frame.method === "session/set_config_option");
  assert.deepEqual(sets.map((frame) => frame.params), [
    { sessionId: "fake-session-1", configId: "thought_level", value: "xhigh" },
    { sessionId: "fake-session-1", configId: "model", value: "capture/model-b" },
  ]);
  const current = Object.fromEntries(h.session.state.configOptions.map((option) => [option.id, option.currentValue]));
  assert.deepEqual(current, { model: "capture/model-b", thought_level: "xhigh", permissions: "manual" });
  await h.session.dispose();
});

test("bandeau : valeur annoncée à l'ouverture, puis config_option_update", async () => {
  const auto = open({ permissions: "auto" });
  await ready(auto);
  assert.equal(autoWithoutSandbox(auto.session.state), true);
  await auto.session.dispose();
  const manual = open();
  await ready(manual);
  assert.equal(autoWithoutSandbox(manual.session.state), false);
  await manual.session.send("auto");
  assert.equal(autoWithoutSandbox(manual.session.state), true);
  await manual.session.dispose();
});

test("authentification : message fixe avec le chemin réel du binaire", async () => {
  const h = open();
  await ready(h);
  await h.session.send("auth");
  const last = h.session.state.items.at(-1);
  assert.ok(last?.kind === "auth");
  assert.equal(last.binaryPath, fakeAgentPath());
  await h.session.dispose();
});

test("identifiant absent (kind other, forme du vrai binaire) : même message /login", async () => {
  const h = open();
  await ready(h);
  await h.session.send("sanscle");
  const last = h.session.state.items.at(-1);
  assert.ok(last?.kind === "auth");
  assert.equal(last.binaryPath, fakeAgentPath());
});

test("isAuthFailure : kind authentication ou texte d'identifiant absent, rien d'autre", () => {
  assert.equal(isAuthFailure({ kind: "authentication" }), true);
  assert.equal(isAuthFailure({ kind: "other", details: "yusagent turn failed: No API key found for anthropic.\n\nUse /login" }), true);
  assert.equal(isAuthFailure({ kind: "other", details: "Provider server error" }), false);
  assert.equal(isAuthFailure({ kind: "rate_limit" }), false);
  assert.equal(isAuthFailure(undefined), false);
});

test("plantage : relance avec nouvelle session, puis arrêt au deuxième en moins de 60 s, Réessayer", async () => {
  const h = open();
  await ready(h);
  await h.session.send("crash").catch(() => undefined);
  await waitFor(() => starts(h.dir).length === 2 && h.session.state.phase === "ready", "relance");
  const state = h.session.state;
  assert.ok(state.staleBefore > 0, "historique grisé");
  assert.ok(state.items.some((item) => item.kind === "notice" && item.text === TEXTS.restarting("code 3")));
  assert.deepEqual(methods(h, lastStart(h).pid), ["initialize", "session/new"]);
  h.clock.now += 30_000;
  await h.session.send("crash").catch(() => undefined);
  await waitFor(() => h.session.state.phase === "stopped", "arrêt");
  assert.equal(starts(h.dir).length, 2, "pas de troisième lancement automatique");
  await h.session.retry();
  await ready(h);
  assert.equal(starts(h.dir).length, 3);
  await h.session.dispose();
});

test("plantages espacés de plus de 60 s : relance à chaque fois", async () => {
  const h = open();
  await ready(h);
  await h.session.send("crash").catch(() => undefined);
  await waitFor(() => starts(h.dir).length === 2 && h.session.state.phase === "ready", "première relance");
  h.clock.now += 61_000;
  await h.session.send("crash").catch(() => undefined);
  await waitFor(() => starts(h.dir).length === 3 && h.session.state.phase === "ready", "deuxième relance");
  await h.session.dispose();
});

test("binaire absent : aucun lancement, message avec le chemin", async () => {
  const h = open({}, { binaryPath: "/nulle-part/yusagent", autostart: false });
  await h.session.start();
  assert.equal(h.session.state.phase, "stopped");
  const last = h.session.state.items.at(-1);
  assert.ok(last?.kind === "notice" && last.text.includes("/nulle-part/yusagent"));
  assert.deepEqual(last.kind === "notice" && last.actions, ["openSetting", "retry"]);
  assert.equal(starts(h.dir).length, 0);
});

test("aucun dossier ouvert : pas de lancement", async () => {
  const h = open({}, { cwd: null, autostart: false });
  await h.session.start();
  assert.equal(h.session.state.phase, "stopped");
  assert.ok(h.session.state.items.some((item) => item.kind === "notice" && item.text === TEXTS.noWorkspace));
  assert.equal(starts(h.dir).length, 0);
});

test("version ACP inattendue : ouverture refusée, processus arrêté", async () => {
  const h = open({ protocolVersion: 2 }, { autostart: false });
  await h.session.start();
  await waitFor(() => h.session.state.phase === "stopped", "refus");
  const last = h.session.state.items.at(-1);
  assert.ok(last?.kind === "notice" && last.text.includes("version ACP 2"));
  assert.deepEqual(methods(h, lastStart(h).pid), ["initialize"]);
});

test("fermeture : session/close, puis fin du processus sans relance", async () => {
  const h = open();
  await ready(h);
  const { pid } = lastStart(h);
  await h.session.dispose();
  assert.equal(methods(h, pid).at(-1), "session/close");
  await waitFor(() => {
    try {
      process.kill(pid, 0);
      return false;
    } catch {
      return true;
    }
  }, "fin du processus");
  assert.equal(starts(h.dir).length, 1);
  assert.equal(h.session.state.phase, "closed");
});

import assert from "node:assert/strict";
import { test } from "node:test";

import type { ConfigOption } from "../../src/acp/types";
import {
  autoWithoutSandbox,
  initialState,
  reduce,
  selectors,
  TEXTS,
  type Action,
  type ChatState,
} from "../../src/shared/state";

const options = (permissions?: string): ConfigOption[] => [
  { id: "model", name: "Model", type: "select", category: "model", currentValue: "a", options: [{ value: "a", name: "A" }] },
  { id: "thought_level", name: "Effort", type: "select", category: "thought_level", currentValue: "high", options: [{ value: "high", name: "high" }] },
  ...(permissions
    ? [{ id: "permissions", name: "Permissions", type: "select", category: "mode", currentValue: permissions, options: [] }]
    : []),
];

const run = (actions: Action[], state: ChatState = initialState("ws")) => actions.reduce(reduce, state);
const chunk = (text: string, kind = "agent_message_chunk"): Action => ({
  type: "update",
  update: { sessionUpdate: kind, content: { type: "text", text } },
});

test("streaming : les morceaux s'ajoutent au dernier message de l'agent", () => {
  const state = run([{ type: "started", configOptions: [] }, { type: "userPrompt", text: "salut" }, chunk("Bon"), chunk("jour"), chunk("…", "agent_thought_chunk"), chunk("!")]);
  assert.deepEqual(
    state.items.map((item) => [item.kind, "text" in item ? item.text : ""]),
    [["user", "salut"], ["agent", "Bonjour"], ["thought", "…"], ["agent", "!"]],
  );
  assert.equal(state.phase, "turn");
});

test("appels : statut suivi ; Arrêter annule les appels pas finis et retire les attentes", () => {
  let state = run([
    { type: "started", configOptions: [] },
    { type: "userPrompt", text: "x" },
    { type: "update", update: { sessionUpdate: "tool_call", toolCallId: "c1", title: "Python cell", status: "in_progress" } },
    { type: "update", update: { sessionUpdate: "tool_call_update", toolCallId: "c1", status: "completed" } },
    { type: "update", update: { sessionUpdate: "tool_call", toolCallId: "c2", title: "Python cell", status: "pending" } },
    { type: "permissionRequested", requestKey: "ya-perm-1", toolCallId: "c2" },
  ]);
  assert.ok(state.items.some((item) => item.kind === "permission"));
  state = reduce(state, { type: "stopRequested" });
  assert.equal(state.phase, "stopping");
  assert.deepEqual(state.items.filter((item) => item.kind === "tool").map((item) => item.kind === "tool" && item.status), ["completed", "cancelled"]);
  assert.ok(!state.items.some((item) => item.kind === "permission"));
  state = reduce(state, { type: "turnEnded", stopReason: "cancelled" });
  assert.equal(state.phase, "ready");
  assert.equal(state.items.at(-1)?.kind === "notice" && state.items.at(-1)?.kind, "notice");
});

test("bandeau : suit configOptions puis config_option_update, jamais autre chose", () => {
  let state = run([{ type: "started", configOptions: options("manual") }]);
  assert.equal(autoWithoutSandbox(state), false);
  state = reduce(state, { type: "update", update: { sessionUpdate: "config_option_update", configOptions: options("auto") } });
  assert.equal(autoWithoutSandbox(state), true);
  state = reduce(state, { type: "configOptions", configOptions: options("manual") });
  assert.equal(autoWithoutSandbox(state), false);
  assert.equal(autoWithoutSandbox(run([{ type: "started", configOptions: options() }])), false);
});

test("sélecteurs : modèle et effort seulement, par catégorie", () => {
  const state = run([{ type: "started", configOptions: options("auto") }]);
  assert.deepEqual(selectors(state).map((option) => option.id), ["model", "thought_level"]);
});

test("authentification : message fixe avec le chemin réel du binaire", () => {
  const state = run([{ type: "started", configOptions: [] }, { type: "userPrompt", text: "x" }, { type: "authFailed", binaryPath: "/opt/y/bin/yusagent" }]);
  const last = state.items.at(-1);
  assert.equal(last?.kind, "auth");
  assert.equal(TEXTS.auth("/opt/y/bin/yusagent"), "Authentification requise : lancez /opt/y/bin/yusagent dans un terminal, puis /login.");
  assert.equal(state.phase, "ready");
});

test("arrêt du processus : historique grisé, tour marqué échoué, relance ou arrêt", () => {
  const base = run([
    { type: "started", configOptions: options("auto") },
    { type: "userPrompt", text: "x" },
    { type: "update", update: { sessionUpdate: "tool_call", toolCallId: "c1", title: "Python cell", status: "in_progress" } },
  ]);
  const restarting = reduce(base, { type: "processExited", description: "code 3", restarting: true });
  assert.equal(restarting.phase, "starting");
  assert.deepEqual(restarting.configOptions, []);
  assert.equal(restarting.staleBefore, restarting.items.length - 1);
  assert.ok(restarting.items.some((item) => item.kind === "tool" && item.status === "failed"));
  assert.ok(restarting.items.some((item) => item.kind === "notice" && item.text === TEXTS.turnInterrupted));
  const last = restarting.items.at(-1);
  assert.ok(last?.kind === "notice" && last.text === TEXTS.restarting("code 3"));
  const stopped = reduce(restarting, { type: "processExited", description: "code 3", restarting: false });
  const final = stopped.items.at(-1);
  assert.equal(stopped.phase, "stopped");
  assert.ok(final?.kind === "notice" && final.actions.includes("retry"));
  // Un morceau après une relance ouvre un nouveau message (pas d'ajout à un message grisé).
  const after = run([{ type: "started", configOptions: [] }, chunk("A")], restarting);
  assert.ok(after.items.at(-1)?.kind === "agent");
  assert.equal(after.items.filter((item) => item.kind === "agent").length, 1);
});

test("binaire absent : arrêt avec Ouvrir le réglage et Réessayer", () => {
  const state = run([{ type: "spawnFailed", text: "Binaire yusAgent introuvable : /x" }]);
  const last = state.items.at(-1);
  assert.equal(state.phase, "stopped");
  assert.ok(last?.kind === "notice" && last.actions.join() === "openSetting,retry");
});

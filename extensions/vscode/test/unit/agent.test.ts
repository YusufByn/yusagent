// Le code repris d'Unharnessed, adapté (agent, session, chat, conversations, fournisseurs, réglages,
// historique), contre l'agent factice : vrai processus, vraies trames sur stdio, sans VS Code.

import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, test } from "node:test";

import { Agent } from "../../src/agent";
import { Conversations } from "../../src/conversations";
import type { Files } from "../../src/files";
import { History } from "../../src/history";
import type { ChatEntry, ConfigChange, TurnItem } from "../../src/protocol";
import { Providers } from "../../src/providers";
import { Settings } from "../../src/settings";
import { ShownAgent } from "../../src/shown";
import { yaMeta } from "../../src/ya/meta";
import { features } from "../../src/ya/methods";
import { fakeAgentPath, fixtureFrames, newControlDir, noticesOf, normalize, permissionsOf, received, starts, waitFor } from "../helpers";

const log = new Proxy({} as Record<string, unknown>, { get: () => () => undefined }) as never;
const files = { cwd: "", links: () => [], list: async () => ({ files: [], open: [] }) } as unknown as Files;

interface Harness {
  conversations: Conversations;
  dir: string;
  cwd: string;
}

const opened: Conversations[] = [];
afterEach(async () => {
  await Promise.all(opened.splice(0).map((conversations) => conversations.stopAll()));
});

function open(scenario: object = {}, overrides: { binary?: string; cwd?: string } = {}): Harness {
  const dir = newControlDir(scenario);
  process.env.FAKE_ACP_DIR = dir;
  const cwd = overrides.cwd ?? realpathSync(mkdtempSync(join(tmpdir(), "yusagent-ws-")));
  const shown = new ShownAgent(log);
  // Comme `activate` : ces modules écoutent l'agent montré et l'interrogent à son démarrage.
  new Providers(shown, log);
  new Settings(shown, log);
  const conversations = new Conversations(overrides.binary ?? fakeAgentPath(), cwd, "0", { ...files, cwd } as Files, shown, log);
  new History(shown, conversations, cwd, log);
  opened.push(conversations);
  conversations.start();
  return { conversations, dir, cwd };
}

const shown = (h: Harness) => h.conversations.shown;
const ready = (h: Harness) => waitFor(() => shown(h).session.current?.kind === "open", "session ouverte");
const idle = (h: Harness) => waitFor(() => !shown(h).chat.current.busy, "tour fini");
const lastPid = (h: Harness) => starts(h.dir).at(-1)!.pid;
const methods = (h: Harness) => received(h.dir, lastPid(h)).map((frame) => frame.method ?? `réponse:${String(frame.id)}`);
const items = (h: Harness): TurnItem[] =>
  shown(h).chat.current.entries.flatMap((entry) => (entry.kind === "turn" ? entry.items : []));
const texts = (h: Harness, kind: string) =>
  items(h).filter((item) => item.kind === kind).map((item) => ("text" in item ? item.text : ""));
const toolStatus = (h: Harness, id: string) =>
  items(h).find((item): item is Extract<TurnItem, { kind: "tool" }> => item.kind === "tool" && item.id === id)?.status;
const lastTurn = (h: Harness) => shown(h).chat.current.entries.filter((entry) => entry.kind === "turn").at(-1);

test("ouverture puis prompt : exactement les trames du fixture (preuve 1), rien d'autre à l'ouverture", async () => {
  const h = open();
  await ready(h);
  // Réglages, historique : rien ne part au démarrage. Ticket 40 (X5) : la liste des fournisseurs, oui (puce
  // « Connect a provider »), après `session/new` (écouteur de la session inscrit avant celui du panneau).
  await new Promise((resolve) => setTimeout(resolve, 200));
  assert.deepEqual(methods(h), ["initialize", "session/new", "_yus/providers/list"]);
  const { cwd, argv } = starts(h.dir).at(-1)!;
  assert.deepEqual(argv, ["--mode", "acp"]);
  assert.equal(cwd, h.cwd);
  shown(h).chat.send("bonjour", []);
  await idle(h);
  assert.deepEqual(texts(h, "text"), ["Bonjour"]);
  assert.equal((lastTurn(h) as { state?: string }).state, "done");
  assert.deepEqual(normalize(received(h.dir, lastPid(h)), { cwd: h.cwd, sessionId: "fake-session-1", prompt: "bonjour" }), fixtureFrames());
});

test("méthode non servie : rejet sur place, aucune trame", async () => {
  const h = open();
  await ready(h);
  const before = received(h.dir, lastPid(h)).length;
  // Ticket 28 : `session/list` est servie ; `_yus/session/rewind` ne l'est pas encore (noms du ticket 32).
  // Ticket 39 : `_yus/session/steer` est servie (tests « message pendant un tour » plus bas).
  await assert.rejects(shown(h).agent.request("_yus/session/rewind", {}), /pas encore servie/);
  shown(h).agent.notify("_yus/whatever", {});
  assert.equal(received(h.dir, lastPid(h)).length, before);
});

test("Arrêter : session/cancel, puis cancelled à la demande ouverte, demande retirée de la boîte", async () => {
  const h = open({ permissions: "manual" }); // ticket 30 : l'agent factice est en auto par défaut
  await ready(h);
  shown(h).chat.send("lent", []);
  await waitFor(() => shown(h).agent.permissions.waiting === 1, "demande ouverte");
  assert.deepEqual(shown(h).agent.permissions.asks, [
    {
      toolCallId: "call-2",
      options: [
        { optionId: "allow-once", name: "Allow", kind: "allow_once" },
        { optionId: "reject-once", name: "Reject", kind: "reject_once" },
      ],
    },
  ]);
  assert.equal(toolStatus(h, "call-2"), "pending", "carte en attente d'accord, pas en cours (défaut 3)");
  shown(h).chat.cancel();
  await idle(h);
  assert.equal(toolStatus(h, "call-2"), "pending", "jamais lancée : la carte n'est pas passée en cours");
  const frames = received(h.dir, lastPid(h));
  const cancel = frames.findIndex((frame) => frame.method === "session/cancel");
  const answer = frames.findIndex((frame) => frame.id === "ya-permission-1");
  assert.ok(cancel >= 0 && answer > cancel, "session/cancel puis la réponse");
  assert.deepEqual(frames[answer], { jsonrpc: "2.0", id: "ya-permission-1", result: { outcome: { outcome: "cancelled" } } });
  assert.deepEqual(shown(h).agent.permissions.asks, [], "demande retirée de la boîte");
  assert.ok(!texts(h, "notice").some((text) => text.includes("Demande d'autorisation")), "plus de ligne d'attente");
  assert.equal((lastTurn(h) as { state?: string }).state, "cancelled");
});

test("$/cancel_request du pont : réponse cancelled, le tour continue", async () => {
  const h = open({ permissions: "manual" }); // ticket 30 : l'agent factice est en auto par défaut
  await ready(h);
  shown(h).chat.send("retire", []);
  // Défaut 3 : `pending` à l'annonce, `running` seulement à `in_progress`.
  await waitFor(() => toolStatus(h, "call-3") === "pending", "carte en attente d'accord");
  await waitFor(() => toolStatus(h, "call-3") === "running", "carte en cours après in_progress");
  await idle(h);
  assert.equal(toolStatus(h, "call-3"), "done");
  assert.deepEqual(received(h.dir, lastPid(h)).find((frame) => frame.id === "ya-permission-1"), {
    jsonrpc: "2.0",
    id: "ya-permission-1",
    result: { outcome: { outcome: "cancelled" } },
  });
  assert.equal(shown(h).agent.permissions.waiting, 0);
  assert.deepEqual(texts(h, "text"), ["retirée"]);
});

test("cartes (PR 3, étape 2) : bash() réels, vrais diffs, sortie sans les blocs diff ; refus", async () => {
  const h = open();
  await ready(h);
  shown(h).chat.send("carte", []);
  await idle(h);
  const tool = items(h).find((item): item is Extract<TurnItem, { kind: "tool" }> => item.kind === "tool" && item.id === "call-c")!;
  assert.equal(tool.status, "done");
  assert.equal(tool.output, "Edited a.py", "les contenus diff ne s'ajoutent pas à la sortie");
  assert.deepEqual(tool.runs, [
    { command: "echo lu", exitCode: 0, output: "lu\n", durationMs: 6 },
    { command: "false", exitCode: 1, output: "", durationMs: 3 },
  ]);
  assert.deepEqual(
    tool.edits?.map(({ path, shown, created, lines }) => ({ path, shown, created, lines })),
    [
      { path: join(h.cwd, "src/a.py"), shown: "src/a.py", created: false, lines: [["-", "x = 1"], ["+", "x = 2"]] },
      { path: join(h.cwd, "neuf.txt"), shown: "neuf.txt", created: true, lines: [["+", "un"], ["+", "deux"]] },
    ],
  );
  assert.equal(tool.cell?.kind, "edit");
  assert.deepEqual(tool.cell?.files, ["src/a.py", "neuf.txt"]);
  assert.deepEqual([tool.cell?.diff?.added, tool.cell?.diff?.removed], [3, 1]);
  assert.equal(tool.declined, undefined);

  shown(h).chat.send("refus", []);
  await idle(h);
  const declined = items(h).find((item): item is Extract<TurnItem, { kind: "tool" }> => item.kind === "tool" && item.id === "call-r")!;
  assert.equal(declined.status, "failed");
  assert.equal(declined.declined, true);
});

test("boîte (PR 3, étape 3) : Allow → selected allow-once, la cellule tourne ; Reject → reject-once, Declined", async () => {
  const h = open({ permissions: "manual" }); // ticket 30 : l'agent factice est en auto par défaut
  await ready(h);
  const permissions = shown(h).agent.permissions;
  shown(h).chat.send("accord", []);
  await waitFor(() => permissions.waiting === 1, "demande ouverte");
  assert.equal(permissions.answer("call-a", "autre-option"), false, "option inconnue : ignorée");
  assert.equal(permissions.answer("call-x", "allow-once"), false, "appel inconnu : ignoré");
  assert.equal(permissions.waiting, 1, "toujours ouverte");
  assert.equal(permissions.answer("call-a", "allow-once"), true);
  await idle(h);
  const frames = received(h.dir, lastPid(h));
  assert.deepEqual(frames.find((frame) => frame.id === "ya-permission-1"), {
    jsonrpc: "2.0",
    id: "ya-permission-1",
    result: { outcome: { outcome: "selected", optionId: "allow-once" } },
  });
  assert.equal(toolStatus(h, "call-a"), "done");
  assert.equal(permissions.answer("call-a", "allow-once"), false, "déjà réglée : rien ne part");
  assert.equal(received(h.dir, lastPid(h)).filter((frame) => frame.id === "ya-permission-1").length, 1, "une seule réponse");

  shown(h).chat.send("accord", []);
  await waitFor(() => permissions.waiting === 1, "deuxième demande");
  assert.equal(permissions.answer("call-a", "reject-once"), true);
  await idle(h);
  assert.deepEqual(received(h.dir, lastPid(h)).find((frame) => frame.id === "ya-permission-2"), {
    jsonrpc: "2.0",
    id: "ya-permission-2",
    result: { outcome: { outcome: "selected", optionId: "reject-once" } },
  });
  const declined = items(h).filter((item): item is Extract<TurnItem, { kind: "tool" }> => item.kind === "tool" && item.id === "call-a").at(-1)!;
  assert.equal(declined.status, "failed");
  assert.equal(declined.declined, true);
  assert.equal(permissions.waiting, 0);
});

test("diffs (PR 3, étape 4) : « Rejeter » restaure, retire le fichier créé, texte fixe au message suivant", async () => {
  const h = open();
  await ready(h);
  const a = join(h.cwd, "a.txt");
  const created = join(h.cwd, "neuf.txt");
  writeFileSync(a, "un\ndeux\ntrois\n");
  const tool = () => items(h).find((item): item is Extract<TurnItem, { kind: "tool" }> => item.kind === "tool" && item.id === "call-d");
  const states = () => tool()?.edits?.map((edit) => edit.revert);
  // Garde-fou 1 : pas pendant un tour (une demande d'accord tient le tour ouvert).
  shown(h).chat.send("diff", []);
  await idle(h);
  await waitFor(() => states()?.every((status) => status?.state === "ready"), "après lu, avant retrouvé");
  assert.equal(readFileSync(a, "utf8"), "un\nDEUX\ntrois\n");
  shown(h).chat.send("lent", []);
  await waitFor(() => shown(h).agent.permissions.waiting === 1, "un tour en cours");
  await shown(h).chat.yaRevert("call-d", a);
  assert.deepEqual(states()?.[0], { state: "ready", refused: "Not while a turn runs." });
  assert.equal(readFileSync(a, "utf8"), "un\nDEUX\ntrois\n", "rien d'écrit pendant le tour");
  shown(h).chat.cancel();
  await idle(h);
  // Hors tour : restauré ; le fichier créé est retiré.
  await shown(h).chat.yaRevert("call-d", a);
  await shown(h).chat.yaRevert("call-d", created);
  assert.deepEqual(states(), [{ state: "reverted" }, { state: "reverted" }]);
  assert.equal(readFileSync(a, "utf8"), "un\ndeux\ntrois\n");
  assert.equal(existsSync(created), false);
  // Une commande `/…` ne porte pas le texte fixe ; le message suivant, si.
  shown(h).chat.send("/rien", []);
  await idle(h);
  shown(h).chat.send("bonjour", []);
  await idle(h);
  const prompts = received(h.dir, lastPid(h))
    .filter((frame) => frame.method === "session/prompt")
    .map((frame) => (frame.params as { prompt: { text: string }[] }).prompt);
  assert.deepEqual(prompts.at(-2), [{ type: "text", text: "/rien" }]);
  assert.deepEqual(prompts.at(-1), [{ type: "text", text: "bonjour\n\n[user-reverted] a.txt\n[user-reverted] neuf.txt" }]);
  const last = lastTurn(h) as { prompt?: { text: string } };
  assert.equal(last.prompt?.text, "bonjour\n\n[user-reverted] a.txt\n[user-reverted] neuf.txt", "visible dans la bulle");
});

test("diffs (PR 3, étape 4) : edit() puis sed -i sur le même texte, « Rejeter » impossible avec la raison", async () => {
  const h = open();
  await ready(h);
  shown(h).chat.send("diffsed", []);
  await idle(h);
  const tool = () => items(h).find((item): item is Extract<TurnItem, { kind: "tool" }> => item.kind === "tool" && item.id === "call-d");
  await waitFor(() => tool()?.edits?.[0].revert?.state === "blocked", "rejet impossible");
  assert.deepEqual(tool()?.edits?.[0].revert, { state: "blocked", reason: "the edit's new text is no longer at line 2" });
  assert.deepEqual(tool()?.edits?.[1].revert, { state: "ready" }, "l'autre fichier de la cellule reste rejetable");
  await shown(h).chat.yaRevert("call-d", join(h.cwd, "a.txt"));
  assert.equal(readFileSync(join(h.cwd, "a.txt"), "utf8"), "un\nDeux\ntrois\n", "rien d'écrit");
});

test("environnement du binaire (PR 3) : PRIME_AGENT_KERNEL_PYTHON retirée, PRIME_AGENT_KERNEL_VENV passée", async () => {
  process.env.PRIME_AGENT_KERNEL_PYTHON = "/chemin/python-de-l-appelant";
  process.env.PRIME_AGENT_KERNEL_VENV = "/chemin/kernel-venv";
  try {
    const h = open();
    await ready(h);
    const start = starts(h.dir).at(-1)!;
    assert.equal(start.kernelPython, null, "PRIME_AGENT_KERNEL_PYTHON ne passe pas au binaire");
    assert.equal(start.kernelVenv, "/chemin/kernel-venv", "les autres variables passent");
    assert.equal(process.env.PRIME_AGENT_KERNEL_PYTHON, "/chemin/python-de-l-appelant", "l'environnement de VS Code n'est pas modifié");
  } finally {
    delete process.env.PRIME_AGENT_KERNEL_PYTHON;
    delete process.env.PRIME_AGENT_KERNEL_VENV;
  }
});

// Ticket 40 (X5, test 12, réponses 2 et 4 de Yusuf) : `kind: "authentication"` du pont (H2, R5) marque
// le tour avec le fournisseur du modèle de la session ; le panneau le nomme et mène à l'écran
// Providers. Le texte seul (`details`) ne compte plus.
test("authentification (ticket 40, X5) : tour marqué avec le fournisseur, texte fixe sans terminal ; le texte seul ne compte plus", async () => {
  const h = open();
  await ready(h);
  shown(h).chat.send("auth", []);
  await idle(h);
  const auth = lastTurn(h) as { state?: string; error?: string; auth?: unknown };
  assert.equal(auth.state, "failed");
  // D6 : le texte est figé à l'échec ; le fournisseur de la session est nommé (Capture est connecté).
  assert.equal(auth.error, "Not signed in to Capture, or the sign-in has expired.");
  assert.deepEqual(auth.auth, { provider: "capture" }, "le fournisseur du modèle de la session");
  shown(h).chat.send("sanscle", []);
  await idle(h);
  const other = lastTurn(h) as { state?: string; error?: string; auth?: unknown };
  assert.equal(other.state, "failed");
  assert.equal(other.auth, undefined, "kind other : pas un échec d'authentification");
  assert.match(other.error ?? "", /^yusagent turn failed: No API key found for amazon-bedrock\./, "le texte du pont");
});

test("plantage : pas de relance automatique, fin du stderr, Relancer", async () => {
  const h = open();
  await ready(h);
  shown(h).chat.send("crash", []);
  await waitFor(() => shown(h).agent.current.kind === "failed", "arrêt");
  const state = shown(h).agent.current;
  assert.ok(state.kind === "failed" && state.error.includes("code 3"));
  await new Promise((resolve) => setTimeout(resolve, 300));
  assert.equal(starts(h.dir).length, 1, "aucune relance automatique");
  await h.conversations.restart();
  await ready(h);
  assert.equal(starts(h.dir).length, 2);
});

test("arrêt : fin de stdin, sortie du processus, ni session/close ni signal", async () => {
  const h = open();
  await ready(h);
  const pid = lastPid(h);
  const began = Date.now();
  await shown(h).agent.stop();
  assert.ok(Date.now() - began < 10_000);
  await waitFor(() => {
    try {
      process.kill(pid, 0);
      return false;
    } catch {
      return true;
    }
  }, "fin du processus");
  assert.equal(methods(h).includes("session/close"), false);
});

test("binaire absent, aucun dossier, version ACP inattendue : pas de session", async () => {
  const absent = open({}, { binary: "/nulle-part/yusagent" });
  await waitFor(() => shown(absent).agent.current.kind === "failed", "binaire absent");
  const a = shown(absent).agent.current;
  assert.ok(a.kind === "failed" && a.error.includes("/nulle-part/yusagent"));
  assert.equal(starts(absent.dir).length, 0);

  const noFolder = open({}, { cwd: "" });
  await waitFor(() => shown(noFolder).agent.current.kind === "failed", "aucun dossier");
  const b = shown(noFolder).agent.current;
  assert.ok(b.kind === "failed" && b.error.startsWith("Ouvrez un dossier"));
  assert.equal(starts(noFolder.dir).length, 0);

  const version = open({ protocolVersion: 2 });
  await waitFor(() => shown(version).agent.current.kind === "failed", "version");
  const c = shown(version).agent.current;
  assert.ok(c.kind === "failed" && c.error.includes("ACP version 2"));
});

test("D4 : permissions annoncé à l'ouverture (auto par défaut, ticket 30), puis config_option_update", async () => {
  const auto = open();
  await ready(auto);
  const a = shown(auto).session.current;
  assert.ok(a?.kind === "open" && permissionsOf(a.config) === "auto");

  const manual = open({ permissions: "manual" });
  await ready(manual);
  const b = shown(manual).session.current;
  assert.ok(b?.kind === "open" && permissionsOf(b.config) === "manual");
  shown(manual).chat.send("auto", []);
  await waitFor(() => {
    const s = shown(manual).session.current;
    return s?.kind === "open" && permissionsOf(s.config) === "auto";
  }, "auto annoncé");
});

test("sélecteurs : set_config_option seulement sur un changement", async () => {
  const h = open();
  await ready(h);
  await shown(h).session.setOption({ configId: "thought_level", value: "xhigh" });
  const sets = received(h.dir, lastPid(h)).filter((frame) => frame.method === "session/set_config_option");
  assert.deepEqual(sets.map((frame) => frame.params), [{ sessionId: "fake-session-1", configId: "thought_level", value: "xhigh" }]);
  const s = shown(h).session.current;
  assert.ok(s?.kind === "open" && s.config.effort?.current === "xhigh");
});

test("yaMeta : notre espace de noms, champs du pont tels quels (ticket 25 : turnStart vient du pont)", () => {
  const meta = (fields: object) => ({ "ai.primeintellect.prime-agent": { promptTurnId: 3, ...fields } });
  const start = { sessionUpdate: "session_info_update", _meta: meta({ phase: "event", turnStart: { origin: "wake", wake: "bash" } }) };
  assert.deepEqual(yaMeta(start), { promptTurnId: 3, phase: "event", turnStart: { origin: "wake", wake: "bash" } });
  // Plus de début déduit : un contenu sans `turnStart` n'ouvre pas de tour, une frontière ne porte pas de `turnEnd`.
  assert.deepEqual(yaMeta({ sessionUpdate: "agent_message_chunk", _meta: meta({ phase: "event" }) }), { promptTurnId: 3, phase: "event" });
  const boundary = { sessionUpdate: "session_info_update", _meta: meta({ phase: "responseBoundary", outcome: "result" }) };
  assert.deepEqual(yaMeta(boundary), { promptTurnId: 3, phase: "responseBoundary", outcome: "result" });
  // Ticket 32 : l'ancienne clé d'Unharnessed, que personne n'envoie, n'est plus lue.
  assert.deepEqual(yaMeta({ sessionUpdate: "x", _meta: { unharnessed: { promptTurnId: 1 } } }), {});
  assert.deepEqual(yaMeta({ sessionUpdate: "x" }), {});
});

test("tour lancé par l'agent (ticket 25) : ouvert par turnStart, fermé sur sa frontière, le panneau se libère", async () => {
  const h = open();
  await ready(h);
  shown(h).chat.send("réveil", []);
  const wake = await waitFor(() => {
    const turn = lastTurn(h);
    return turn?.kind === "turn" && turn.wake === "bash" && turn.state === "done" ? turn : undefined;
  }, "tour de réveil fini");
  assert.equal(wake.prompt, undefined, "un tour de l'agent n'a pas de message");
  await idle(h);
  const turns = shown(h).chat.current.entries.filter((entry) => entry.kind === "turn");
  assert.deepEqual(
    turns.map((turn) => (turn.kind === "turn" ? [turn.wake ?? null, turn.state] : [])),
    [[null, "done"], ["bash", "done"]],
  );
});

test("Agent seul : binaryPath = chemin résolu", async () => {
  const dir = newControlDir();
  const agent = new Agent("yusagent-introuvable-xyz", "/tmp", "0", log, { ...process.env, PATH: "/nulle-part", FAKE_ACP_DIR: dir });
  agent.start();
  assert.equal(agent.current.kind, "failed");
  assert.equal(agent.binaryPath, "yusagent-introuvable-xyz");
});

// Ticket 31 (X3, C5) : le sélecteur de permissions, par `session/set_config_option`, libellés lus
// dans l'option du pont (`crates/pa-daemon/src/acp/ya_permissions.rs`).
test("sélecteur de permissions (ticket 31) : set_config_option permissions, affichage immédiat, libellés du pont", async () => {
  const h = open();
  await ready(h);
  const change = { configId: "permissions", value: "manual" } as unknown as ConfigChange;
  const sent = shown(h).session.setOption(change);
  const before = shown(h).session.current;
  assert.equal(before?.kind === "open" ? permissionsOf(before.config) : null, "manual", "manual affiché avant la réponse");
  await sent;
  const sets = received(h.dir, lastPid(h)).filter((frame) => frame.method === "session/set_config_option");
  assert.deepEqual(sets.map((frame) => frame.params), [{ sessionId: "fake-session-1", configId: "permissions", value: "manual" }]);
  const after = shown(h).session.current;
  assert.equal(after?.kind === "open" ? permissionsOf(after.config) : null, "manual");
  const choices = after?.kind === "open" ? (after.config.permissions as unknown as { choices?: unknown })?.choices : undefined;
  assert.deepEqual(choices, [
    { value: "manual", name: "Ask before each tool call" },
    { value: "auto", name: "Run tool calls without asking (no sandbox)" },
  ]);
});

// Ticket 31 (X3, §3.2, décision C2) : la notice `_yus/notice` du pont devient une ligne visible du
// tour qu'elle accompagne, calculée de `details` : skills puis MCP, `added`, `changed`, `removed`.
test("notice (ticket 31) : lignes visibles du tour, texte du §3.2", async () => {
  const h = open();
  await ready(h);
  shown(h).chat.send("bonjour", []);
  await idle(h);
  const agent = shown(h).agent;
  // Changements entre deux prompts ; refusés sur place tant que les méthodes ne sont pas servies.
  await agent.request("_yus/skills/create", { name: "b-x3", description: "B." }).catch(() => undefined);
  await agent.request("_yus/skills/create", { name: "a-x3", description: "A." }).catch(() => undefined);
  await agent
    .request("_yus/skills/set", { path: join(h.dir, "builtin", "edit", "SKILL.md"), enabled: false })
    .catch(() => undefined);
  await agent
    .request("_yus/settings/set", { key: "mcpServers", value: { "srv-x3": { type: "http", url: "http://127.0.0.1:9/mcp" } } })
    .catch(() => undefined);
  shown(h).chat.send("bonjour", []);
  await idle(h);
  assert.deepEqual(noticesOf(lastTurn(h)), [
    "Skills updated: added a-x3, b-x3; removed edit. Sent to the agent.",
    "MCP servers updated: added srv-x3. Sent to the agent.",
  ]);
  // Le tour d'avant n'en a pas.
  const turns = shown(h).chat.current.entries.filter((entry) => entry.kind === "turn");
  assert.equal(noticesOf(turns.at(-2))?.length ?? 0, 0);
});

// Ticket 39 (X4) : un message pendant un tour (`_yus/session/steer`), contre l'agent factice, qui
// répond comme le pont (`crates/pa-daemon/src/acp/ya_turns/steer.rs`).
const framesOf = (h: Harness, method: string) => received(h.dir, lastPid(h)).filter((frame) => frame.method === method);
const promptTexts = (h: Harness) =>
  framesOf(h, "session/prompt").map((frame) =>
    ((frame.params as { prompt: { text?: string }[] }).prompt ?? []).map((block) => block.text ?? "").join(""),
  );
// En file, pas « en cours d'envoi » (la file montrée commence par les messages envoyés dans le tour).
const waitingIn = (h: Harness, text: string) =>
  shown(h).chat.current.queue.some((message) => message.text === text && message.state === "waiting");
// Le message est parti sur le fil, ou revenu en file (refusé) : l'assertion qui suit dit lequel.
const placed = (h: Harness, text: string, steers: number) =>
  waitFor(
    () => framesOf(h, "_yus/session/steer").length >= steers || waitingIn(h, text),
    `message « ${text} » parti ou en file`,
  );
const working = (h: Harness) => waitFor(() => toolStatus(h, "call-l") === "running", "cellule longue en cours");
const turnsOf = (h: Harness) =>
  shown(h).chat.current.entries.filter((entry): entry is Extract<ChatEntry, { kind: "turn" }> => entry.kind === "turn");

test("message pendant un tour : l'option « Send now » est servie (features().steer)", () => {
  assert.equal(features().steer, true);
});

test("message pendant un tour (Cmd+Entrée) : un steer avec l'identifiant du tour, tour rejoint, une seule trame session/prompt", async () => {
  const h = open();
  await ready(h);
  shown(h).chat.send("longue", []);
  await working(h);
  shown(h).chat.send("ajoute ceci", [], undefined, true);
  await placed(h, "ajoute ceci", 1);
  const steers = framesOf(h, "_yus/session/steer");
  assert.equal(steers.length, 1, "un steer part sur le fil");
  await idle(h);
  assert.deepEqual(steers[0].params, { sessionId: "fake-session-1", prompt: [{ type: "text", text: "ajoute ceci" }], turn: 1 });
  assert.deepEqual(promptTexts(h), ["longue"], "aucun doublon : une seule trame session/prompt");
  const turns = turnsOf(h);
  assert.equal(turns.length, 2, "le message ouvre un tour sous le premier");
  assert.equal(turns[1].prompt?.text, "ajoute ceci");
  assert.equal(turns[1].state, "done");
  assert.deepEqual(texts(h, "text").at(-1), "pris : ajoute ceci");
  assert.deepEqual(shown(h).chat.current.queue, []);
});

test("message pendant un tour refusé par le pont : en tête de file, envoyé en prompt après le tour", async () => {
  const h = open();
  await ready(h);
  shown(h).chat.send("longue", []);
  await working(h);
  shown(h).chat.send("refusé : plus tard", [], undefined, true);
  await waitFor(() => waitingIn(h, "refusé : plus tard"), "en file");
  assert.equal(framesOf(h, "_yus/session/steer").length, 1, "le steer est parti, le pont l'a refusé");
  shown(h).chat.send("ajoute ceci", [], undefined, true);
  await waitFor(() => promptTexts(h).length === 2, "le message refusé part en prompt");
  await idle(h);
  assert.equal(framesOf(h, "_yus/session/steer").length, 2, "le refusé puis l'accepté sont partis sur le fil");
  assert.deepEqual(promptTexts(h), ["longue", "refusé : plus tard"]);
});

test("Stop avec un message envoyé et non pris : file en pause, « play » l'envoie une fois", async () => {
  const h = open();
  await ready(h);
  shown(h).chat.send("longue", []);
  await working(h);
  shown(h).chat.send("pas encore pris", [], undefined, true);
  await placed(h, "pas encore pris", 1);
  assert.equal(framesOf(h, "_yus/session/steer").length, 1, "un steer part sur le fil");
  shown(h).chat.cancel();
  await idle(h);
  assert.equal(shown(h).chat.current.paused, true, "file en pause");
  assert.deepEqual(
    shown(h).chat.current.queue.map((message) => message.text),
    ["pas encore pris"],
  );
  assert.deepEqual(promptTexts(h), ["longue"]);
  shown(h).chat.resume();
  await waitFor(() => promptTexts(h).length === 2, "« play »");
  await idle(h);
  assert.deepEqual(promptTexts(h), ["longue", "pas encore pris"], "envoyé une fois");
});

test("tour en échec avec un message envoyé (E7) : renvoyé une fois en prompt, réponse affichée une fois", async () => {
  const h = open();
  await ready(h);
  shown(h).chat.send("échoue", []);
  await waitFor(() => toolStatus(h, "call-e") === "running", "cellule en cours");
  shown(h).chat.send("note après échec", [], undefined, true);
  await waitFor(() => promptTexts(h).length === 2, "renvoyé en prompt après l'échec");
  await idle(h);
  assert.deepEqual(promptTexts(h), ["échoue", "note après échec"], "renvoyé une fois");
  assert.equal(texts(h, "text").filter((text) => text === "écho : note après échec").length, 1, "réponse affichée une fois");
  assert.deepEqual(shown(h).chat.current.queue, []);
});

test("deux messages pendant un tour (E2) : le second ne part qu'après la réponse au premier, dans l'ordre", async () => {
  // Le pont répond après 200 ms ; le tour prend les messages 300 ms après le premier.
  const h = open({ steerDelayMs: 200 });
  await ready(h);
  shown(h).chat.send("longue", []);
  await working(h);
  shown(h).chat.send("un", [], undefined, true);
  shown(h).chat.send("deux", [], undefined, true);
  await new Promise((resolve) => setTimeout(resolve, 100));
  const texts = () =>
    framesOf(h, "_yus/session/steer").map((frame) =>
      ((frame.params as { prompt: { text?: string }[] }).prompt ?? []).map((block) => block.text ?? "").join(""),
    );
  assert.deepEqual(texts(), ["un"], "avant la réponse au premier, le second n'est pas parti");
  await idle(h);
  assert.deepEqual(texts(), ["un", "deux"], "dans l'ordre d'envoi");
  assert.deepEqual(
    turnsOf(h).slice(1).map((turn) => turn.prompt?.text),
    ["un", "deux"],
    "les tours rejoints, dans l'ordre",
  );
  assert.deepEqual(promptTexts(h), ["longue"], "aucun doublon");
});


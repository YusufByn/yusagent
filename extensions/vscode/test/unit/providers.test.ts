// yusAgent (YA-10, ticket 40, X5) : l'écran Providers et le login contre l'agent factice, sur les
// méthodes `_yus/providers/*` du pont de R5 (`crates/pa-daemon/src/acp/daemon/ya_providers.rs`),
// formes reprises dans `test/fake-agent/fake-agent.mjs`. Tests du §12 du ticket 40 (et §20) : 9, 10, 11,
// 13 (journal) et 14. Le journal de l'extension est lu : ni clé, ni URL de login, ni `state`, ni
// réponse d'une invite (règle 9, exigence de sécurité du §10).

import assert from "node:assert/strict";
import { mkdtempSync, realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, test } from "node:test";

import { Conversations } from "../../src/conversations";
import type { Files } from "../../src/files";
import { Providers } from "../../src/providers";
import { Settings } from "../../src/settings";
import { ShownAgent } from "../../src/shown";
import type { Provider, TurnHead } from "../../src/protocol";
import { authFailureText, authProviderName, authText } from "../../src/ya/auth";
import { features } from "../../src/ya/methods";
import { fakeAgentPath, newControlDir, received, starts, waitFor } from "../helpers";

/** Le `state` de l'URL de login de l'agent factice (`LOGIN_STATE`). */
const LOGIN_STATE = "x5-state-7d41c2";
const LOGIN_HOST = "auth.example.test";

const opened: Conversations[] = [];
afterEach(async () => {
  await Promise.all(opened.splice(0).map((conversations) => conversations.stopAll()));
});

interface Harness {
  providers: Providers;
  settings: Settings;
  conversations: Conversations;
  dir: string;
  lines: string[];
}

function open(scenario: object = {}): Harness {
  const dir = newControlDir(scenario);
  process.env.FAKE_ACP_DIR = dir;
  const cwd = realpathSync(mkdtempSync(join(tmpdir(), "yusagent-ws-")));
  const lines: string[] = [];
  // Le journal de l'extension (l'Output) : chaque ligne gardée, pour vérifier ce qui y est écrit.
  const log = new Proxy({} as Record<string, unknown>, {
    get: (_target, level) => (...args: unknown[]) => lines.push(`${String(level)}: ${args.map(String).join(" ")}`),
  }) as never;
  const shown = new ShownAgent(log);
  // Comme `activate` : avant la première conversation.
  const providers = new Providers(shown, log);
  const settings = new Settings(shown, log);
  const files = { cwd, links: () => [], list: async () => ({ files: [], open: [] }) } as unknown as Files;
  const conversations = new Conversations(fakeAgentPath(), cwd, "0", files, shown, log);
  opened.push(conversations);
  conversations.start();
  return { providers, settings, conversations, dir, lines };
}

const ready = (h: Harness) => waitFor(() => h.conversations.shown.session.current?.kind === "open", "session ouverte");
const frames = (h: Harness) => received(h.dir, starts(h.dir).at(-1)!.pid);
const sent = (h: Harness, method: string) => frames(h).filter((frame) => frame.method === method);
const settle = () => new Promise((resolve) => setTimeout(resolve, 300));

/** La liste lue à l'ouverture, ou `null` une fois l'ouverture passée sans elle. */
async function listed(h: Harness) {
  await ready(h);
  await settle();
  return h.providers.providers;
}

test("test 9 (ticket 40, X5) : features().providers vrai ; la liste est lue à l'ouverture, aucun fournisseur connecté", async () => {
  assert.equal(features().providers, true, "le panneau montre l'écran Providers");
  const h = open({ providers: "none" });
  const list = await listed(h);
  assert.equal(sent(h, "_yus/providers/list").length, 1, "_yus/providers/list envoyée une fois à l'ouverture");
  assert.ok(list, "liste lue");
  assert.ok(list.length > 0 && !list.some((row) => row.connected), `aucun fournisseur connecté : ${JSON.stringify(list)}`);
});

test("test 10 (ticket 40, X5) : login, URL et invite affichées, réponse relayée, fournisseur connecté", async () => {
  const h = open();
  await listed(h);
  const done = h.providers.startLogin("openai-codex");
  await waitFor(() => sent(h, "_yus/providers/login").length > 0 || h.providers.signIn?.error, "login envoyé ou refusé");
  assert.deepEqual(
    sent(h, "_yus/providers/login").map((frame) => frame.params),
    [{ providerId: "openai-codex" }],
    `login envoyé ; état : ${JSON.stringify(h.providers.signIn)}`,
  );
  const prompt = await waitFor(() => h.providers.signIn?.prompt, "invite affichée");
  assert.ok(h.providers.signIn?.url?.url.includes(`state=${LOGIN_STATE}`), "URL affichée");
  assert.equal(prompt.optional, true, "collage en course avec le navigateur");
  const answer = { kind: "text" as const, value: "http://localhost:1455/auth/callback?code=x5-code" };
  await h.providers.answer(prompt.promptId, answer);
  assert.equal(await done, true, "login réglé en done");
  assert.deepEqual(
    sent(h, "_yus/providers/login_answer").map((frame) => frame.params),
    [{ promptId: prompt.promptId, answer }],
    "réponse relayée",
  );
  assert.equal(h.providers.signIn, null, "écran du login fermé");
  const codex = h.providers.providers?.find((row) => row.id === "openai-codex");
  assert.equal(codex?.connected, true, "ChatGPT connecté");
});

test("test 10 (ticket 40, X5) : Cancel envoie login_cancel, le login se règle en cancelled", async () => {
  const h = open();
  await listed(h);
  const done = h.providers.startLogin("openai-codex");
  await waitFor(() => sent(h, "_yus/providers/login").length > 0 || h.providers.signIn?.error, "login envoyé ou refusé");
  assert.equal(sent(h, "_yus/providers/login").length, 1, `login envoyé ; état : ${JSON.stringify(h.providers.signIn)}`);
  await waitFor(() => h.providers.signIn?.url, "URL affichée");
  h.providers.cancelLogin();
  assert.equal(await done, false, "login non réglé en done");
  assert.equal(sent(h, "_yus/providers/login_cancel").length, 1, "login_cancel envoyée");
  assert.equal(h.providers.signIn, null, "écran du login fermé, sans erreur");
});

// Ticket 40 (X5) : la décision 5 est annulée (Yusuf, 26/11/2026) : la ligne Claude par abonnement
// est montrée comme les autres, à côté de la ligne Anthropic par clé.
test("test 11 (ticket 40, X5) : la ligne Anthropic par clé et la ligne Claude par abonnement sont montrées", async () => {
  const h = open();
  const list = await listed(h);
  assert.ok(list, "liste lue");
  const rows = (providers: typeof list) => providers.filter((row) => row.id === "anthropic").map((row) => row.method);
  assert.deepEqual(rows(list), ["apiKey", "subscription"], "à l'ouverture");
  const error = await h.providers.setApiKey("anthropic", "sk-x5-anthropic");
  assert.equal(error, undefined, "clé acceptée");
  assert.deepEqual(rows(h.providers.providers ?? []), ["apiKey", "subscription"], "après une clé");
});

test("test 13 (ticket 40, X5) : un jeton MCP part par _yus/mcp/set_token, jamais dans le journal", async () => {
  const h = open();
  await ready(h);
  const token = "zoom-x5-secret-91e2";
  const error = await h.settings.setToken("zoom", token);
  assert.equal(error, undefined, "jeton accepté");
  assert.deepEqual(sent(h, "_yus/mcp/set_token").map((frame) => frame.params), [{ id: "zoom", token }]);
  assert.ok(h.settings.current.mcp?.some((server) => server.id === "zoom" && server.signedIn), "service ajouté");
  assert.deepEqual(h.lines.filter((line) => line.includes(token)), [], "jeton absent du journal");
});

test("test 14 (ticket 40, X5) : le journal ne garde ni la clé, ni l'URL du login, ni son state, ni la réponse d'une invite", async () => {
  const h = open();
  await listed(h);
  const key = "sk-x5-secret-5b07a1";
  await h.providers.setApiKey("anthropic", key);
  const done = h.providers.startLogin("openai-codex");
  const prompt = await waitFor(() => h.providers.signIn?.prompt ?? (h.providers.signIn?.error ? "refusé" : undefined), "invite du login");
  const answer = "http://localhost:1455/auth/callback?code=x5-answer-0c3d";
  if (typeof prompt !== "string") await h.providers.answer(prompt.promptId, { kind: "text", value: answer });
  await done;
  assert.equal(sent(h, "_yus/providers/set_api_key").length, 1, "précondition : la clé est partie");
  assert.equal(sent(h, "_yus/providers/login_answer").length, 1, "précondition : la réponse est partie");
  for (const secret of [key, LOGIN_STATE, LOGIN_HOST, "x5-answer-0c3d"]) {
    assert.deepEqual(h.lines.filter((line) => line.includes(secret)), [], `${secret} absent du journal`);
  }
});

// Ticket 40 (X5, défaut 2 du contrôle de Yusuf) : sans aucun identifiant, le démon ouvre la session sur
// le premier modèle du catalogue (`amazon-bedrock`, `crates/pa-daemon/src/agent_engine/model.rs:41`). Le
// texte ne nomme pas ce fournisseur de repli tant qu'aucun fournisseur n'est connecté.
test("échec d'auth (ticket 40, X5, défaut 2) : sans fournisseur connecté, le texte ne nomme pas le fournisseur du modèle de repli", () => {
  const row = (id: string, name: string, connected: boolean): Provider => ({
    id,
    name,
    method: "apiKey",
    login: "apiKey",
    connected,
    removable: connected,
  });
  const none = [row("amazon-bedrock", "Amazon Bedrock", false), row("openai", "OpenAI", false)];
  assert.equal(authText(authProviderName(none, "amazon-bedrock")), "Not signed in, or the sign-in has expired.", "rien de connecté");
  const one = [row("amazon-bedrock", "Amazon Bedrock", false), row("openai", "OpenAI", true)];
  assert.equal(
    authText(authProviderName(one, "amazon-bedrock")),
    "Not signed in to Amazon Bedrock, or the sign-in has expired.",
    "un autre fournisseur connecté : le modèle de la session est nommé",
  );
  assert.equal(authText(authProviderName(one, null)), "Not signed in, or the sign-in has expired.", "fournisseur inconnu");
});

// Ticket 40 (X5, D6 du contrôle de Yusuf) : le texte sous un tour en échec d'authentification est celui
// de l'échec ; il ne change pas quand la liste des fournisseurs change ensuite (un login, une clé).
const lastFailed = (h: Harness) =>
  h.conversations.shown.chat.current.entries.filter((entry) => entry.kind === "turn").at(-1) as TurnHead;

async function failAuth(h: Harness): Promise<TurnHead> {
  h.conversations.shown.chat.send("auth", []);
  await waitFor(() => !h.conversations.shown.chat.current.busy && lastFailed(h)?.state === "failed", "tour en échec");
  return lastFailed(h);
}

test("D6 (ticket 40, X5) : le texte d'un échec d'auth ne change pas après une clé", async () => {
  const h = open({ providers: "none" });
  await listed(h);
  const turn = await failAuth(h);
  const before = authFailureText(turn);
  assert.equal(before, "Not signed in, or the sign-in has expired.", "aucun fournisseur connecté à l'échec");
  assert.equal(await h.providers.setApiKey("anthropic", "sk-x5-d6"), undefined, "clé acceptée");
  assert.equal(authFailureText(lastFailed(h)), before, "le même échec, après la clé");
});

test("D6 (ticket 40, X5) : un échec alors qu'un fournisseur est connecté nomme le fournisseur de la session", async () => {
  const h = open();
  await listed(h);
  const turn = await failAuth(h);
  assert.equal(authFailureText(turn), "Not signed in to Capture, or the sign-in has expired.");
});

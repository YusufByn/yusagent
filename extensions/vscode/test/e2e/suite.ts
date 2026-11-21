// Tests e2e dans l'hôte d'extension de VS Code, contre l'agent factice (ticket 24, PR I).
// Exécuteur minimal, sans dépendance : chaque test en série, échec = rejet de `run()`.

import assert from "node:assert/strict";
import * as vscode from "vscode";

import type { AgentState, ChatState, Probe, SessionState, WebviewMessage } from "../../src/protocol";
import { authText } from "../../src/ya/auth";
import { fakeAgentPath, fixtureFrames, normalize, received, setScenario, starts, waitFor } from "../helpers";

interface State {
  agent: AgentState;
  session: SessionState | null;
  chat: ChatState;
  html: string;
  rendered: boolean;
  badge?: number;
}

const control = process.env.FAKE_ACP_DIR as string;
const workspace = process.env.YUSAGENT_E2E_WORKSPACE as string;

const state = async () => (await vscode.commands.executeCommand<State>("yusagent._test.state"))!;
const post = (message: WebviewMessage) => vscode.commands.executeCommand("yusagent._test.post", message);
const probe = () => vscode.commands.executeCommand<Probe | undefined>("yusagent._test.probe");
const probeKeys = (keys: string[], focus: boolean) =>
  vscode.commands.executeCommand<Probe | undefined>("yusagent._test.probeKeys", keys, focus);
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const BOX = "Allow this Python cell to run?";
const lastPid = () => starts(control).at(-1)!.pid;

async function ready(): Promise<State> {
  return waitFor(async () => {
    const s = await state();
    return s.session?.kind === "open" && s.rendered ? s : undefined;
  }, "session ouverte et webview montée", 30_000);
}

async function idle(): Promise<void> {
  await waitFor(async () => !(await state()).chat.busy, "tour fini");
}

async function shows(text: string, what: string): Promise<string> {
  return waitFor(async () => {
    const shown = (await probe())?.text;
    return shown?.includes(text) ? shown : undefined;
  }, what);
}

async function permissionsIs(value: string): Promise<void> {
  await waitFor(async () => {
    const s = (await state()).session;
    return s?.kind === "open" && s.config.permissions === value;
  }, `permissions = ${value}`);
}

/** PR 3, étape 3 : la boîte de permission de bout en bout (agent factice, scénario `accord`). */
async function permissionBox(): Promise<void> {
  await post({ type: "send", text: "accord", images: [] });
  const box = await shows(BOX, "boîte de permission");
  for (const part of ["print('accord')", "Allow", "Reject"]) assert.ok(box.includes(part), `boîte : ${part}`);
  assert.equal((await state()).badge, 1, "badge sur l'icône de la vue");
  // Fenêtre de calme : `y` tout de suite ne fait rien (et la relance).
  assert.ok((await probeKeys(["y"], true))?.text.includes(BOX), "y pendant la fenêtre de calme : rien");
  await sleep(1_100);
  assert.ok((await probeKeys(["Enter"], true))?.text.includes(BOX), "Entrée : jamais une réponse");
  await sleep(1_100);
  assert.ok((await probeKeys(["y"], false))?.text.includes(BOX), "y sans le focus : rien");
  assert.equal(received(control, lastPid()).filter((frame) => frame.id === "ya-permission-2").length, 0, "aucune réponse partie");
  await probeKeys(["y"], true);
  await idle();
  assert.deepEqual(received(control, lastPid()).find((frame) => frame.id === "ya-permission-2"), {
    jsonrpc: "2.0",
    id: "ya-permission-2",
    result: { outcome: { outcome: "selected", optionId: "allow-once" } },
  });
  const done = await shows("autorisée", "la cellule a tourné");
  assert.ok(!done.includes(BOX), "boîte fermée après la réponse");
  assert.equal((await state()).badge, undefined, "badge retiré");
  // Esc refuse ; la carte dit « Declined » (le tour s'arrête, D7).
  await post({ type: "send", text: "accord", images: [] });
  await shows(BOX, "deuxième demande");
  await sleep(1_100);
  await probeKeys(["Escape"], true);
  await idle();
  assert.deepEqual(received(control, lastPid()).find((frame) => frame.id === "ya-permission-3")?.result, {
    outcome: { outcome: "selected", optionId: "reject-once" },
  });
  await shows("Declined", "carte refusée");
  // Le bouton « Reject » envoie le même message que la boîte.
  await post({ type: "send", text: "accord", images: [] });
  await shows(BOX, "troisième demande");
  await post({ type: "permission", toolCallId: "call-a", optionId: "reject-once" });
  await idle();
  assert.deepEqual(received(control, lastPid()).find((frame) => frame.id === "ya-permission-4")?.result, {
    outcome: { outcome: "selected", optionId: "reject-once" },
  });
}

const tests: [string, () => Promise<void>][] = [
  [
    "vue de la barre latérale : trames du fixture, CSP stricte, texte streamé affiché",
    async () => {
      setScenario(control, {});
      await vscode.commands.executeCommand("yusagent.open");
      const view = await ready();
      const csp = /<meta http-equiv="Content-Security-Policy" content="([^"]+)">/.exec(view.html)?.[1] ?? "";
      const directives = Object.fromEntries(
        csp.split("; ").map((part) => {
          const [name, ...sources] = part.split(" ");
          return [name, sources];
        }),
      );
      assert.deepEqual(Object.keys(directives), ["default-src", "style-src", "font-src", "img-src", "script-src"]);
      assert.deepEqual(directives["default-src"], ["'none'"]);
      const resources = directives["style-src"];
      assert.ok(
        resources.length > 0 && resources.every((source: string) => source === "'self'" || /^https:\/\/\*\.vscode-cdn\.net$/.test(source)),
        `sources des ressources : ${resources.join(" ")}`,
      );
      for (const name of ["style-src", "font-src", "img-src"]) assert.deepEqual(directives[name], resources);
      assert.match(directives["script-src"][0], /^'nonce-[A-Za-z0-9+/=]+'$/);
      assert.doesNotMatch(csp, /unsafe-|data:|blob:|(^|\s)\*(\s|;|$)/);
      console.log(`CSP : ${csp.replace(/'nonce-[^']+'/, "'nonce-…'")}`);
      assert.equal(starts(control).at(-1)!.cwd, workspace, "processus lancé dans le workspace");
      // Fournisseurs, réglages, historique : rien ne part à l'ouverture (méthodes non servies).
      assert.deepEqual(received(control, lastPid()).map((frame) => frame.method), ["initialize", "session/new"]);
      await post({ type: "send", text: "bonjour", images: [] });
      await idle();
      assert.deepEqual(normalize(received(control, lastPid()), { cwd: workspace, sessionId: "fake-session-1", prompt: "bonjour" }), fixtureFrames());
      const shown = await shows("Bonjour", "texte streamé affiché");
      assert.ok(shown.includes("bonjour"), "message de l'utilisateur affiché");
      assert.ok(!shown.includes("Mode auto sans sandbox"), "pas de bandeau en manuel");
      // PR 3, défauts 1 et 2 : aucun « Unharnessed » visible ; panneau du kernel masqué (non servi).
      assert.doesNotMatch(shown, /unharnessed/i);
      assert.ok(!shown.includes("Running servers") && !shown.includes("REPL variables"), "panneau du kernel masqué");
      assert.equal((await probe())?.input, "Ask yusAgent");
    },
  ],
  [
    "Arrêter : boîte ouverte, session/cancel puis cancelled, boîte fermée",
    async () => {
      await post({ type: "send", text: "lent", images: [] });
      await shows(BOX, "boîte de permission");
      // PR 3, défaut 3 : la carte attend l'accord, elle ne tourne pas.
      const waiting = await shows("Awaiting permission", "carte en attente d'accord");
      assert.ok(!waiting.includes("Running"), "pas de « Running » avant l'accord");
      await post({ type: "cancel" });
      await idle();
      const frames = received(control, lastPid());
      const cancel = frames.findIndex((frame) => frame.method === "session/cancel");
      const answer = frames.findIndex((frame) => frame.id === "ya-permission-1");
      assert.ok(cancel >= 0 && answer > cancel, "session/cancel puis la réponse");
      assert.deepEqual(frames[answer], { jsonrpc: "2.0", id: "ya-permission-1", result: { outcome: { outcome: "cancelled" } } });
      const after = await shows("Not run", "carte jamais lancée après Arrêter");
      assert.ok(!after.includes(BOX), "boîte fermée");
      assert.ok(!after.includes("Demande d'autorisation"), "plus de ligne d'attente");
    },
  ],
  [
    "boîte de permission (PR 3, étape 3) : code, badge, touches (calme de 1 s, focus, jamais Entrée), refus",
    async () => {
      try {
        await permissionBox();
      } finally {
        // Une demande restée ouverte bloquerait les tests suivants (tour sans fin).
        if ((await state()).chat.busy) {
          await post({ type: "cancel" });
          await idle();
        }
      }
    },
  ],
  [
    "cartes (PR 3, étape 2) : édition réelle, code de sortie, cellule refusée",
    async () => {
      await post({ type: "send", text: "carte", images: [] });
      await idle();
      // En-tête : type et fichiers d'après les vrais diffs, lignes changées, premier `bash()` en erreur.
      const shown = await shows("Edited", "carte d'une cellule qui édite");
      for (const part of ["a.py", "neuf.txt", "+3", "−1", "exit 1"]) assert.ok(shown.includes(part), `en-tête : ${part}`);
      await post({ type: "send", text: "refus", images: [] });
      await idle();
      await shows("Declined", "carte d'une cellule refusée");
    },
  ],
  [
    "sélecteurs : modèle affiché, set_config_option seulement sur un changement",
    async () => {
      await shows("Model A", "modèle affiché");
      const count = () => received(control, lastPid()).filter((frame) => frame.method === "session/set_config_option").length;
      assert.equal(count(), 0, "rien à l'ouverture (E7)");
      await post({ type: "setConfig", configId: "thought_level", value: "xhigh" });
      await waitFor(async () => {
        const s = (await state()).session;
        return s?.kind === "open" && s.config.effort?.current === "xhigh";
      }, "effort changé");
      // L'état change avant l'envoi (affichage optimiste d'Unharnessed) : attendre la trame.
      await waitFor(() => count() === 1, "set_config_option reçu");
      await new Promise((resolve) => setTimeout(resolve, 300));
      assert.equal(count(), 1, "une seule trame pour un changement");
    },
  ],
  [
    "authentification : message fixe avec le chemin du binaire",
    async () => {
      await post({ type: "send", text: "sanscle", images: [] });
      await idle();
      await shows(authText(fakeAgentPath()), "message /login");
    },
  ],
  [
    "plantage : pas de relance automatique, Restart reprend",
    async () => {
      const before = starts(control).length;
      await post({ type: "send", text: "crash", images: [] });
      await waitFor(async () => (await state()).agent.kind === "failed", "arrêt");
      await shows("Agent stopped", "état arrêté");
      await new Promise((resolve) => setTimeout(resolve, 500));
      assert.equal(starts(control).length, before, "aucune relance automatique");
      await post({ type: "restartAgent" });
      await ready();
      assert.equal(starts(control).length, before + 1);
    },
  ],
  [
    "bandeau D4 : nouvelle conversation en auto, puis config_option_update en manuel",
    async () => {
      setScenario(control, { permissions: "auto" });
      await post({ type: "newChat", draft: { text: "", images: [] } });
      await permissionsIs("auto");
      await shows("Mode auto sans sandbox", "bandeau");
      setScenario(control, {});
      await post({ type: "newChat", draft: { text: "", images: [] } });
      await permissionsIs("manual");
      await post({ type: "send", text: "auto", images: [] });
      await permissionsIs("auto");
      await shows("Mode auto sans sandbox", "bandeau après config_option_update");
    },
  ],
];

export async function run(): Promise<void> {
  const failures: string[] = [];
  for (const [name, body] of tests) {
    const started = Date.now();
    try {
      await body();
      console.log(`ok - ${name} (${Date.now() - started} ms)`);
    } catch (error) {
      failures.push(name);
      console.log(`not ok - ${name}\n${error instanceof Error ? error.stack : String(error)}`);
    }
  }
  console.log(`e2e : ${tests.length - failures.length}/${tests.length} réussis`);
  if (failures.length) throw new Error(`${failures.length} test(s) e2e en échec : ${failures.join(" ; ")}`);
}

// Tests e2e dans l'hôte d'extension de VS Code, contre l'agent factice (ticket 24, PR 2).
// Exécuteur minimal, sans dépendance : chaque test en série, échec = rejet de `run()`.

import assert from "node:assert/strict";
import * as vscode from "vscode";

import type { Probe, ToHost } from "../../src/shared/messages";
import { autoWithoutSandbox, TEXTS, type ChatState } from "../../src/shared/state";
import { fakeAgentPath, fixtureFrames, normalize, received, setScenario, starts, waitFor } from "../helpers";

interface ChatView {
  state: ChatState;
  html: string;
  rendered: boolean;
}

const control = process.env.FAKE_ACP_DIR as string;
const workspace = process.env.YUSAGENT_E2E_WORKSPACE as string;

const chats = async () => (await vscode.commands.executeCommand<ChatView[]>("yusagent._test.chats")) ?? [];
const chat = async (index = 0) => (await chats())[index];
const message = (msg: ToHost, index = 0) => vscode.commands.executeCommand("yusagent._test.message", index, msg);
const probe = (index = 0) => vscode.commands.executeCommand<Probe | undefined>("yusagent._test.probe", index);
const lastPid = () => starts(control).at(-1)!.pid;

async function openChat(scenario: object = {}): Promise<number> {
  setScenario(control, scenario);
  const before = starts(control).length;
  await vscode.commands.executeCommand("yusagent.newChat");
  await waitFor(async () => (await chat())?.state.phase === "ready" && starts(control).length > before, "chat prêt");
  await waitFor(async () => (await chat())?.rendered, "webview montée (script chargé sous la CSP)");
  return lastPid();
}

async function closeChat(): Promise<void> {
  await vscode.commands.executeCommand("yusagent._test.close", 0);
  await waitFor(async () => (await chats()).length === 0, "onglet fermé");
}

async function shows(text: string, what: string): Promise<string> {
  return waitFor(async () => {
    const shown = (await probe())?.text;
    return shown?.includes(text) ? shown : undefined;
  }, what);
}

const tests: [string, () => Promise<void>][] = [
  [
    "ouverture puis prompt : trames du fixture, CSP stricte, texte streamé affiché",
    async () => {
      const pid = await openChat();
      const view = await chat();
      const csp = /<meta http-equiv="Content-Security-Policy" content="([^"]+)">/.exec(view.html)?.[1] ?? "";
      // Seule source autorisée : `webview.cspSource` (ressources de l'extension), la même pour
      // les styles, les polices et les images ; scripts par nonce seulement.
      const directives = Object.fromEntries(csp.split("; ").map((part) => {
        const [name, ...sources] = part.split(" ");
        return [name, sources];
      }));
      assert.deepEqual(Object.keys(directives), ["default-src", "style-src", "font-src", "img-src", "script-src"]);
      assert.deepEqual(directives["default-src"], ["'none'"]);
      // `webview.cspSource` de VS Code 1.146 : `'self'` et l'hôte des ressources de l'extension.
      const resources = directives["style-src"];
      assert.ok(
        resources.length > 0 && resources.every((source: string) => source === "'self'" || /^https:\/\/\*\.vscode-cdn\.net$/.test(source)),
        `sources des ressources : ${resources.join(" ")}`,
      );
      for (const name of ["style-src", "font-src", "img-src"]) assert.deepEqual(directives[name], resources);
      assert.equal(directives["script-src"].length, 1);
      assert.match(directives["script-src"][0], /^'nonce-[A-Za-z0-9+/=]+'$/);
      assert.doesNotMatch(csp, /unsafe-|data:|blob:|(^|\s)\*(\s|;|$)/);
      console.log(`CSP : ${csp.replace(/'nonce-[^']+'/, "'nonce-…'")}`);
      const scripts = view.html.match(/<script[^>]*>/g) ?? [];
      assert.equal(scripts.length, 1);
      assert.match(scripts[0], / nonce="[^"]+" src="[^"]+webview\.js"/);
      assert.doesNotMatch(view.html, /<style|style="/);
      assert.equal(starts(control).at(-1)!.cwd, workspace, "processus lancé dans le workspace");
      await message({ type: "send", text: "bonjour" });
      await waitFor(async () => (await chat()).state.phase === "ready" && (await chat()).state.items.some((item) => item.kind === "agent"), "réponse");
      assert.deepEqual(normalize(received(control, pid), { cwd: workspace, sessionId: "fake-session-1", prompt: "bonjour" }), fixtureFrames());
      const shown = await shows("Bonjour", "texte streamé affiché");
      assert.ok(shown.includes("bonjour"), "message de l'utilisateur affiché");
      assert.ok(shown.includes("Envoyer"));
      assert.ok(!shown.includes("Mode auto sans sandbox"), "pas de bandeau en manuel");
    },
  ],
  [
    "Arrêter : bouton affiché pendant le tour, session/cancel, cancelled à la demande",
    async () => {
      await message({ type: "send", text: "lent" });
      await waitFor(async () => (await chat()).state.items.some((item) => item.kind === "permission"), "demande d'accord");
      const during = await shows("Arrêter", "bouton Arrêter");
      assert.ok(during.includes("Demande d'autorisation en attente"));
      await message({ type: "stop" });
      await waitFor(async () => (await chat()).state.phase === "ready", "fin du tour");
      const frames = received(control, lastPid());
      assert.ok(frames.some((frame) => frame.method === "session/cancel"));
      assert.deepEqual(frames.find((frame) => frame.id === "ya-perm-1"), { jsonrpc: "2.0", id: "ya-perm-1", result: { outcome: { outcome: "cancelled" } } });
      await shows(TEXTS.cancelled, "notice Arrêté");
    },
  ],
  [
    "sélecteurs : affichés, set_config_option seulement sur un changement",
    async () => {
      const { selects } = await waitFor(async () => await probe(), "sonde");
      assert.deepEqual(selects.map((select) => [select.label, select.value, select.disabled]), [
        ["Model", "capture/model-a", false],
        ["Reasoning effort", "high", false],
      ], "modèle et effort, pas de sélecteur permissions");
      assert.deepEqual(selects[1].options, ["off", "low", "medium", "high", "xhigh"]);
      const count = () => received(control, lastPid()).filter((frame) => frame.method === "session/set_config_option").length;
      assert.equal(count(), 0, "rien à l'ouverture (E7)");
      await message({ type: "setConfig", configId: "thought_level", value: "xhigh" });
      await waitFor(async () => (await chat()).state.configOptions.find((option) => option.id === "thought_level")?.currentValue === "xhigh", "effort changé");
      assert.equal(count(), 1);
      await waitFor(async () => (await probe())?.selects[1]?.value === "xhigh", "sélecteur à jour");
    },
  ],
  [
    "authentification : message fixe avec le chemin réel du binaire",
    async () => {
      await message({ type: "send", text: "auth" });
      await shows(TEXTS.auth(fakeAgentPath()), "message /login");
    },
  ],
  [
    "plantage : relance, historique grisé, nouvelle session",
    async () => {
      const before = starts(control).length;
      await message({ type: "send", text: "crash" });
      await waitFor(async () => starts(control).length === before + 1 && (await chat()).state.phase === "ready", "relance");
      assert.ok((await chat()).state.staleBefore > 0);
      await shows("nouvelle session", "message de relance");
    },
  ],
  [
    "fermeture de l'onglet : session/close",
    async () => {
      const pid = lastPid();
      await closeChat();
      await waitFor(() => received(control, pid).at(-1)?.method === "session/close", "session/close reçu");
    },
  ],
  [
    "bandeau : suit permissions annoncé par l'agent (auto à l'ouverture)",
    async () => {
      await openChat({ permissions: "auto" });
      assert.equal(autoWithoutSandbox((await chat()).state), true);
      await shows("Mode auto sans sandbox", "bandeau");
      await closeChat();
    },
  ],
  [
    "bandeau : apparaît sur config_option_update",
    async () => {
      await openChat({ permissions: "manual" });
      await message({ type: "send", text: "auto" });
      await shows("Mode auto sans sandbox", "bandeau après config_option_update");
      await closeChat();
    },
  ],
  [
    "binaire absent : message avec le chemin, boutons, aucun lancement",
    async () => {
      const config = vscode.workspace.getConfiguration("yusagent");
      await config.update("binaryPath", "/nulle-part/yusagent", vscode.ConfigurationTarget.Global);
      try {
        const before = starts(control).length;
        await vscode.commands.executeCommand("yusagent.newChat");
        await waitFor(async () => (await chat())?.state.phase === "stopped", "arrêt");
        await waitFor(async () => (await chat())?.rendered, "webview montée");
        const shown = await shows("/nulle-part/yusagent", "message binaire absent");
        assert.ok(shown.includes("Ouvrir le réglage yusagent.binaryPath") && shown.includes("Réessayer"));
        assert.equal(starts(control).length, before);
        await closeChat();
      } finally {
        await config.update("binaryPath", fakeAgentPath(), vscode.ConfigurationTarget.Global);
      }
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

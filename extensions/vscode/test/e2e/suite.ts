// Tests e2e dans l'hôte d'extension de VS Code, contre l'agent factice (ticket 24, PR I).
// Exécuteur minimal, sans dépendance : chaque test en série, échec = rejet de `run()`.

import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import * as vscode from "vscode";

import type { AgentState, ChatState, Probe, ProbeInput, SessionState, WebviewMessage } from "../../src/protocol";
import { fixtureFrames, noticesOf, normalize, permissionsOf, received, setScenario, starts, waitFor } from "../helpers";

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
const probeInput = (input: ProbeInput) => vscode.commands.executeCommand<Probe | undefined>("yusagent._test.probeInput", input);
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const BOX = "Allow this Python cell to run?";
// Ticket 30 (C1, option B) : l'étiquette discrète de « auto » sans sandbox, comme le TUI ; plus de bandeau d'alerte.
const AUTO_BADGE = "auto · no sandbox";
const OLD_BANNER = "Mode auto sans sandbox";

/** Contrôle de Yusuf : des clics d'utilisateur, dans l'ordre ; chacun doit atteindre son bouton. */
async function userClicks(...texts: string[]): Promise<void> {
  for (const text of texts) {
    const done = await waitFor(async () => {
      const result = await probeInput({ userClick: text });
      return result?.clicked === "clicked" ? result : undefined;
    }, `clic sur « ${text} » (issue : ${(await probe())?.clicked ?? "?"})`, 5_000);
    assert.equal(done.clicked, "clicked");
  }
}
const lastPid = () => starts(control).at(-1)!.pid;

async function ready(): Promise<State> {
  return waitFor(async () => {
    const s = await state();
    return s.session?.kind === "open" && s.rendered ? s : undefined;
  }, "session ouverte et webview montée", 30_000);
}

type Turn = Extract<ChatState["entries"][number], { kind: "turn" }>;

/** Ticket 25 : les tours lancés par l'agent sur un avis `[bash-done]`. */
async function wakeTurns(): Promise<Turn[]> {
  return (await state()).chat.entries.filter((entry): entry is Turn => entry.kind === "turn" && entry.wake === "bash");
}

/** Ticket 31 : les « Worked » de la page, un par tour qui a du contenu (aplati et masqué tant qu'il tourne). */
async function workedToggles(): Promise<Probe["toggles"]> {
  return (await probe())?.toggles.filter((toggle) => toggle.text.startsWith("Worked")) ?? [];
}

/**
 * Ticket 31 : le nombre de « Worked » avant un envoi, une fois la page à jour : autant que de tours
 * de l'état du chat qui ont du contenu (`webview/Chat.tsx`, `WorkRun` : un par tour non vide).
 */
async function workedCount(): Promise<number> {
  const expected = (await state()).chat.entries.filter((entry) => entry.kind === "turn" && entry.items.length > 0).length;
  await waitFor(async () => (await workedToggles()).length === expected, `${expected} « Worked » dans la page`);
  return expected;
}

/**
 * Ticket 31 : ouvre le travail du tour qui vient de finir. Le tour fini dans l'état du chat, puis un
 * « Worked » de plus qu'avant son envoi (`before`, de `workedCount`), affiché et fermé : c'est le
 * sien, le dernier de la page. Sans cette attente, le lot du chat qui le replie peut arriver après le
 * clic, qui ouvre alors (ou referme) celui d'un tour précédent.
 */
async function openNewWork(before: number): Promise<void> {
  await idle();
  await waitFor(async () => {
    const worked = await workedToggles();
    const last = worked.at(-1);
    return worked.length === before + 1 && last?.visible && !last.inert && !last.expanded;
  }, `un « Worked » de plus (${before + 1}), affiché et fermé`);
  await userClicks("Worked");
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
    return s?.kind === "open" && permissionsOf(s.config) === value;
  }, `permissions = ${value}`);
}

/** PR 3, étape 3 : la boîte de permission de bout en bout (agent factice, scénario `accord`). */
async function permissionBox(): Promise<void> {
  const answered = (id: string) => received(control, lastPid()).filter((frame) => frame.id === id).length;
  // 1. Zone de message vide : la boîte prend le focus ; boutons grisés 1 s, sans relance par les frappes.
  // Ticket 31 : délais vérifiés sur l'état de la boîte (sonde), jamais sur l'horloge du test. Le clic
  // sur « Allow » et `y` sont joués dans l'appel même qui ouvre la boîte : ni l'un ni l'autre ne compte.
  await probeInput({ atBoxOpen: { click: "Allow", keys: ["y"] } });
  await post({ type: "send", text: "accord", images: [] });
  const box = await shows(BOX, "boîte de permission");
  for (const part of ["print('accord')", "Allow", "Reject"]) assert.ok(box.includes(part), `boîte : ${part}`);
  assert.equal((await state()).badge, 1, "badge sur l'icône de la vue");
  let seen = (await probe())!;
  assert.equal(seen.focused, "box", "zone de message vide : la boîte a le focus");
  assert.deepEqual(seen.box?.click, { name: "Allow", disabled: true }, "clic à l'ouverture : bouton grisé");
  assert.deepEqual(seen.box?.keys, [{ key: "y", armed: false, result: "ignored" }], "y à l'ouverture : fenêtre de calme, rien");
  assert.equal(answered("ya-permission-2"), 0, "ni le clic grisé ni y n'ont répondu");
  // Une frappe relance la fenêtre des touches, pas le délai des boutons : `a` puis `y` dans le même
  // appel, `y` tombe dans la fenêtre que `a` vient de relancer.
  seen = (await probeInput({ keys: ["a", "y"] }))!;
  assert.deepEqual(seen.box?.keys.slice(-1), [{ key: "y", armed: false, result: "ignored" }], "y juste après une frappe : rien");
  assert.ok(seen.text.includes(BOX), "boîte toujours ouverte");
  assert.ok(seen.box!.keysReadyAt > seen.box!.buttonsReadyAt, "la frappe a repoussé les touches au-delà des boutons");
  // Les boutons deviennent actifs à leur délai, malgré les frappes ; `y` juste après une frappe, encore rien.
  await waitFor(async () => (await probe())?.box?.buttonsReady, "boutons prêts (état de la boîte)");
  seen = (await probeInput({ keys: ["a", "y"] }))!;
  assert.deepEqual(
    seen.boxButtons.map((button) => button.disabled),
    [false, false],
    "boutons actifs 1 s après l'ouverture, malgré les frappes",
  );
  assert.deepEqual(seen.box?.keys.slice(-1), [{ key: "y", armed: false, result: "ignored" }], "y juste après une frappe, boutons actifs : rien");
  await sleep(1_100);
  assert.ok((await probeInput({ keys: ["Enter"] }))?.text.includes(BOX), "Entrée : jamais une réponse");
  await sleep(1_100);
  assert.ok((await probeInput({ focus: "none", keys: ["y"] }))?.text.includes(BOX), "y sans le focus : rien");
  assert.equal(answered("ya-permission-2"), 0, "aucune réponse partie");
  await probeInput({ focus: "box", keys: ["y"] });
  await idle();
  assert.deepEqual(received(control, lastPid()).find((frame) => frame.id === "ya-permission-2"), {
    jsonrpc: "2.0",
    id: "ya-permission-2",
    result: { outcome: { outcome: "selected", optionId: "allow-once" } },
  });
  const done = await shows("autorisée", "la cellule a tourné");
  assert.ok(!done.includes(BOX), "boîte fermée après la réponse");
  assert.equal((await state()).badge, undefined, "badge retiré");
  // 2. Esc refuse ; la carte dit « Declined » (le tour s'arrête, D7).
  const workedBefore = await workedCount();
  await post({ type: "send", text: "accord", images: [] });
  await shows(BOX, "deuxième demande");
  await sleep(1_100);
  await probeInput({ focus: "box", keys: ["Escape"] });
  await idle();
  assert.deepEqual(received(control, lastPid()).find((frame) => frame.id === "ya-permission-3")?.result, {
    outcome: { outcome: "selected", optionId: "reject-once" },
  });
  // Le travail d'un tour fini est replié : on l'ouvre comme un utilisateur.
  await openNewWork(workedBefore);
  await shows("Declined", "carte refusée");
  // 3. Zone de message non vide : le focus y reste ; le bouton « Reject », actif après 1 s, refuse.
  assert.equal((await probeInput({ type: "brouillon en cours" }))?.focused, "composer");
  await post({ type: "send", text: "accord", images: [] });
  await shows(BOX, "troisième demande");
  assert.equal((await probe())?.focused, "composer", "zone de message non vide : le focus ne bouge pas");
  await sleep(1_100);
  await probeInput({ click: "Reject" });
  await idle();
  assert.deepEqual(received(control, lastPid()).find((frame) => frame.id === "ya-permission-4")?.result, {
    outcome: { outcome: "selected", optionId: "reject-once" },
  });
  await probeInput({ type: "" });
}

const tests: [string, () => Promise<void>][] = [
  [
    "vue de la barre latérale : trames du fixture, CSP stricte, texte streamé affiché",
    async () => {
      // Ticket 30 : l'agent factice ouvre en auto par défaut (C7) ; les boîtes de permission ci-dessous, en manuel.
      setScenario(control, { permissions: "manual" });
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
      assert.ok(!shown.includes(AUTO_BADGE) && !shown.includes(OLD_BANNER), "ni étiquette ni bandeau en manuel");
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
    "tour lancé par l'agent (ticket 25) : turnStart du pont, fermé sur responseBoundary, message suivant envoyé tout de suite",
    async () => {
      await post({ type: "send", text: "réveil", images: [] });
      await waitFor(async () => (await wakeTurns()).at(-1)?.state === "done", "tour de réveil fini sur sa frontière");
      await shows("réveil fini", "contenu du réveil");
      await idle();
      const prompts = () => received(control, lastPid()).filter((frame) => frame.method === "session/prompt").length;
      const before = prompts();
      await post({ type: "send", text: "bonjour", images: [] });
      await waitFor(() => prompts() === before + 1, "session/prompt envoyé tout de suite", 2_000);
      await shows("Bonjour", "réponse au message suivant");
      await idle();
    },
  ],
  [
    "Stop pendant un réveil (ticket 25, T4) : session/cancel, tour arrêté, réveil suivant affiché",
    async () => {
      await post({ type: "send", text: "réveil-lent", images: [] });
      await shows("réveil en cours", "réveil en cours");
      assert.ok((await state()).chat.busy, "panneau occupé pendant le réveil");
      await post({ type: "cancel" });
      await shows("second réveil", "réveil suivant");
      await idle();
      assert.deepEqual(
        (await wakeTurns()).slice(-2).map((turn) => turn.state),
        ["cancelled", "done"],
      );
      assert.ok(received(control, lastPid()).some((frame) => frame.method === "session/cancel"), "session/cancel envoyé");
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
    "diffs (PR 3, étape 4) : vscode.diff, Revert par applyEdit, fichier créé à la corbeille, texte au message suivant",
    async () => {
      const a = join(workspace, "a.txt");
      const created = join(workspace, "neuf.txt");
      writeFileSync(a, "un\ndeux\ntrois\n");
      const workedBefore = await workedCount();
      await post({ type: "send", text: "diff", images: [] });
      await idle();
      const edits = async () => {
        for (const entry of (await state()).chat.entries) {
          if (entry.kind !== "turn") continue;
          const tool = entry.items.find((item) => item.kind === "tool" && item.id === "call-d");
          if (tool?.kind === "tool") return tool.edits;
        }
        return undefined;
      };
      await waitFor(async () => (await edits())?.every((edit) => edit.revert?.state === "ready"), "avant retrouvé, Revert possible");
      // Contrôle de Yusuf : rien d'affiché n'ignore les clics (repli fermé affiché mais `inert`).
      assert.equal((await probe())?.visibleInert, 0, "aucun bloc inerte affiché");
      // Le geste de Yusuf : un clic sur la carte telle qu'on la voit. Repliée dans « Worked », elle
      // n'est pas affichée (« covered ») ; affichée, elle doit répondre, pas être inerte.
      assert.notEqual((await probeInput({ userClick: "Edited" }))?.clicked, "inert", "carte affichée mais inerte");
      // Puis tout par des clics d'utilisateur : le travail replié, la carte, le nom du fichier (diff).
      await openNewWork(workedBefore);
      await userClicks("Edited", "a.txt");
      assert.equal((await probe())?.visibleInert, 0, "aucun bloc inerte affiché, replis ouverts");
      const sheet = await shows("Revert", "carte dépliée avec Revert");
      for (const part of ["DEUX", "nouveau", "a.txt", "neuf.txt"]) assert.ok(sheet.includes(part), `carte dépliée : ${part}`);
      const diff = await waitFor(() => {
        for (const tab of vscode.window.tabGroups.all.flatMap((group) => group.tabs)) {
          if (tab.input instanceof vscode.TabInputTextDiff && tab.input.original.scheme === "yusagent-diff") return tab.input;
        }
        return undefined;
      }, "vscode.diff ouvert");
      assert.equal((await vscode.workspace.openTextDocument(diff.original)).getText(), "un\ndeux\ntrois\n", "avant");
      assert.equal((await vscode.workspace.openTextDocument(diff.modified)).getText(), "un\nDEUX\ntrois\n", "après");
      // Revert par les boutons de la carte : applyEdit puis enregistrement ; le fichier créé va à la corbeille (C12).
      await vscode.commands.executeCommand("workbench.action.closeAllEditors");
      await vscode.commands.executeCommand("yusagent.open");
      await userClicks("Revert");
      await waitFor(async () => (await edits())?.some((edit) => edit.revert?.state === "reverted"), "premier Revert");
      // Le panneau a remplacé ce bouton par « Reverted » avant le second clic.
      await shows("Reverted", "premier Revert affiché");
      await userClicks("Revert");
      await waitFor(async () => (await edits())?.every((edit) => edit.revert?.state !== "ready" || edit.revert.refused), "Revert traité");
      assert.deepEqual((await edits())?.map((edit) => edit.revert), [{ state: "reverted" }, { state: "reverted" }]);
      assert.equal(readFileSync(a, "utf8"), "un\ndeux\ntrois\n", "a.txt restauré et enregistré");
      assert.equal(existsSync(created), false, "neuf.txt retiré");
      await post({ type: "send", text: "bonjour", images: [] });
      await idle();
      const prompt = received(control, lastPid())
        .filter((frame) => frame.method === "session/prompt")
        .at(-1) as { params: { prompt: { text: string }[] } };
      // Une ligne par fichier, dans l'ordre où les restaurations ont fini (les deux Revert partent ensemble).
      const [text, lines] = prompt.params.prompt[0].text.split("\n\n");
      assert.equal(text, "bonjour");
      assert.deepEqual(lines.split("\n").sort(), ["[user-reverted] a.txt", "[user-reverted] neuf.txt"]);
      await vscode.commands.executeCommand("workbench.action.closeAllEditors");
    },
  ],
  [
    "cartes (PR 3, étape 2) : édition réelle, code de sortie, cellule refusée",
    async () => {
      let workedBefore = await workedCount();
      await post({ type: "send", text: "carte", images: [] });
      await idle();
      // En-tête : type et fichiers d'après les vrais diffs, lignes changées, premier `bash()` en erreur.
      // Le travail du tour est replié : « Worked » l'ouvre ; `exit 1` n'est que sur cette carte.
      await openNewWork(workedBefore);
      const shown = await shows("exit 1", "carte d'une cellule qui édite");
      for (const part of ["Edited", "a.py", "neuf.txt", "+3", "−1"]) assert.ok(shown.includes(part), `en-tête : ${part}`);
      // Dépliée par un clic : une ligne par bash(), séparateurs lisibles (contrôle de Yusuf : « exit 0·32 ms »).
      await userClicks("Edited");
      const sheet = await shows("$ false", "carte dépliée");
      assert.ok(sheet.includes("exit 0 · 6 ms") && sheet.includes("exit 1 · 3 ms"), "exit puis durée, séparés");
      workedBefore = await workedCount();
      await post({ type: "send", text: "refus", images: [] });
      await openNewWork(workedBefore);
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
    "authentification (ticket 40, X5, test 12) : texte fixe avec le nom du fournisseur, bouton « Open Providers » vers l'écran Providers",
    async () => {
      await post({ type: "send", text: "auth", images: [] });
      await idle();
      const shown = await shows("Not signed in to Capture, or the sign-in has expired.", "texte fixe avec le fournisseur");
      assert.ok(!shown.includes("/login") && !/terminal/i.test(shown), "plus de renvoi au terminal");
      await userClicks("Open Providers");
      await shows("ChatGPT Plus/Pro", "écran Providers");
      await userClicks("Back");
    },
  ],
  [
    "Providers (ticket 40, X5, test 9) : sans fournisseur connecté, la puce « Connect a provider » mène à l'écran ; bouton Providers visible",
    async () => {
      setScenario(control, { providers: "none" });
      await post({ type: "newChat", draft: { text: "", images: [] } });
      await ready();
      await userClicks("Connect a provider");
      const screen = await shows("Sign in with a subscription", "écran Providers, rien de connecté");
      assert.ok(screen.includes("ChatGPT Plus/Pro"), "ligne ChatGPT");
      await userClicks("Back");
      setScenario(control, {});
      await post({ type: "newChat", draft: { text: "", images: [] } });
      await ready();
      await userClicks("Providers");
      await shows("Capture", "écran Providers par le bouton");
      await userClicks("Back");
    },
  ],
  [
    "Providers (ticket 40, X5, test 11) : pas d'écran Local, pas de ligne Claude par abonnement (même envoyée), ligne Anthropic par clé",
    async () => {
      await userClicks("Providers");
      const screen = await shows("ChatGPT Plus/Pro", "écran Providers");
      assert.ok(screen.includes("Anthropic"), "ligne Anthropic par clé");
      assert.ok(!screen.includes("Claude Pro/Max"), "pas de ligne Claude par abonnement");
      assert.ok(!screen.split("\n").some((line) => line.trim() === "Local"), "pas de section Local");
      assert.ok(!screen.includes("Add a local model"), "pas d'ajout de modèle local");
      await userClicks("Back");
    },
  ],
  [
    "login (ticket 40, X5, test 10) : écran SignIn, « Open » envoie login_open, Cancel envoie login_cancel",
    async () => {
      const sent = (method: string) => received(control, lastPid()).filter((frame) => frame.method === method);
      await userClicks("Providers");
      await shows("ChatGPT Plus/Pro", "écran Providers");
      await userClicks("ChatGPT Plus/Pro");
      await shows("Sign in with your browser", "écran SignIn");
      assert.deepEqual(sent("_yus/providers/login").map((frame) => frame.params), [{ providerId: "openai-codex" }]);
      await userClicks("Open the sign-in page");
      await waitFor(() => sent("_yus/providers/login_open").length === 1, "login_open envoyée");
      await userClicks("Cancel");
      await waitFor(() => sent("_yus/providers/login_cancel").length === 1, "login_cancel envoyée");
      await shows("ChatGPT Plus/Pro", "retour à l'écran Providers");
      await userClicks("Back");
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
    "étiquette D4 (ticket 30) : auto par défaut comme le pont, étiquette discrète sans bandeau ; manuel sans étiquette, puis config_option_update",
    async () => {
      // Sans réglage, l'agent factice ouvre en auto, comme le pont (C7).
      setScenario(control, {});
      await post({ type: "newChat", draft: { text: "", images: [] } });
      await permissionsIs("auto");
      const shown = await shows(AUTO_BADGE, "étiquette");
      assert.ok(!shown.includes(OLD_BANNER), "plus de bandeau d'alerte");
      setScenario(control, { permissions: "manual" });
      await post({ type: "newChat", draft: { text: "", images: [] } });
      await permissionsIs("manual");
      await waitFor(async () => (await probe())?.text.includes(AUTO_BADGE) === false, "pas d'étiquette en manuel");
      await post({ type: "send", text: "auto", images: [] });
      await permissionsIs("auto");
      await shows(AUTO_BADGE, "étiquette après config_option_update");
    },
  ],
  [
    "réglages (ticket 31, X3) : bouton Settings, onglets General, MCP et Skills sans Instructions ni Images ni « restart », écran lu à son ouverture seulement",
    async () => {
      setScenario(control, {});
      await post({ type: "newChat", draft: { text: "", images: [] } });
      await ready();
      // Ticket 40 (X5) : les trames de cet écran ; `_yus/providers/list` part à l'ouverture (puce « Connect a provider »).
      const screenFrames = () =>
        received(control, lastPid()).filter((frame) => /^_yus\/(settings|skills|mcp)\//.test(String(frame.method ?? "")));
      await sleep(300);
      assert.deepEqual(screenFrames(), [], "rien lu avant l'ouverture de l'écran (E7)");
      // L'onglet ouvert, sans dépendre de celui où un test précédent a laissé l'écran.
      await userClicks("Settings", "General");
      const general = await shows("Compaction", "onglet General");
      for (const label of ["General", "MCP", "Skills", "Retries", "Switching providers", "Usage limits"]) {
        assert.ok(general.includes(label), `${label} affiché`);
      }
      for (const absent of ["Instructions", "Image model", "Block images"]) assert.ok(!general.includes(absent), `${absent} absent`);
      assert.ok(!/restart/i.test(general), "aucun texte « restart »");
      await waitFor(
        () => ["_yus/settings/get", "_yus/skills/list", "_yus/mcp/list"].every((method) => screenFrames().some((frame) => frame.method === method)),
        "écran lu à son ouverture",
      );
      await userClicks("Skills");
      const skills = await shows("Changes reach the agent with your next message.", "texte de l'onglet Skills (C3)");
      assert.ok(!/restart/i.test(skills), "aucun « restart » dans Skills");
      await userClicks("MCP");
      const mcp = await shows("Your servers", "onglet MCP");
      assert.ok(mcp.includes("Changes reach the agent with your next message."), "texte de l'onglet MCP (C3)");
      // Ticket 40 (X5, décision 8) : le catalogue n'est plus masqué (C4) ; test 13 plus bas.
      await userClicks("Back");
    },
  ],
  [
    "catalogue MCP (ticket 40, X5, test 13) : Marketplace visible, Add ouvre le formulaire de jeton, _yus/mcp/set_token part",
    async () => {
      // L'onglet MCP directement : l'écran rouvre le dernier onglet ouvert.
      await userClicks("Settings", "MCP");
      try {
        const mcp = await shows("Marketplace", "catalogue");
        assert.ok(mcp.includes("Linear") && mcp.includes("Zoom"), "services du catalogue");
        // Le dernier « Add » de la page : Zoom (A à Z), le service ajouté par jeton.
        await userClicks("Add");
        await shows("Zoom token", "formulaire de jeton");
        await post({ type: "mcpSetToken", id: "zoom", token: "zoom-x5-e2e" });
        await waitFor(
          () => received(control, lastPid()).some((frame) => frame.method === "_yus/mcp/set_token" && (frame.params as { id?: string }).id === "zoom"),
          "_yus/mcp/set_token envoyée",
        );
      } finally {
        // Même en échec : l'écran fermé, les tests suivants partent de la conversation.
        await userClicks("Back");
      }
    },
  ],
  [
    "réglages stylés (ticket 31) : libellé séparé de sa description ; interrupteur de taille visible, état lisible",
    async () => {
      setScenario(control, {});
      await post({ type: "newChat", draft: { text: "", images: [] } });
      await ready();
      // L'onglet ouvert est gardé par l'app (le cas précédent finit sur MCP) : General, par un clic.
      await userClicks("Settings", "General");
      try {
        const general = await shows("Compaction", "onglet General");
        // Le texte affiché (`innerText`) suit la mise en page : libellé et description sur deux lignes.
        assert.ok(!general.includes("Compact automaticallyBefore"), "libellé collé à sa description");
        assert.ok(general.includes("Compact automatically\nBefore the context is full."), "libellé puis description, séparés");
        const retry = async () => (await probe())?.switches.find((item) => item.label === "Retry failed requests");
        // Allumé : le défaut du pont (C1), dans l'agent factice comme dans le binaire.
        const on = await waitFor(async () => {
          const found = await retry();
          return found?.checked ? found : undefined;
        }, "« Retry failed requests » allumé");
        assert.ok(on.width >= 24 && on.height >= 14, `interrupteur de taille visible : ${on.width}×${on.height}`);
        await userClicks("Retry failed requests");
        // Éteint : son fond et la place de son bouton changent (état lisible, pas seulement annoncé).
        await waitFor(async () => {
          const off = await retry();
          return off && !off.checked && off.background !== on.background && off.knobLeft !== on.knobLeft;
        }, `état lisible : fond ou bouton inchangés (allumé : ${on.background}, bouton à ${on.knobLeft})`);
        await userClicks("Retry failed requests");
      } finally {
        // Retour au chat même en cas d'échec : les cas suivants partent de lui.
        await probeInput({ userClick: "Back" });
      }
    },
  ],
  [
    "skills (ticket 31) : skill créé puis message, ligne au-dessus du message ; interrupteur → _yus/skills/set, ligne du retrait",
    async () => {
      setScenario(control, {});
      await post({ type: "newChat", draft: { text: "", images: [] } });
      await ready();
      // Le premier prompt construit la session : ce que le modèle sait part de là.
      await post({ type: "send", text: "bonjour", images: [] });
      await idle();
      await post({ type: "createSkill", name: "demo-e2e", description: "Demo skill." });
      await waitFor(() => received(control, lastPid()).some((frame) => frame.method === "_yus/skills/create"), "_yus/skills/create reçu");
      await post({ type: "send", text: "x3 un", images: [] });
      await idle();
      const added = "Skills updated: added demo-e2e. Sent to the agent.";
      const text = await shows(added, "ligne de l'ajout");
      assert.ok(text.indexOf(added) < text.indexOf("x3 un"), "au-dessus du message");
      assert.deepEqual(noticesOf((await state()).chat.entries.at(-1)), [added]);
      await userClicks("Settings", "Skills", "Use demo-e2e");
      await waitFor(
        () => received(control, lastPid()).some((frame) => frame.method === "_yus/skills/set" && (frame.params as { enabled?: boolean }).enabled === false),
        "_yus/skills/set reçu",
      );
      await userClicks("Back");
      await post({ type: "send", text: "x3 deux", images: [] });
      await idle();
      await shows("Skills updated: removed demo-e2e. Sent to the agent.", "ligne du retrait");
    },
  ],
  [
    "permissions (ticket 31, C5, C6) : menu sur l'étiquette, libellés du pont, set_config_option seulement sur un changement, étiquette manual",
    async () => {
      setScenario(control, {});
      await post({ type: "newChat", draft: { text: "", images: [] } });
      await ready();
      await permissionsIs("auto");
      const sets = () => received(control, lastPid()).filter((frame) => frame.method === "session/set_config_option");
      await userClicks(AUTO_BADGE);
      const menu = await shows("Ask before each tool call", "menu des permissions");
      assert.ok(menu.includes("Run tool calls without asking (no sandbox)"), "les deux libellés du pont");
      // La valeur courante : rien ne part.
      await userClicks("Run tool calls without asking (no sandbox)");
      await sleep(300);
      assert.equal(sets().length, 0, "rien pour la valeur courante");
      await userClicks(AUTO_BADGE, "Ask before each tool call");
      await permissionsIs("manual");
      await waitFor(() => sets().length === 1, "set_config_option reçu");
      assert.deepEqual(sets()[0].params, { sessionId: "fake-session-1", configId: "permissions", value: "manual" });
      const shown = await shows("manual", "étiquette manual");
      assert.ok(!shown.includes(AUTO_BADGE), "plus d'étiquette auto");
      await userClicks("manual", "Run tool calls without asking (no sandbox)");
      await permissionsIs("auto");
      await waitFor(() => sets().length === 2, "second set_config_option");
      assert.deepEqual(sets()[1].params, { sessionId: "fake-session-1", configId: "permissions", value: "auto" });
      await shows(AUTO_BADGE, "étiquette auto revenue");
    },
  ],
  [
    "ticket 39 (X4) : « Send now » pendant un tour : le message rejoint le tour, une seule trame session/prompt",
    async () => {
      await ready();
      await idle();
      const sent = (method: string) => received(control, lastPid()).filter((frame) => frame.method === method);
      const prompts = sent("session/prompt").length;
      try {
        await post({ type: "send", text: "longue", images: [] });
        await waitFor(async () => {
          const turn = (await state()).chat.entries.filter((entry): entry is Turn => entry.kind === "turn").at(-1);
          return turn?.items.some((item) => item.kind === "tool" && item.id === "call-l" && item.status === "running");
        }, "cellule longue en cours");
        await post({ type: "send", text: "ajoute ceci", images: [] });
        await waitFor(
          async () => (await state()).chat.queue.some((message) => message.text === "ajoute ceci" && message.state === "waiting"),
          "message en file pendant le tour",
        );
        // Caché pendant un tour tant que la méthode n'est pas servie (`Composer.tsx`, `hidden={busy && !steer}`) :
        // l'issue est alors `covered` (taille nulle).
        let outcome: Probe["clicked"];
        await waitFor(async () => {
          outcome = (await probeInput({ userClick: "Send now" }))?.clicked;
          return outcome === "clicked";
        }, "clic sur « Send now »", 5_000).catch(() => undefined);
        assert.equal(outcome, "clicked", "« Send now » visible pendant un tour");
        await idle();
        assert.equal(sent("_yus/session/steer").length, 1, "un steer");
        assert.equal(sent("session/prompt").length, prompts + 1, "aucun doublon");
        const last = (await state()).chat.entries.filter((entry): entry is Turn => entry.kind === "turn").at(-1);
        assert.equal(last?.prompt?.text, "ajoute ceci");
        assert.equal(last?.state, "done");
      } finally {
        // Un échec ne laisse pas la cellule longue aux tests suivants.
        if ((await state()).chat.busy) await post({ type: "cancel" });
        await idle();
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

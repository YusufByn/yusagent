// yusAgent (YA-10, ticket 24, PR 3, étape 6) : e2e dans VS Code contre le vrai binaire (debug),
// provider factice, kernel réel. Lancé par `run-real.ts` (CI seulement).
// - session 1 (manuel) : cellule avec `bash()` (accord par le bouton de la boîte), `edit()` puis
//   « Revert », refus (fichier absent, « Declined », le tour s'arrête), texte `[user-reverted]` dans
//   le message suivant (fichier de session), Stop pendant une cellule ;
// - session 2 (auto) : bandeau, cellule exécutée sans demande ; tour lancé par l'agent (avis
//   `[bash-done]`, ticket 25) ouvert par `turnStart` et fermé sur `responseBoundary`, message suivant
//   envoyé tout de suite ; suppression pure par `edit()` après un emoji, puis « Revert » (ticket 26 :
//   colonne du pont), fichier revenu octet pour octet ;
// - ticket 28 (reprise, historique, suppression ; clics sur l'élément réellement sous le pointeur) :
//   conversation neuve quittée vide, supprimée (session arrêtée dans le démon, fichier retiré) ;
//   « Restart » après un plantage du pont (SIGKILL) : la conversation revient par `session/load`,
//   avec le tour lancé par l'agent pendant que le chat était fermé ; tour de l'agent en cours au
//   chargement : affiché en cours, message tapé mis en attente jusqu'à sa fin ; historique : retour
//   à une conversation de fond, telle quelle ; double ouverture (un second client ACP tient la
//   conversation) : refus affiché, agent arrêté, puis « Restart » la reprend une fois libérée ;
//   suppression d'une conversation de fond : son agent, sa session du démon, son fichier ;
// - fin : démon arrêté, aucun processus restant.

import assert from "node:assert/strict";
import { execFileSync, spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { basename, join } from "node:path";
import { createInterface } from "node:readline";
import * as vscode from "vscode";

import type { AgentState, ChatState, HistoryState, Probe, ProbeInput, SessionState, TurnItem, WebviewMessage } from "../../src/protocol";
import { waitFor } from "../helpers";

interface State {
  agent: AgentState;
  session: SessionState | null;
  chat: ChatState;
  rendered: boolean;
  badge?: number;
  pid?: number;
  saved?: { id: string; path: string };
  history: HistoryState;
}

type Tool = Extract<TurnItem, { kind: "tool" }>;
type Turn = Extract<ChatState["entries"][number], { kind: "turn" }>;

const workspace = process.env.YUSAGENT_E2E_WORKSPACE as string;
const home = process.env.YUSAGENT_E2E_HOME as string;
const temp = process.env.YUSAGENT_E2E_TMP as string;
const binary = process.env.YUSAGENT_E2E_BINARY as string;
const script = process.env.PRIME_AGENT_FAUX_SCRIPT as string;
const SLOW = 90_000;
const BOX = "Allow this Python cell to run?";

const state = async () => (await vscode.commands.executeCommand<State>("yusagent._test.state"))!;
const post = (message: WebviewMessage) => vscode.commands.executeCommand("yusagent._test.post", message);
const probe = () => vscode.commands.executeCommand<Probe | undefined>("yusagent._test.probe");
const probeInput = (input: ProbeInput) => vscode.commands.executeCommand<Probe | undefined>("yusagent._test.probeInput", input);
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** Contrôle de Yusuf : des clics d'utilisateur, dans l'ordre ; chacun doit atteindre son bouton. */
async function userClicks(...texts: string[]): Promise<void> {
  for (const text of texts) {
    const done = await waitFor(async () => {
      const result = await probeInput({ userClick: text });
      return result?.clicked === "clicked" ? result : undefined;
    }, `clic sur « ${text} »`, 5_000);
    assert.equal(done.clicked, "clicked");
  }
}

/** Le script du provider factice d'une session (lu à sa création) : une réponse par appel au modèle. */
function writeScript(responses: unknown[]): void {
  writeFileSync(script, JSON.stringify({ engine: "faux", responses }));
}
const cell = (id: string, code: string) => ({
  content: [{ type: "toolCall", id, name: "python_repl", arguments: { code } }],
});

async function ready(): Promise<void> {
  await waitFor(async () => {
    const s = await state();
    return s.session?.kind === "open" && s.rendered;
  }, "session ouverte et webview montée", SLOW);
}

async function idle(timeout = SLOW): Promise<void> {
  await waitFor(async () => !(await state()).chat.busy, "tour fini", timeout);
}

async function shows(text: string, what: string, timeout = SLOW): Promise<string> {
  return waitFor(async () => {
    const shown = (await probe())?.text;
    return shown?.includes(text) ? shown : undefined;
  }, what, timeout);
}

async function tool(id: string): Promise<Tool | undefined> {
  for (const entry of (await state()).chat.entries) {
    if (entry.kind !== "turn") continue;
    const found = entry.items.find((item): item is Tool => item.kind === "tool" && item.id === id);
    if (found) return found;
  }
  return undefined;
}

/** La demande d'accord de `id` est ouverte (sa boîte affichée), puis la réponse. */
async function answer(id: string, optionId: "allow-once" | "reject-once"): Promise<void> {
  await waitFor(async () => (await tool(id))?.status === "pending", `carte ${id} en attente d'accord`, SLOW);
  await shows(BOX, `boîte de ${id}`);
  await post({ type: "permission", toolCallId: id, optionId });
}

/** Le texte de tous les fichiers de session du HOME de test. */
function sessionText(): string {
  const out: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (name.endsWith(".jsonl")) out.push(readFileSync(path, "utf8"));
    }
  };
  walk(join(home, ".yusagent"));
  return out.join("\n");
}

/** Les processus du binaire de test encore en vie. */
function leftovers(): string[] {
  try {
    return execFileSync("pgrep", ["-f", binary], { encoding: "utf8" }).split("\n").filter(Boolean);
  } catch {
    return [];
  }
}

const tests: [string, () => Promise<void>][] = [
  [
    "cellule avec bash() : boîte, accord par le bouton, carte avec la commande et son code de sortie",
    async () => {
      writeScript([
        cell("call-bash", "r = await bash('echo lu')\nprint('LU', r.exit_code)"),
        { text: "bash fini" },
        cell("call-edit", "print(await edit('ok.txt', '', 'oui\\n'))"),
        { text: "edit fini" },
        cell("call-deny", "open('refuse.txt', 'w').write('x')"),
        { text: "jamais demandé : le tour s'arrête après un refus" },
        cell("call-slow", "import time\ntime.sleep(30)"),
        { text: "jamais : arrêté" },
      ]);
      await vscode.commands.executeCommand("yusagent.open");
      await ready();
      await post({ type: "send", text: "cellule bash", images: [] });
      await waitFor(async () => (await tool("call-bash"))?.status === "pending", "carte en attente d'accord", SLOW);
      await shows(BOX, "boîte");
      await sleep(1_100);
      await probeInput({ click: "Allow" });
      await idle();
      const done = await tool("call-bash");
      assert.equal(done?.status, "done");
      assert.deepEqual(
        done?.runs?.map(({ command, exitCode, output }) => ({ command, exitCode, output })),
        [{ command: "echo lu", exitCode: 0, output: "lu\n" }],
      );
      assert.equal(done?.cell?.kind, "shell");
      assert.equal(done?.cell?.command, "echo lu");
      await shows("bash fini", "réponse du modèle");
      // Ticket 28 : la webview reçoit la fin du tour après l'hôte. Un clic sur « Ran » avant le repli
      // ouvrait la carte (travail encore à plat), le clic final la refermait : on attend le repli.
      // La carte par sa commande : l'en-tête de groupe « Ran 1 Python cell », masqué pour une cellule
      // seule, la précède dans la page.
      const toggle = async (verb: string, target = "") =>
        (await probe())?.toggles.find((button) => button.text.startsWith(verb) && button.text.includes(target));
      await waitFor(async () => {
        const worked = await toggle("Worked");
        return worked?.visible && !worked.inert && !worked.expanded;
      }, "tour replié, « Worked » affiché", SLOW);
      assert.equal((await toggle("Ran", "echo lu"))?.expanded, false, "carte fermée au repli");
      await userClicks("Worked");
      // Contrôle de Yusuf : rien d'affiché n'ignore les clics, et la carte telle qu'on la voit n'est pas inerte.
      const card = await toggle("Ran", "echo lu");
      assert.deepEqual([card?.visible, card?.inert, card?.expanded], [true, false, false], "carte affichée, fermée, pas inerte");
      assert.equal((await probe())?.visibleInert, 0, "aucun bloc inerte affiché");
      // Dépliée par un seul clic : la ligne du bash(), exit puis durée séparés.
      await userClicks("Ran");
      assert.match(await shows("$ echo lu", "carte dépliée"), /exit 0 · \d+ ms/);
    },
  ],
  [
    "edit() : vrai diff, Revert (fichier créé à la corbeille), texte [user-reverted] dans le message suivant",
    async () => {
      await post({ type: "send", text: "edit", images: [] });
      await answer("call-edit", "allow-once");
      await idle();
      const file = join(workspace, "ok.txt");
      assert.equal(readFileSync(file, "utf8"), "oui\n");
      await waitFor(async () => (await tool("call-edit"))?.edits?.[0].revert?.state === "ready", "Revert possible", SLOW);
      const edit = (await tool("call-edit"))!.edits![0];
      assert.deepEqual([edit.shown, edit.created, edit.lines], ["ok.txt", true, [["+", "oui"]]]);
      // Par des clics d'utilisateur : le travail replié, la carte, puis « Revert ».
      await userClicks("Worked", "Wrote", "Revert");
      await waitFor(async () => (await tool("call-edit"))?.edits?.[0].revert?.state === "reverted", "Revert fait");
      assert.equal(existsSync(file), false, "ok.txt retiré");
    },
  ],
  [
    "refus : fichier absent, Declined, le tour s'arrête ; le message porte [user-reverted]",
    async () => {
      await post({ type: "send", text: "refus", images: [] });
      await answer("call-deny", "reject-once");
      await idle();
      const declined = await tool("call-deny");
      assert.equal(declined?.status, "failed");
      assert.equal(declined?.declined, true);
      assert.equal(existsSync(join(workspace, "refuse.txt")), false, "la cellule refusée n'a pas tourné");
      assert.ok(sessionText().includes("refus\\n\\n[user-reverted] ok.txt"), "texte fixe à la fin du message, dans la session");
      // La réponse restée dans le script n'a pas été demandée : le prompt suivant la reçoit.
      await post({ type: "send", text: "suivant", images: [] });
      await idle();
      await shows("jamais demandé", "le tour s'était arrêté après le refus");
    },
  ],
  [
    "Stop pendant une cellule : tour annulé en quelques secondes",
    async () => {
      await post({ type: "send", text: "lent", images: [] });
      await answer("call-slow", "allow-once");
      await waitFor(async () => (await tool("call-slow"))?.status === "running", "cellule en cours", SLOW);
      await sleep(1_000);
      const started = Date.now();
      await post({ type: "cancel" });
      await idle(10_000);
      const took = Date.now() - started;
      console.log(`Stop : tour fini en ${took} ms`);
      const turns = (await state()).chat.entries.filter((entry) => entry.kind === "turn");
      assert.equal((turns.at(-1) as { state?: string }).state, "cancelled");
    },
  ],
  [
    "auto : bandeau, cellule exécutée sans demande",
    async () => {
      writeFileSync(join(home, ".yusagent", "settings.json"), JSON.stringify({ permissions: "auto" }));
      writeScript([
        cell("call-auto", "open('auto.txt', 'w').write('x')"),
        { text: "auto fini" },
        // Ticket 25 : un bash() jamais lu, puis le tour de réveil sur son avis, puis le message suivant.
        cell("call-launch", "h = bash('sleep 2; echo reveil')\nprint('lancé')"),
        { text: "lancé" },
        { text: "réveil fini" },
        { text: "suivant fini" },
        // Ticket 26 : une suppression pure en milieu de ligne, après un emoji.
        cell("call-cut", "print(await edit('emoji.txt', 'un ', ''))"),
        { text: "coupe fini" },
      ]);
      await post({ type: "newChat", draft: { text: "", images: [] } });
      await waitFor(async () => {
        const s = (await state()).session;
        return s?.kind === "open" && s.config.permissions === "auto";
      }, "permissions = auto", SLOW);
      await shows("Mode auto sans sandbox", "bandeau");
      await post({ type: "send", text: "auto", images: [] });
      await idle();
      assert.equal((await tool("call-auto"))?.status, "done");
      assert.ok(existsSync(join(workspace, "auto.txt")), "cellule exécutée");
      assert.equal((await state()).badge, undefined, "aucune demande");
    },
  ],
  [
    "tour lancé par l'agent (ticket 25) : avis [bash-done], tour fermé sur responseBoundary, message suivant envoyé tout de suite",
    async () => {
      await post({ type: "send", text: "réveil", images: [] });
      const wake = await waitFor(async () => {
        const turn = (await state()).chat.entries.filter((entry) => entry.kind === "turn").at(-1);
        return turn?.kind === "turn" && turn.wake === "bash" && turn.state === "done" ? turn : undefined;
      }, "tour de réveil fini sur sa frontière", SLOW);
      assert.equal(wake.prompt, undefined, "tour lancé par l'agent, sans message");
      await shows("réveil fini", "réponse du réveil");
      assert.equal((await state()).chat.busy, false, "panneau libre après la frontière");
      const started = Date.now();
      await post({ type: "send", text: "suivant", images: [] });
      await shows("suivant fini", "réponse au message suivant", 15_000);
      await idle();
      console.log(`message suivant : réponse en ${Date.now() - started} ms`);
    },
  ],
  [
    "suppression pure après un emoji (ticket 26) : Revert par un clic, fichier revenu octet pour octet",
    async () => {
      // Sans la colonne du pont, cet « avant » serait ambigu (pas de « Revert ») ; une colonne prise
      // en unités UTF-16 au lieu de points de code remettrait `un ` au mauvais endroit.
      const file = join(workspace, "emoji.txt");
      const original = Buffer.from("garder\n😀 un deux\nfin\n", "utf8");
      writeFileSync(file, original);
      await post({ type: "send", text: "coupe", images: [] });
      await idle();
      await shows("coupe fini", "réponse après la coupe");
      assert.equal(readFileSync(file, "utf8"), "garder\n😀 deux\nfin\n");
      await waitFor(async () => (await tool("call-cut"))?.edits?.[0].revert?.state === "ready", "Revert possible", SLOW);
      const edit = (await tool("call-cut"))!.edits![0];
      assert.deepEqual([edit.shown, edit.created], ["emoji.txt", false]);
      // Par des clics d'utilisateur, sur l'élément réellement sous le pointeur : le travail replié,
      // la carte, puis « Revert ».
      await userClicks("Worked", "Edited", "Revert");
      await waitFor(async () => (await tool("call-cut"))?.edits?.[0].revert?.state === "reverted", "Revert fait");
      assert.ok(readFileSync(file).equals(original), "emoji.txt revenu octet pour octet");
    },
  ],
];

// Ticket 28.

const turns = async (): Promise<Turn[]> => (await state()).chat.entries.filter((entry): entry is Turn => entry.kind === "turn");
const textOf = (turn: Turn) => turn.items.map((item) => (item.kind === "text" ? item.text : "")).join("");

function alive(pid: number | undefined): boolean {
  if (pid === undefined) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

/** Les sessions vivantes du démon de test (`list --json`) : identifiant du fichier, worker. */
function daemonSessions(): { sessionId: string; workerPid?: number }[] {
  const out = execFileSync(binary, ["list", "--json"], { cwd: workspace, env: process.env, encoding: "utf8", timeout: 30_000 });
  return (JSON.parse(out) as { sessions?: { sessionId: string; workerPid?: number }[] }).sessions ?? [];
}

/** Plantage du pont de la conversation du panneau : SIGKILL, puis « Agent stopped ». */
async function crashBridge(): Promise<void> {
  const pid = (await state()).pid;
  assert.ok(pid, "pid de l'agent");
  process.kill(pid, "SIGKILL");
  await waitFor(async () => (await state()).agent.kind === "failed", "agent arrêté", SLOW);
  await shows("Agent stopped", "état arrêté");
}

/** La conversation revenue (rejeu fini, session ouverte). */
async function loaded(): Promise<void> {
  await waitFor(async () => {
    const s = await state();
    return s.session?.kind === "open" && !s.chat.loading && s.rendered;
  }, "conversation rechargée", SLOW);
}

/** Le menu de l'historique ouvert par un clic, la ligne `title` listée. */
async function historyShows(title: string): Promise<void> {
  await userClicks("History");
  await waitFor(async () => (await state()).history.chats?.some((chat) => chat.title === title), `« ${title} » dans l'historique`, SLOW);
  await shows(title, `ligne « ${title} » affichée`);
}

/**
 * Les fichiers nommés d'après une session (`<id>.*`) à côté de son fichier, hors son journal d'usage,
 * gardé à la suppression (C4) : le fichier lui-même, l'instantané de fenêtre, l'état de scan.
 */
function sessionFiles(path: string): string[] {
  const dir = join(path, "..");
  const stem = basename(path, ".jsonl");
  return readdirSync(dir)
    .filter((name) => name.startsWith(`${stem}.`) && !name.endsWith(".usage.ndjson"))
    .sort();
}

/** Un second client ACP (une autre fenêtre) : le vrai binaire, mêmes HOME, TMPDIR et dossier. */
class OtherWindow {
  private readonly proc: ChildProcessWithoutNullStreams;
  private readonly pending = new Map<number, (message: Record<string, unknown>) => void>();
  private nextId = 1;

  constructor(args: string[] = []) {
    const env = { ...process.env };
    delete env.PRIME_AGENT_KERNEL_PYTHON;
    this.proc = spawn(binary, ["--mode", "acp", ...args], { cwd: workspace, env, stdio: "pipe" });
    this.proc.stderr.resume();
    createInterface({ input: this.proc.stdout }).on("line", (line) => {
      let message: Record<string, unknown>;
      try {
        message = JSON.parse(line) as Record<string, unknown>;
      } catch {
        return;
      }
      if (message.method === undefined && typeof message.id === "number") {
        this.pending.get(message.id)?.(message);
        this.pending.delete(message.id);
      }
    });
  }

  call(method: string, params: Record<string, unknown>): Promise<Record<string, unknown>> {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`${method} : pas de réponse`)), SLOW);
      this.pending.set(id, (message) => {
        clearTimeout(timer);
        resolve(message);
      });
      this.proc.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
    });
  }

  /** Fin de stdin, comme une fenêtre fermée. */
  async close(): Promise<void> {
    if (this.proc.exitCode !== null) return;
    const exited = new Promise((resolve) => this.proc.once("exit", resolve));
    this.proc.stdin.end();
    await Promise.race([exited, sleep(15_000)]);
    if (this.proc.exitCode === null) this.proc.kill("SIGKILL");
  }
}

let relaunched: { pid?: number; saved?: { id: string; path: string } } = {};
let background: { pid?: number; saved?: { id: string; path: string } } = {};

const resumeTests: [string, () => Promise<void>][] = [
  [
    "conversation neuve quittée vide (D1, D2) : fermée, supprimée, sa session arrêtée dans le démon",
    async () => {
      // Le script des conversations qui suivent : lu par chaque worker à sa création.
      writeScript([
        cell("call-r1", "h = bash('sleep 3; echo fini')\nprint('lancé')"),
        { text: "relance lancé" },
        { text: "réveil pendant la fermeture" },
        { text: "après relance" },
        cell("call-r2", "h = bash('sleep 2; echo deux')\nprint('lancé 2')"),
        { text: "lancé 2" },
        cell("call-long", "import time\ntime.sleep(15)\nprint('long fini')"),
        { text: "réveil long fini" },
        { text: "en attente fini" },
        cell("call-r3", "h = bash('sleep 2; echo trois')\nprint('lancé 3')"),
        { text: "lancé 3" },
        cell("call-stop", "import time\ntime.sleep(8)\nprint('jamais')"),
        { text: "jamais : arrêté" },
      ]);
      await post({ type: "newChat", draft: { text: "", images: [] } });
      await ready();
      const empty = (await state()).saved;
      assert.ok(empty, "fichier de la conversation vide");
      const worker = daemonSessions().find((row) => row.sessionId === empty.id)?.workerPid;
      assert.ok(alive(worker), "worker de la conversation vide");
      await post({ type: "newChat", draft: { text: "", images: [] } });
      await ready();
      await waitFor(() => !daemonSessions().some((row) => row.sessionId === empty.id), "session vide arrêtée dans le démon", SLOW);
      await waitFor(() => !alive(worker), "worker arrêté", 15_000);
      await waitFor(() => !existsSync(empty.path), "fichier vide retiré", 15_000);
      assert.deepEqual(sessionFiles(empty.path), [], "rien de la conversation vide ne reste");
    },
  ],
  [
    "Restart après un plantage du pont : la conversation revient, avec le tour lancé par l'agent pendant que le chat était fermé",
    async () => {
      await post({ type: "send", text: "relance", images: [] });
      await idle();
      await shows("relance lancé", "réponse avant le plantage");
      const saved = (await state()).saved;
      assert.ok(saved, "fichier de la conversation");
      await crashBridge();
      // Le worker vit sans client : le réveil de son bash() fait un tour, écrit dans le fichier.
      await waitFor(() => readFileSync(saved.path, "utf8").includes("réveil pendant la fermeture"), "tour de réveil fait pendant la fermeture", SLOW);
      await userClicks("Restart");
      await loaded();
      const back = await turns();
      assert.equal(back[0]?.prompt?.text, "relance");
      assert.equal(back[0]?.state, "done");
      const wake = back.find((turn) => turn.wake === "bash");
      assert.ok(wake, `tour de réveil rejoué : ${JSON.stringify(back.map((turn) => [turn.prompt?.text, turn.wake, turn.state]))}`);
      assert.equal(wake.state, "done");
      assert.equal(wake.prompt, undefined);
      assert.match(textOf(wake), /réveil pendant la fermeture/);
      await shows("réveil pendant la fermeture", "réveil affiché");
      await post({ type: "send", text: "suite", images: [] });
      await shows("après relance", "message suivant accepté");
      await idle();
      relaunched = { pid: (await state()).pid, saved: (await state()).saved };
    },
  ],
  [
    "tour de l'agent en cours au chargement (D4) : affiché en cours, message tapé mis en attente jusqu'à sa fin",
    async () => {
      await post({ type: "send", text: "deux", images: [] });
      await idle();
      await shows("lancé 2", "réponse avant le plantage");
      const saved = (await state()).saved!;
      await crashBridge();
      await waitFor(() => readFileSync(saved.path, "utf8").includes("call-long"), "cellule longue du réveil en cours", SLOW);
      await userClicks("Restart");
      await loaded();
      const last = (await turns()).at(-1);
      assert.equal(last?.wake, "bash", "dernier tour : le réveil");
      assert.equal(last?.state, "running", "réveil en cours après le chargement");
      assert.equal((await state()).chat.busy, true, "le chat est occupé (Stop affiché)");
      await post({ type: "send", text: "attente", images: [] });
      await waitFor(async () => (await state()).chat.queue.length === 1, "message en attente", 5_000);
      await waitFor(async () => (await turns()).find((turn) => turn.wake === "bash" && turn.state !== "running" && textOf(turn).includes("réveil long fini")), "réveil fini", SLOW);
      const woke = (await turns()).filter((turn) => turn.wake === "bash").at(-1)!;
      assert.equal(woke.state, "done");
      assert.equal(woke.ms, undefined, "sans durée");
      await shows("en attente fini", "message envoyé après la fin du tour");
      await idle();
      relaunched = { pid: (await state()).pid, saved };
    },
  ],
  [
    "tour de l'agent en cours au chargement (D4) : Stop par un clic, le tour s'arrête dans l'agent",
    async () => {
      await post({ type: "send", text: "trois", images: [] });
      await idle();
      await shows("lancé 3", "réponse avant le plantage");
      const saved = (await state()).saved!;
      await crashBridge();
      await waitFor(() => readFileSync(saved.path, "utf8").includes("call-stop"), "cellule du réveil en cours", SLOW);
      await userClicks("Restart");
      await loaded();
      assert.equal((await turns()).at(-1)?.state, "running", "réveil en cours après le chargement");
      const started = Date.now();
      await userClicks("Stop");
      await idle(10_000);
      assert.equal((await turns()).at(-1)?.state, "cancelled");
      // Arrêté dans l'agent : sans arrêt, la cellule (8 s) finirait et la réponse suivante viendrait.
      await sleep(12_000);
      const text = readFileSync(saved.path, "utf8");
      assert.ok(!text.includes("jamais : arrêté"), "le tour ne continue pas après Stop");
      console.log(`Stop après chargement : tour fini en ${Date.now() - started} ms`);
      relaunched = { pid: (await state()).pid, saved };
    },
  ],
  [
    "historique : une deuxième conversation, puis retour à la première, telle quelle",
    async () => {
      writeScript([{ text: "autre fini" }]);
      await post({ type: "newChat", draft: { text: "", images: [] } });
      await ready();
      await post({ type: "send", text: "autre", images: [] });
      await idle();
      await shows("autre fini", "réponse de la deuxième conversation");
      background = { pid: (await state()).pid, saved: (await state()).saved };
      await historyShows("relance");
      await userClicks("relance");
      await waitFor(async () => (await state()).saved?.id === relaunched.saved?.id, "première conversation dans le panneau", SLOW);
      const s = await state();
      assert.equal(s.pid, relaunched.pid, "même agent : affichée telle quelle");
      assert.equal(s.chat.loading, false);
      assert.equal((await turns())[0]?.prompt?.text, "relance");
      await shows("lancé 3", "conversation affichée");
      assert.ok(alive(background.pid), "la deuxième reste ouverte en fond");
    },
  ],
  [
    "double ouverture (C12) : refus affiché, agent arrêté ; libérée, Restart la reprend depuis son fichier",
    async () => {
      writeScript([{ text: "tenue ailleurs fini" }]);
      const holder = new OtherWindow();
      try {
        await holder.call("initialize", { protocolVersion: 1, clientCapabilities: {} });
        const opened = (await holder.call("session/new", { cwd: workspace, mcpServers: [] })).result as { sessionId: string };
        const answer = await holder.call("session/prompt", { sessionId: opened.sessionId, prompt: [{ type: "text", text: "tenue ailleurs" }] });
        assert.equal((answer.result as { stopReason?: string })?.stopReason, "end_turn", JSON.stringify(answer));
        await historyShows("tenue ailleurs");
        await userClicks("tenue ailleurs");
        await waitFor(async () => {
          const s = await state();
          // `session` : la session ne s'est pas ouverte (D3) ; lu sans le type, pour tourner aussi sans le correctif.
          return s.agent.kind === "failed" && (s.agent as { session?: boolean }).session === true ? s : undefined;
        }, "session refusée", SLOW);
        const shown = await shows("This conversation is open in another window.", "raison du refus affichée");
        assert.match(shown, /Conversation not opened/);
        assert.equal((await state()).pid, undefined, "agent arrêté");
      } finally {
        await holder.close();
      }
      await userClicks("Restart");
      await loaded();
      const back = await turns();
      assert.equal(back[0]?.prompt?.text, "tenue ailleurs");
      await shows("tenue ailleurs fini", "conversation reprise");
    },
  ],
  [
    "suppression d'une conversation de fond : son agent arrêté, sa session arrêtée dans le démon, son fichier retiré",
    async () => {
      const { pid, saved } = background;
      assert.ok(saved && pid, "conversation de fond");
      const worker = daemonSessions().find((row) => row.sessionId === saved.id)?.workerPid;
      assert.ok(alive(worker), "worker de la conversation de fond");
      await historyShows("autre");
      await userClicks("Delete autre", "Delete");
      await waitFor(async () => {
        const chats = (await state()).history.chats;
        return chats !== null && !chats.some((chat) => chat.id === saved.id);
      }, "ligne retirée de l'historique", SLOW);
      assert.equal((await state()).history.error, undefined);
      assert.equal(existsSync(saved.path), false, "fichier retiré");
      assert.deepEqual(sessionFiles(saved.path), [], "rien de la conversation supprimée ne reste");
      assert.equal(existsSync(join(saved.path, "..", "..", "session-artifacts", saved.id)), false, "instantané retiré");
      assert.ok(!daemonSessions().some((row) => row.sessionId === saved.id), "session absente du démon");
      await waitFor(() => !alive(worker) && !alive(pid), "agent et worker arrêtés", 15_000);
    },
  ],
  [
    "fermeture de VS Code avec une conversation neuve vide : sa session et son fichier retirés ; une vide tenue par une autre fenêtre reste",
    async () => {
      // X : neuve, vide, tenue par un autre client ; quittée pour Y (D2), elle doit rester.
      await post({ type: "newChat", draft: { text: "", images: [] } });
      await ready();
      const held = (await state()).saved;
      assert.ok(held, "fichier de la conversation tenue ailleurs");
      const holder = new OtherWindow(["--resume", held.path]);
      try {
        await holder.call("initialize", { protocolVersion: 1, clientCapabilities: {} });
        // Y : neuve, vide, dans le panneau à la fermeture.
        await post({ type: "newChat", draft: { text: "", images: [] } });
        await ready();
        const empty = (await state()).saved;
        assert.ok(empty && empty.id !== held.id, "fichier de la conversation vide du panneau");
        assert.ok(daemonSessions().some((row) => row.sessionId === empty.id), "session vide dans le démon");
        // Le chemin de `deactivate` (fermeture ou « Reload Window ») : chaque agent s'arrête.
        await vscode.commands.executeCommand("yusagent._test.stopAll");
        assert.ok(existsSync(empty.path.replace(/\.jsonl$/, ".window-cache.json")), "instantané de fenêtre présent avant la fermeture");
        await waitFor(() => !existsSync(empty.path), "fichier de la conversation vide retiré", SLOW);
        await waitFor(() => !daemonSessions().some((row) => row.sessionId === empty.id), "session vide arrêtée dans le démon", SLOW);
        assert.deepEqual(sessionFiles(empty.path), [], "rien de la conversation vide ne reste");
        assert.ok(existsSync(held.path), "la conversation tenue ailleurs garde son fichier");
        assert.ok(daemonSessions().some((row) => row.sessionId === held.id), "la conversation tenue ailleurs garde sa session");
      } finally {
        await holder.close();
      }
    },
  ],
];
tests.push(...resumeTests);

/** Après un échec : chaque cellule de la conversation (statut, sortie, fichiers), et le workspace. */
async function diagnose(): Promise<void> {
  for (const entry of (await state()).chat.entries) {
    if (entry.kind !== "turn") continue;
    for (const item of entry.items) {
      if (item.kind !== "tool") continue;
      const edits = item.edits?.map((edit) => ({ path: edit.path, revert: edit.revert }));
      console.log(`  cellule ${item.id} ${item.status} : ${JSON.stringify({ output: item.output?.slice(0, 600), edits })}`);
    }
  }
  console.log(`  workspace : ${JSON.stringify(readdirSync(workspace))} ; cwd du binaire : ${process.env.YUSAGENT_E2E_WORKSPACE}`);
}

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
      await diagnose();
      if ((await state()).chat.busy) {
        await post({ type: "cancel" });
        await idle(15_000).catch(() => undefined);
      }
    }
  }
  // Arrêt : les agents de l'extension (fin de stdin), le superviseur de ce TMPDIR, puis plus aucun
  // processus du binaire.
  await vscode.commands.executeCommand("yusagent._test.stopAll");
  try {
    execFileSync("pkill", ["-TERM", "-f", `${temp}/yusagent-`]);
  } catch {
    // Rien à arrêter.
  }
  let left: string[] = [];
  try {
    left = await waitFor(() => (leftovers().length === 0 ? [] : false), "processus arrêtés", 30_000);
  } catch {
    left = leftovers();
  }
  console.log(`processus restants : ${left.length === 0 ? "aucun" : left.join(", ")}`);
  if (left.length > 0) failures.push("processus restants");
  console.log(`e2e vrai binaire : ${tests.length - failures.length}/${tests.length} réussis`);
  if (failures.length) throw new Error(`${failures.length} test(s) e2e en échec : ${failures.join(" ; ")}`);
}

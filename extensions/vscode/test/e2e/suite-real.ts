// yusAgent (YA-10, ticket 24, PR 3, étape 6) : e2e dans VS Code contre le vrai binaire (debug),
// provider factice, kernel réel. Lancé par `run-real.ts` (CI seulement).
// - session 1 (manuel) : cellule avec `bash()` (accord par le bouton de la boîte), `edit()` puis
//   « Revert », refus (fichier absent, « Declined », le tour s'arrête), texte `[user-reverted]` dans
//   le message suivant (fichier de session), Stop pendant une cellule ;
// - session 2 (auto) : bandeau, cellule exécutée sans demande ;
// - fin : démon arrêté, aucun processus restant.
// Le tour lancé par l'agent (avis `[bash-done]`) est au ticket 25.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import * as vscode from "vscode";

import type { AgentState, ChatState, Probe, ProbeInput, SessionState, TurnItem, WebviewMessage } from "../../src/protocol";
import { waitFor } from "../helpers";

interface State {
  agent: AgentState;
  session: SessionState | null;
  chat: ChatState;
  rendered: boolean;
  badge?: number;
}

type Tool = Extract<TurnItem, { kind: "tool" }>;

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
      // Dépliée par des clics : la ligne du bash(), exit puis durée séparés.
      await userClicks("Worked", "Ran");
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
      writeScript([cell("call-auto", "open('auto.txt', 'w').write('x')"), { text: "auto fini" }]);
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
];

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

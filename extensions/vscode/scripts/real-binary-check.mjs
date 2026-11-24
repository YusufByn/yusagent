#!/usr/bin/env node
// Contrôle du code de l'extension contre le vrai binaire (ticket 24, PR I, règle 13).
//
// Usage : npm run compile-tests && node scripts/real-binary-check.mjs <chemin absolu de yusagent> [venv du kernel]
//
// Lance le code de l'extension hors VS Code (conversations, agent, session, chat, fournisseurs,
// réglages, historique ; module `vscode` remplacé par le bouchon des tests unitaires) sur le
// binaire, en permissions « manual » puis « auto » (réglage global d'un dossier d'agent temporaire).
// Vérifie : session ouverte avec les seules trames de l'extension, options annoncées, bandeau qui
// suit `permissions`, `set_config_option` sur l'effort, message `/login` sur l'échec
// d'authentification, arrêt par fin de stdin, aucun processus restant.
//
// Aucun modèle n'est joignable (règle 6) : HOME et dossier d'agent vides (aucun identifiant),
// environnement réduit (aucune clé d'API), binaire sous `sandbox-exec` (réseau limité à localhost,
// profil du banc de capture), démon isolé (`--daemon-socket`). Le binaire est lancé par un petit
// script intermédiaire : c'est son chemin qu'affiche le message `/login`.
//
// PR 3 (cas « cellules ») : provider factice (`PRIME_AGENT_FAUX_SCRIPT`, aucun modèle), kernel réel
// sur un venv déjà préparé (le binaire tourne sans réseau) : par défaut celui du banc de capture,
// `~/.cache/yusagent-capture/home/.yusagent/kernel-venv`, passé par `PRIME_AGENT_KERNEL_VENV` (pas
// `PRIME_AGENT_KERNEL_PYTHON`, qui désactiverait les skills Python, dont `edit`). En « manual » :
// cellule avec `bash()` (demande d'accord, réponse « Allow »), carte avec la commande et son code
// de sortie ; `edit()` qui crée un fichier, vrai diff, « Revert » (fichier retiré) ; refus (fichier
// absent, « Declined », le tour s'arrête) et `[user-reverted]` à la fin du message, dans le fichier
// de session ; Stop pendant une cellule.

import { execFileSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const TMP = tmpdir(); // figé avant que les cas ne réduisent l'environnement
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const sandboxProfile = resolve(root, "..", "..", "bench", "tools", "capture", "sandbox.sb");
require(join(root, "out", "test", "unit", "setup.js"));
const { Conversations } = require(join(root, "out", "src", "conversations.js"));
const { ShownAgent } = require(join(root, "out", "src", "shown.js"));
const { Providers } = require(join(root, "out", "src", "providers.js"));
const { Settings } = require(join(root, "out", "src", "settings.js"));
const { History } = require(join(root, "out", "src", "history.js"));
const { authText } = require(join(root, "out", "src", "ya", "auth.js"));

const HOME_DIR = process.env.HOME;
const binary = process.argv[2];
const kernelVenv = process.argv[3] ?? join(HOME_DIR ?? "", ".cache", "yusagent-capture", "home", ".yusagent", "kernel-venv");
if (!binary || !isAbsolute(binary) || !existsSync(binary)) {
  console.error("usage : node scripts/real-binary-check.mjs <chemin absolu de yusagent>");
  process.exit(2);
}

const log = new Proxy({}, { get: () => () => undefined });
const failures = [];
const check = (ok, label) => {
  console.log(`  ${ok ? "oui" : "NON"} : ${label}`);
  if (!ok) failures.push(label);
};

async function waitFor(probe, what, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const value = probe();
    if (value) return value;
    if (Date.now() > deadline) throw new Error(`délai dépassé : ${what}`);
    await new Promise((resolveWait) => setTimeout(resolveWait, 100));
  }
}

function pgrep(pattern) {
  try {
    return execFileSync("pgrep", ["-f", pattern], { encoding: "utf8" }).split("\n").filter(Boolean);
  } catch {
    return [];
  }
}

async function runCase(permissions) {
  console.log(`\n== permissions « ${permissions} » ==`);
  const base = mkdtempSync(join(TMP, "ya-ext-"));
  const home = join(base, "home");
  const agentDir = join(base, "agent");
  const project = join(base, "project");
  const socket = join(base, "d.sock");
  for (const dir of [home, agentDir, project, join(base, "tmp")]) mkdirSync(dir, { recursive: true });
  writeFileSync(join(agentDir, "settings.json"), JSON.stringify({ permissions, cacheRetention: "short" }) + "\n");
  const wrapper = join(base, "yusagent");
  writeFileSync(
    wrapper,
    `#!/bin/sh\nexec /usr/bin/sandbox-exec -f '${sandboxProfile}' '${binary}' --daemon-socket '${socket}' "$@"\n`,
  );
  chmodSync(wrapper, 0o755);
  // L'agent prend l'environnement du processus (comme l'extension celui de VS Code) : réduit ici.
  for (const key of Object.keys(process.env)) delete process.env[key];
  Object.assign(process.env, {
    PATH: "/usr/bin:/bin:/usr/sbin:/sbin",
    HOME: home,
    TMPDIR: join(base, "tmp"),
    LANG: "en_US.UTF-8",
    DO_NOT_TRACK: "1",
    PI_OFFLINE: "1",
    PI_SKIP_VERSION_CHECK: "1",
    PI_CACHE_RETENTION: "short",
    PRIME_AGENT_CODING_AGENT_DIR: agentDir,
  });
  const shown = new ShownAgent(log);
  new Providers(shown, log);
  new Settings(shown, log);
  const files = { cwd: project, links: () => [], list: async () => ({ files: [], open: [] }) };
  const conversations = new Conversations(wrapper, project, "0", files, shown, log);
  new History(shown, conversations, project, log);
  try {
    conversations.start();
    const conversation = conversations.shown;
    await waitFor(
      () => conversation.session.current?.kind === "open" || conversation.agent.current.kind === "failed",
      "ouverture",
      90_000,
    );
    const session = conversation.session.current;
    check(session?.kind === "open", `session ouverte (${session?.kind ?? conversation.agent.current.kind})`);
    if (session?.kind !== "open") return;
    const config = session.config;
    console.log(`  modèle ${config.model?.current} (${config.model?.choices.length} choix), effort ${config.effort?.current}, permissions ${config.permissions?.current}`);
    check(config.model !== null, "sélecteur de modèle annoncé");
    check(config.permissions?.current === permissions, `permissions annoncé = ${permissions}`);
    check((config.permissions?.current === "auto") === (permissions === "auto"), `bandeau ${permissions === "auto" ? "affiché" : "absent"}`);

    if (config.effort) {
      const target = config.effort.levels.find((level) => level !== config.effort.current);
      await conversation.session.setOption({ configId: "thought_level", value: target });
      const after = conversation.session.current;
      const now = after?.kind === "open" ? after.config.effort?.current : undefined;
      check(now === target, `effort ${config.effort.current} → ${target} par set_config_option (lu : ${now})`);
    } else {
      console.log("  (pas de sélecteur d'effort pour ce modèle)");
    }

    conversation.chat.send("bonjour", []);
    await waitFor(() => !conversation.chat.current.busy && conversation.chat.current.entries.length > 0, "fin du tour", 60_000);
    const turn = conversation.chat.current.entries.filter((entry) => entry.kind === "turn").at(-1);
    console.log(`  tour : ${turn?.state} ; ${turn?.error}`);
    check(turn?.state === "failed" && turn.error === authText(wrapper), "message /login avec le chemin lancé");

    const began = Date.now();
    await conversations.stopAll();
    check(Date.now() - began < 10_000, `arrêt par fin de stdin en ${Date.now() - began} ms`);
  } catch (error) {
    check(false, String(error));
  } finally {
    await conversations.stopAll();
    try {
      execFileSync("pkill", ["-TERM", "-f", "--", `--daemon-socket ${socket}`]);
    } catch {
      // aucun démon à arrêter
    }
    await waitFor(() => pgrep(socket).length === 0, "arrêt du démon", 30_000).catch(() => undefined);
    check(pgrep(socket).length === 0, "aucun processus restant");
    rmSync(base, { recursive: true, force: true });
  }
}

/** Le texte de tous les fichiers de session sous `dir`. */
function sessionText(dir) {
  const out = [];
  const walk = (at) => {
    for (const name of readdirSync(at)) {
      const path = join(at, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (name.endsWith(".jsonl")) out.push(readFileSync(path, "utf8"));
    }
  };
  walk(dir);
  return out.join("\n");
}

async function runCells() {
  console.log("\n== PR 3 : cellules, boîte, diffs (manual, provider factice, kernel réel) ==");
  if (!existsSync(join(kernelVenv, "bin", "python"))) {
    check(false, `venv du kernel introuvable : ${kernelVenv} (lancer d'abord bench/tools/capture/run.py sur ce binaire)`);
    return;
  }
  const base = mkdtempSync(join(TMP, "ya-ext-"));
  const home = join(base, "home");
  const agentDir = join(base, "agent");
  const project = join(base, "project");
  const socket = join(base, "d.sock");
  const script = join(base, "worker-script.json");
  for (const dir of [home, agentDir, project, join(base, "tmp")]) mkdirSync(dir, { recursive: true });
  writeFileSync(join(agentDir, "settings.json"), JSON.stringify({ permissions: "manual", cacheRetention: "short" }) + "\n");
  const cell = (id, code) => ({ content: [{ type: "toolCall", id, name: "python_repl", arguments: { code } }] });
  writeFileSync(
    script,
    JSON.stringify({
      engine: "faux",
      responses: [
        cell("call-bash", "r = await bash('echo lu')\nprint('LU', r.exit_code)"),
        { text: "bash fini" },
        cell("call-edit", "print(await edit('ok.txt', '', 'oui\\n'))"),
        { text: "edit fini" },
        cell("call-deny", "open('refuse.txt', 'w').write('x')"),
        { text: "jamais demandé : le tour s'arrête après un refus" },
        cell("call-slow", "import time\ntime.sleep(30)"),
        { text: "jamais : arrêté" },
      ],
    }),
  );
  const wrapper = join(base, "yusagent");
  writeFileSync(
    wrapper,
    `#!/bin/sh\nexec /usr/bin/sandbox-exec -f '${sandboxProfile}' '${binary}' --daemon-socket '${socket}' "$@"\n`,
  );
  chmodSync(wrapper, 0o755);
  for (const key of Object.keys(process.env)) delete process.env[key];
  Object.assign(process.env, {
    PATH: "/usr/bin:/bin:/usr/sbin:/sbin",
    HOME: home,
    TMPDIR: join(base, "tmp"),
    LANG: "en_US.UTF-8",
    DO_NOT_TRACK: "1",
    PI_OFFLINE: "1",
    PI_SKIP_VERSION_CHECK: "1",
    PI_CACHE_RETENTION: "short",
    PRIME_AGENT_CODING_AGENT_DIR: agentDir,
    PRIME_AGENT_FAUX_SCRIPT: script,
    PRIME_AGENT_KERNEL_VENV: kernelVenv,
  });
  const shown = new ShownAgent(log);
  new Providers(shown, log);
  new Settings(shown, log);
  const files = { cwd: project, links: () => [], list: async () => ({ files: [], open: [] }) };
  const conversations = new Conversations(wrapper, project, "0", files, shown, log);
  new History(shown, conversations, project, log);
  const tool = (conversation, id) => {
    for (const entry of conversation.chat.current.entries) {
      if (entry.kind !== "turn") continue;
      const found = entry.items.find((item) => item.kind === "tool" && item.id === id);
      if (found) return found;
    }
    return undefined;
  };
  try {
    conversations.start();
    const conversation = conversations.shown;
    const { chat, agent } = conversation;
    await waitFor(() => conversation.session.current?.kind === "open", "ouverture", 90_000);
    const idle = (what) => waitFor(() => !chat.current.busy, what, 120_000);
    const allow = async (id, optionId) => {
      await waitFor(() => tool(conversation, id)?.status === "pending" && agent.permissions.asks.some((ask) => ask.toolCallId === id), `demande ${id}`, 120_000);
      check(agent.permissions.answer(id, optionId), `${id} : demande d'accord reçue, réponse ${optionId}`);
    };

    chat.send("cellule bash", []);
    await allow("call-bash", "allow-once");
    await idle("tour bash");
    const bash = tool(conversation, "call-bash");
    const run = bash?.runs?.[0];
    console.log(`  carte : ${bash?.status}, ${bash?.cell?.kind} \`${bash?.cell?.command}\`, bash() : ${JSON.stringify(run)}`);
    check(bash?.status === "done" && run?.command === "echo lu" && run.exitCode === 0 && run.output === "lu\n", "carte : commande, code de sortie 0, sortie");

    chat.send("edit", []);
    await allow("call-edit", "allow-once");
    await idle("tour edit");
    const file = join(project, "ok.txt");
    check(existsSync(file) && readFileSync(file, "utf8") === "oui\n", "edit() : ok.txt créé dans le projet");
    await waitFor(() => tool(conversation, "call-edit")?.edits?.[0]?.revert?.state === "ready", "Revert possible", 30_000);
    const edit = tool(conversation, "call-edit").edits[0];
    console.log(`  diff : ${edit.shown}, créé ${edit.created}, lignes ${JSON.stringify(edit.lines)}`);
    await chat.yaRevert("call-edit", edit.path);
    check(tool(conversation, "call-edit").edits[0].revert?.state === "reverted" && !existsSync(file), "Revert : ok.txt retiré");

    chat.send("refus", []);
    await allow("call-deny", "reject-once");
    await idle("tour refus");
    const denied = tool(conversation, "call-deny");
    check(denied?.status === "failed" && denied.declined === true, "refus : carte Declined");
    check(!existsSync(join(project, "refuse.txt")), "refus : la cellule n'a pas tourné");
    check(sessionText(agentDir).includes("refus\\n\\n[user-reverted] ok.txt"), "[user-reverted] ok.txt à la fin du message (fichier de session)");
    chat.send("suivant", []);
    await idle("tour suivant");
    const texts = chat.current.entries.filter((entry) => entry.kind === "turn").at(-1)?.items.filter((item) => item.kind === "text").map((item) => item.text).join("");
    check(texts?.includes("jamais demandé"), "le tour s'était arrêté après le refus (réponse suivante non demandée)");

    chat.send("lent", []);
    await allow("call-slow", "allow-once");
    await waitFor(() => tool(conversation, "call-slow")?.status === "running", "cellule en cours", 30_000);
    await new Promise((resolveWait) => setTimeout(resolveWait, 1_000));
    const stopped = Date.now();
    chat.cancel();
    await idle("Stop");
    const turn = chat.current.entries.filter((entry) => entry.kind === "turn").at(-1);
    check(turn?.state === "cancelled", `Stop : tour annulé (panneau libre en ${Date.now() - stopped} ms)`);
  } catch (error) {
    check(false, String(error));
  } finally {
    await conversations.stopAll();
    try {
      execFileSync("pkill", ["-TERM", "-f", "--", `--daemon-socket ${socket}`]);
    } catch {
      // aucun démon à arrêter
    }
    await waitFor(() => pgrep(socket).length === 0, "arrêt du démon", 30_000).catch(() => undefined);
    check(pgrep(socket).length === 0, "aucun processus restant");
    rmSync(base, { recursive: true, force: true });
  }
}

console.log(`binaire : ${binary}`);
console.log(`version : ${execFileSync(binary, ["--version"], { encoding: "utf8", env: { PATH: "/usr/bin:/bin" } }).trim()}`);
await runCase("manual");
await runCase("auto");
await runCells();
console.log(failures.length ? `\nNON CONFORME (${failures.length})` : "\nCONFORME");
process.exit(failures.length ? 1 : 0);

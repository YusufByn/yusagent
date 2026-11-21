#!/usr/bin/env node
// Contrôle du code de l'extension contre le vrai binaire (ticket 24, PR I, règle 13).
//
// Usage : npm run compile-tests && node scripts/real-binary-check.mjs <chemin absolu de yusagent>
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

import { execFileSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
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

const binary = process.argv[2];
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
    console.log(`  modèle ${config.model?.current} (${config.model?.choices.length} choix), effort ${config.effort?.current}, permissions ${config.permissions}`);
    check(config.model !== null, "sélecteur de modèle annoncé");
    check(config.permissions === permissions, `permissions annoncé = ${permissions}`);
    check((config.permissions === "auto") === (permissions === "auto"), `bandeau ${permissions === "auto" ? "affiché" : "absent"}`);

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

console.log(`binaire : ${binary}`);
console.log(`version : ${execFileSync(binary, ["--version"], { encoding: "utf8", env: { PATH: "/usr/bin:/bin" } }).trim()}`);
await runCase("manual");
await runCase("auto");
console.log(failures.length ? `\nNON CONFORME (${failures.length})` : "\nCONFORME");
process.exit(failures.length ? 1 : 0);

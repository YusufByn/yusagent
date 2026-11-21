#!/usr/bin/env node
// Contrôle du client de l'extension contre le vrai binaire (ticket 24, PR 2, règle 13).
//
// Usage : npm run compile-tests && node scripts/real-binary-check.mjs <chemin absolu de yusagent>
//
// Lance `ChatSession` (le code de l'extension, hors VS Code) sur le binaire, deux fois :
// permissions « manual » puis « auto » (réglage global d'un dossier d'agent temporaire). Vérifie :
// session ouverte avec les trames de l'extension, sélecteurs annoncés, bandeau qui suit
// `permissions`, `set_config_option` sur l'effort, message `/login` sur l'échec d'authentification.
//
// Aucun modèle n'est joignable (règle 6) : HOME et dossier d'agent vides (aucun identifiant),
// environnement réduit (aucune clé d'API), binaire sous `sandbox-exec` (réseau limité à
// localhost, profil du banc de capture), démon isolé (`--daemon-socket`). Le binaire est lancé
// par un petit script intermédiaire : c'est son chemin qu'affiche le message `/login`.

import { execFileSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const sandboxProfile = resolve(root, "..", "..", "bench", "tools", "capture", "sandbox.sb");
const { ChatSession } = require(join(root, "out", "src", "session.js"));
const { autoWithoutSandbox, TEXTS } = require(join(root, "out", "src", "shared", "state.js"));

const binary = process.argv[2];
if (!binary || !isAbsolute(binary) || !existsSync(binary)) {
  console.error("usage : node scripts/real-binary-check.mjs <chemin absolu de yusagent>");
  process.exit(2);
}

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
  const base = mkdtempSync(join(tmpdir(), "ya-ext-"));
  const home = join(base, "home");
  const agent = join(base, "agent");
  const project = join(base, "project");
  const socket = join(base, "d.sock");
  for (const dir of [home, agent, project, join(base, "tmp")]) mkdirSync(dir, { recursive: true });
  writeFileSync(join(agent, "settings.json"), JSON.stringify({ permissions, cacheRetention: "short" }) + "\n");
  const wrapper = join(base, "yusagent");
  writeFileSync(
    wrapper,
    `#!/bin/sh\nexec /usr/bin/sandbox-exec -f '${sandboxProfile}' '${binary}' --daemon-socket '${socket}' "$@"\n`,
  );
  chmodSync(wrapper, 0o755);
  const env = {
    PATH: "/usr/bin:/bin:/usr/sbin:/sbin",
    HOME: home,
    TMPDIR: join(base, "tmp"),
    LANG: "en_US.UTF-8",
    DO_NOT_TRACK: "1",
    PI_OFFLINE: "1",
    PI_SKIP_VERSION_CHECK: "1",
    PI_CACHE_RETENTION: "short",
    PRIME_AGENT_CODING_AGENT_DIR: agent,
  };
  const logs = [];
  const session = new ChatSession({
    binaryPath: wrapper,
    cwd: project,
    workspaceName: "project",
    env,
    log: (line) => logs.push(line),
    onState: () => undefined,
  });
  try {
    void session.start();
    await waitFor(() => ["ready", "stopped"].includes(session.state.phase), "ouverture", 90_000);
    check(session.state.phase === "ready", `session ouverte (phase ${session.state.phase})`);
    const options = session.state.configOptions;
    for (const option of options) {
      console.log(`  option ${option.id} (${option.category}) = ${option.currentValue} ; ${option.options.length} valeurs`);
    }
    check(options.some((option) => option.category === "model"), "sélecteur de modèle annoncé");
    const permissionsOption = options.find((option) => option.id === "permissions");
    check(permissionsOption?.currentValue === permissions, `permissions annoncé = ${permissions}`);
    check(autoWithoutSandbox(session.state) === (permissions === "auto"), `bandeau ${permissions === "auto" ? "affiché" : "absent"}`);

    const effort = options.find((option) => option.category === "thought_level");
    if (effort) {
      const target = effort.options.map((choice) => choice.value).find((value) => value !== effort.currentValue);
      await session.setConfig(effort.id, target);
      const after = session.state.configOptions.find((option) => option.id === effort.id)?.currentValue;
      check(after === target, `effort ${effort.currentValue} → ${target} par set_config_option (lu : ${after})`);
    } else {
      console.log("  (pas de sélecteur d'effort pour ce modèle)");
    }

    await session.send("bonjour");
    const last = session.state.items.at(-1);
    const shown = last?.kind === "auth" ? TEXTS.auth(last.binaryPath) : last?.text;
    console.log(`  dernier message : ${shown}`);
    check(last?.kind === "auth" && last.binaryPath === wrapper, "message /login avec le chemin lancé");
  } catch (error) {
    check(false, String(error));
    for (const line of logs.slice(-20)) console.log(`  [stderr] ${line}`);
  } finally {
    await session.dispose();
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

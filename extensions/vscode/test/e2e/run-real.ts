// yusAgent (YA-10, ticket 24, PR 3, étape 6) : lanceur e2e contre le vrai binaire (CI seulement :
// télécharge VS Code, règle 8). Binaire `YUSAGENT_E2E_BINARY` (debug, construit dans le job),
// provider factice (`PRIME_AGENT_FAUX_SCRIPT`, aucun modèle, règle 6), kernel `PRIME_AGENT_KERNEL_PYTHON`.
// Isolement : HOME et TMPDIR temporaires pour l'hôte d'extension, donc pour le binaire qu'il lance
// (dossier `~/.yusagent`, socket du démon sous `$TMPDIR/yusagent-<uid>/`).

import { mkdirSync, mkdtempSync, realpathSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { runTests } from "@vscode/test-electron";

import { EXTENSION_ROOT } from "../helpers";

const VSCODE_VERSION = process.env.VSCODE_TEST_VERSION ?? "1.146.0";

async function main(): Promise<void> {
  const binary = process.env.YUSAGENT_E2E_BINARY;
  const kernelPython = process.env.PRIME_AGENT_KERNEL_PYTHON;
  if (!binary || !kernelPython) throw new Error("YUSAGENT_E2E_BINARY et PRIME_AGENT_KERNEL_PYTHON sont requis");
  const base = realpathSync(mkdtempSync(join(tmpdir(), "yusagent-e2e-real-")));
  const workspace = join(base, "workspace");
  const userData = join(base, "user-data");
  const home = join(base, "home");
  const temp = join(base, "tmp");
  for (const dir of [workspace, join(userData, "User"), join(home, ".yusagent"), temp]) mkdirSync(dir, { recursive: true });
  writeFileSync(
    join(userData, "User", "settings.json"),
    JSON.stringify({ "yusagent.binaryPath": binary, "security.workspace.trust.enabled": false }),
  );
  await runTests({
    version: VSCODE_VERSION,
    extensionDevelopmentPath: EXTENSION_ROOT,
    extensionTestsPath: join(__dirname, "suite-real.js"),
    launchArgs: [workspace, "--disable-extensions", "--user-data-dir", userData, "--disable-gpu"],
    extensionTestsEnv: {
      HOME: home,
      TMPDIR: temp,
      PRIME_AGENT_FAUX_SCRIPT: join(home, "worker-script.json"),
      PRIME_AGENT_KERNEL_PYTHON: kernelPython,
      // Un démon orphelin sort vite (comme `crates/pa-cli/tests/acp_mode_e2e.rs`).
      PRIME_AGENT_INTERNAL_WORKER_SUPERVISOR_LOST_EXIT_MS: "15000",
      YUSAGENT_EXTENSION_TEST: "1",
      YUSAGENT_E2E_WORKSPACE: workspace,
      YUSAGENT_E2E_HOME: home,
      YUSAGENT_E2E_TMP: temp,
      YUSAGENT_E2E_BINARY: binary,
    },
  });
}

main().catch((error: unknown) => {
  console.error(error);
  process.exit(1);
});

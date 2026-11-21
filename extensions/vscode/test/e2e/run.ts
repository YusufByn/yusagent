// Lanceur e2e (CI seulement : télécharge VS Code, règle 8). Workspace et profil temporaires,
// `yusagent.binaryPath` (réglage utilisateur, portée machine) = l'agent factice.

import { mkdirSync, mkdtempSync, realpathSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { runTests } from "@vscode/test-electron";

import { EXTENSION_ROOT, fakeAgentPath, newControlDir } from "../helpers";

/** Version de VS Code de l'e2e, épinglée (celle de Yusuf au 21/11/2026). */
const VSCODE_VERSION = process.env.VSCODE_TEST_VERSION ?? "1.146.0";

async function main(): Promise<void> {
  const base = realpathSync(mkdtempSync(join(tmpdir(), "yusagent-e2e-")));
  const workspace = join(base, "workspace");
  const userData = join(base, "user-data");
  mkdirSync(workspace, { recursive: true });
  mkdirSync(join(userData, "User"), { recursive: true });
  writeFileSync(
    join(userData, "User", "settings.json"),
    JSON.stringify({ "yusagent.binaryPath": fakeAgentPath(), "security.workspace.trust.enabled": false }),
  );
  const control = newControlDir();
  await runTests({
    version: VSCODE_VERSION,
    extensionDevelopmentPath: EXTENSION_ROOT,
    extensionTestsPath: join(__dirname, "suite.js"),
    launchArgs: [workspace, "--disable-extensions", "--user-data-dir", userData, "--disable-gpu"],
    extensionTestsEnv: {
      FAKE_ACP_DIR: control,
      YUSAGENT_EXTENSION_TEST: "1",
      YUSAGENT_E2E_WORKSPACE: workspace,
    },
  });
}

main().catch((error: unknown) => {
  console.error(error);
  process.exit(1);
});

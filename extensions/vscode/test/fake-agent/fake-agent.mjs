#!/usr/bin/env node
// Agent ACP factice pour les tests de l'extension (ticket 24, PR 2) : rejoue des trames de la forme
// du pont (`crates/pa-daemon/src/acp/`) et enregistre chaque trame reçue. Aucun modèle.
//
// Pilotage : variable FAKE_ACP_DIR (dossier existant).
// - `<dir>/scenario.json` (facultatif), lu à chaque lancement : { "permissions": "manual" | "auto" | null,
//   "protocolVersion": 1 } ; `permissions` absent : « auto », comme le pont sans réglage (ticket 30) ;
// - `<dir>/received-<pid>.jsonl` : chaque ligne reçue, telle quelle ;
// - `<dir>/starts.log` : une ligne par lancement (pid, cwd, arguments).
// Le comportement d'un prompt dépend de son texte (voir `onPrompt`).

import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { createInterface } from "node:readline";

const dir = process.env.FAKE_ACP_DIR;
const scenario =
  dir && existsSync(join(dir, "scenario.json"))
    ? JSON.parse(readFileSync(join(dir, "scenario.json"), "utf8"))
    : {};
if (dir) {
  appendFileSync(
    join(dir, "starts.log"),
    JSON.stringify({
      pid: process.pid,
      cwd: process.cwd(),
      argv: process.argv.slice(2),
      // PR 3 : les variables du kernel reçues (l'extension retire PRIME_AGENT_KERNEL_PYTHON).
      kernelPython: process.env.PRIME_AGENT_KERNEL_PYTHON ?? null,
      kernelVenv: process.env.PRIME_AGENT_KERNEL_VENV ?? null,
    }) + "\n",
  );
}

const SESSION_ID = "fake-session-1";
// Ticket 28 : identifiant du fichier de session, un par lancement (comme le démon).
const FILE_ID = `fake-file-${process.pid}`;
const config = [
  {
    id: "model",
    name: "Model",
    type: "select",
    category: "model",
    // Ticket 31 (X3, C7) : la valeur opaque du pont, la paire sérialisée (`config_options.rs`, `model_value`).
    currentValue: '["capture","model-a"]',
    options: [
      { value: '["capture","model-a"]', name: "Model A (capture)" },
      { value: '["capture","model-b"]', name: "Model B (capture)" },
    ],
  },
  {
    id: "thought_level",
    name: "Reasoning effort",
    type: "select",
    category: "thought_level",
    currentValue: "high",
    options: ["off", "low", "medium", "high", "xhigh"].map((level) => ({ value: level, name: level })),
  },
];
const permissions = scenario.permissions === undefined ? "auto" : scenario.permissions;
if (permissions !== null) {
  config.push({
    id: "permissions",
    name: "Permissions",
    type: "select",
    category: "mode",
    currentValue: permissions,
    options: [
      { value: "manual", name: "Ask before each tool call" },
      { value: "auto", name: "Run tool calls without asking (no sandbox)" },
    ],
  });
}

const send = (message) => process.stdout.write(JSON.stringify({ jsonrpc: "2.0", ...message }) + "\n");
const respond = (id, result) => send({ id, result });
const fail = (id, code, message, data) => send({ id, error: { code, message, ...(data ? { data } : {}) } });
// `_meta` de notre pont (`crates/pa-daemon/src/acp/meta.rs`) : tour du prompt en cours, ordre.
let promptTurnId = 0;
let eventSequence = 0;
// PR 3 : trames comparées champ par champ à celles du binaire de `main` 1b96bb645 (capture du
// 21/11/2026, ticket 24, « PR 3, trames de l'agent factice ») : `_yus/*` du corps gardé à côté de
// l'espace du pont ; `prime` ajoute des champs dans cet espace (`ipython.diffCount`, `phase`…).
const update = ({ prime, ...body }) =>
  send({
    method: "session/update",
    params: {
      sessionId: SESSION_ID,
      update: {
        ...body,
        _meta: {
          ...body._meta,
          "ai.primeintellect.prime-agent": { promptTurnId, eventSequence: ++eventSequence, phase: "event", ...prime },
        },
      },
    },
  });
// Comme le pont : un `messageId` par message de l'agent (`prime-agent-assistant-<n>`).
const chunk = (text, kind = "agent_message_chunk") =>
  update({ sessionUpdate: kind, messageId: `prime-agent-assistant-${promptTurnId}`, content: { type: "text", text } });
// Fin d'un tour de prompt réussi, comme le pont (`crates/pa-daemon/src/acp/daemon.rs:1592-1620`) :
// `responseBoundary`, l'enveloppe de quiescence, `terminalQuiescence`, puis la réponse.
const finish = (id) => {
  update({ sessionUpdate: "session_info_update", prime: { phase: "responseBoundary", outcome: "result", terminalQuiescenceExpected: true } });
  update({ sessionUpdate: "session_info_update", prime: { quiescence: { outstandingSubagents: 0, remainingAutonomousContinuations: 0 } } });
  update({
    sessionUpdate: "session_info_update",
    prime: { phase: "terminalQuiescence", outcome: "result", quiescence: { outstandingSubagents: 0, remainingAutonomousContinuations: 0 } },
  });
  respond(id, { stopReason: "end_turn" });
};
const later = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
// Ticket 25 : le pont publie `turnStart` avant le contenu de chaque tour (`ya_turns.rs`).
const turnStart = (start) => update({ sessionUpdate: "session_info_update", prime: { turnStart: start } });
// Ticket 25 : un tour lancé par l'agent (avis `[bash-done]`) : son propre identifiant, `turnStart`,
// son contenu ; `settle` le ferme comme le pont (frontière puis enveloppe, aucune réponse).
const wake = () => {
  promptTurnId += 1;
  turnStart({ origin: "wake", wake: "bash" });
};
const settle = () => {
  update({ sessionUpdate: "session_info_update", prime: { phase: "responseBoundary", outcome: "result", terminalQuiescenceExpected: true } });
  update({ sessionUpdate: "session_info_update", prime: { quiescence: { outstandingSubagents: 0, remainingAutonomousContinuations: 0 } } });
  update({
    sessionUpdate: "session_info_update",
    prime: { phase: "terminalQuiescence", outcome: "result", quiescence: { outstandingSubagents: 0, remainingAutonomousContinuations: 0 } },
  });
};

/** Requêtes de l'agent vers le client en attente de réponse : id → résolution. */
const waiting = new Map();
let cancelled = null;
let nextRequest = 0;

function askPermission(toolCallId) {
  // Préfixe des identifiants du pont (`ID_PREFIX`, `crates/pa-daemon/src/acp/ya_permissions.rs`).
  const id = `ya-permission-${++nextRequest}`;
  send({
    id,
    method: "session/request_permission",
    params: {
      sessionId: SESSION_ID,
      toolCall: { toolCallId, status: "pending" },
      options: [
        { optionId: "allow-once", name: "Allow", kind: "allow_once" },
        { optionId: "reject-once", name: "Reject", kind: "reject_once" },
      ],
    },
  });
  return { id, answer: new Promise((resolve) => waiting.set(id, resolve)) };
}

/** Une cellule `edit()` : `a.txt` laissé à `after`, `neuf.txt` créé, diffs à la forme du pont. */
function editCell(id, after) {
  const cwd = process.cwd();
  writeFileSync(join(cwd, "a.txt"), after);
  writeFileSync(join(cwd, "neuf.txt"), "nouveau\n");
  const code = "await edit('a.txt', 'deux', 'DEUX')\nawait edit('neuf.txt', '', 'nouveau\\n')";
  update({ sessionUpdate: "tool_call", toolCallId: "call-d", title: "Python cell", kind: "execute", status: "in_progress", rawInput: { code } });
  update({
    sessionUpdate: "tool_call_update",
    toolCallId: "call-d",
    status: "completed",
    content: [
      { type: "content", content: { type: "text", text: `Edited ${cwd}/a.txt` } },
      { type: "diff", path: `${cwd}/a.txt`, oldText: "deux", newText: "DEUX", _meta: { "_yus/startLine": 2, "_yus/startColumn": 0 } },
      { type: "diff", path: `${cwd}/neuf.txt`, newText: "nouveau\n", _meta: { "_yus/startLine": 1, "_yus/startColumn": 0 } },
    ],
    prime: { ipython: { diffCount: 2 } },
  });
  return finish(id);
}

async function onPrompt(id, text) {
  switch (text) {
    case "bonjour":
      chunk("Bon");
      await later(20);
      chunk("jour");
      return finish(id);
    case "pense":
      chunk("je réfléchis", "agent_thought_chunk");
      chunk("ok");
      return finish(id);
    case "outil":
      update({ sessionUpdate: "tool_call", toolCallId: "call-1", title: "Python cell", kind: "execute", status: "in_progress", rawInput: { code: "print(1)" } });
      update({ sessionUpdate: "tool_call_update", toolCallId: "call-1", status: "completed" });
      chunk("fait");
      return finish(id);
    case "lent": {
      chunk("Je commence");
      update({ sessionUpdate: "tool_call", toolCallId: "call-2", title: "Python cell", kind: "execute", status: "pending", rawInput: { code: "1" } });
      const request = askPermission("call-2");
      await new Promise((resolve) => (cancelled = resolve));
      await Promise.race([request.answer, later(2000)]);
      return respond(id, { stopReason: "cancelled" });
    }
    case "retire": {
      update({ sessionUpdate: "tool_call", toolCallId: "call-3", title: "Python cell", kind: "execute", status: "pending", rawInput: { code: "2" } });
      const request = askPermission("call-3");
      await later(100);
      send({ method: "$/cancel_request", params: { requestId: request.id } });
      await request.answer;
      update({ sessionUpdate: "tool_call_update", toolCallId: "call-3", status: "in_progress" });
      // La cellule tourne un moment : l'état `running` de la carte se voit (PR 3, défaut 3).
      await later(150);
      update({ sessionUpdate: "tool_call_update", toolCallId: "call-3", status: "completed" });
      chunk("retirée");
      return finish(id);
    }
    // PR 3 : une cellule finie avec les données de carte du pont (`ya_cell_display.rs`) : deux
    // `bash()` lus (le second sorti en 1), un `edit()` d'un fichier existant, un fichier créé.
    case "carte": {
      const code = "r = await bash('echo lu')\nawait bash('false')\nawait edit('src/a.py', 'x = 1\\n', 'x = 2\\n')\nawait edit('neuf.txt', '', 'un\\ndeux\\n')";
      update({ sessionUpdate: "tool_call", toolCallId: "call-c", title: "Python cell", kind: "execute", status: "in_progress", rawInput: { code } });
      update({
        sessionUpdate: "tool_call_update",
        toolCallId: "call-c",
        status: "completed",
        content: [
          { type: "content", content: { type: "text", text: "Edited a.py" } },
          { type: "diff", path: `${process.cwd()}/src/a.py`, oldText: "x = 1\n", newText: "x = 2\n", _meta: { "_yus/startLine": 3, "_yus/startColumn": 0 } },
          { type: "diff", path: `${process.cwd()}/neuf.txt`, newText: "un\ndeux\n", _meta: { "_yus/startLine": 1, "_yus/startColumn": 0 } },
        ],
        _meta: {
          "_yus/bashRuns": [
            { command: "echo lu", exitCode: 0, output: "lu\n", durationMs: 6 },
            { command: "false", exitCode: 1, output: "", durationMs: 3 },
          ],
        },
        prime: { ipython: { diffCount: 2 } },
      });
      return finish(id);
    }
    // PR 3 : une cellule refusée, comme le pont la termine (`failed`, texte fixe), puis fin du tour (D7).
    case "refus":
      update({ sessionUpdate: "tool_call", toolCallId: "call-r", title: "Python cell", kind: "execute", status: "pending", rawInput: { code: "await bash('rm -rf build')" } });
      update({
        sessionUpdate: "tool_call_update",
        toolCallId: "call-r",
        status: "failed",
        content: [{ type: "content", content: { type: "text", text: "[permission-denied] The user declined this cell; nothing was executed." } }],
      });
      return finish(id);
    // PR 3 : une demande d'accord à laquelle la boîte répond ; la suite comme le pont
    // (`crates/pa-cli/tests/ya_permissions_acp_e2e.rs:400-434`, trames capturées le 21/11/2026) :
    // accord → `in_progress` puis `completed` ; refus ou `cancelled` → `failed` avec le texte fixe,
    // et le tour s'arrête (D7).
    case "accord": {
      update({ sessionUpdate: "tool_call", toolCallId: "call-a", title: "Python cell", kind: "execute", status: "pending", rawInput: { code: "print('accord')" } });
      const request = askPermission("call-a");
      const result = await request.answer;
      if (result?.outcome?.outcome === "selected" && result.outcome.optionId === "allow-once") {
        update({ sessionUpdate: "tool_call_update", toolCallId: "call-a", status: "in_progress" });
        update({ sessionUpdate: "tool_call_update", toolCallId: "call-a", status: "completed", content: [{ type: "content", content: { type: "text", text: "accord\n" } }] });
        chunk("autorisée");
      } else {
        update({
          sessionUpdate: "tool_call_update",
          toolCallId: "call-a",
          status: "failed",
          content: [{ type: "content", content: { type: "text", text: "[permission-denied] The user declined this cell; nothing was executed." } }],
        });
      }
      return finish(id);
    }
    // PR 3, étape 4 : une cellule qui modifie `a.txt` (`deux` → `DEUX`, ligne 2) et crée `neuf.txt`,
    // fichiers écrits dans le dossier de travail comme le ferait le kernel, puis les diffs du pont.
    case "diff":
      return editCell(id, "un\nDEUX\ntrois\n");
    // PR 3, étape 4 (demande de Yusuf) : `edit()` puis `bash("sed -i 's/DEUX/Deux/' a.txt")` sur le
    // même fichier : le texte de l'edit n'est plus à sa ligne, « Rejeter » est impossible.
    case "diffsed":
      return editCell(id, "un\nDeux\ntrois\n");
    case "auth":
      return fail(id, -32603, "Internal error", {
        details: "No API key found for capture",
        kind: "authentication",
        category: "authentication",
        retryable: false,
      });
    case "sanscle":
      // Forme relevée sur le vrai binaire sans identifiant (main 1b96bb645) : kind « other ».
      return fail(id, -32603, "Internal error", {
        details: "yusagent turn failed: No API key found for amazon-bedrock.\n\nUse /login to log into a provider via OAuth or API key. See:\n  /x/docs/providers.md\n  /x/docs/models.md",
        kind: "other",
        category: "other",
        retryable: false,
      });
    case "crash":
      process.exit(3);
      return;
    // Ticket 25 : le prompt finit, puis l'agent se réveille seul (avis `[bash-done]`).
    case "réveil":
      chunk("lancé");
      finish(id);
      await later(200);
      wake();
      chunk("réveil fini");
      return settle();
    // Ticket 25 (T4) : Stop pendant un réveil n'arrête que ce tour (`abort_and_send_queued`) : pas
    // de frontière pour lui, puis l'avis resté en file réveille l'agent de nouveau.
    case "réveil-lent":
      chunk("lancé");
      finish(id);
      await later(200);
      wake();
      chunk("réveil en cours");
      await new Promise((resolve) => (cancelled = resolve));
      await later(50);
      wake();
      chunk("second réveil");
      return settle();
    case "auto": {
      const option = config.find((candidate) => candidate.id === "permissions");
      if (option) option.currentValue = "auto";
      update({ sessionUpdate: "config_option_update", configOptions: config });
      return finish(id);
    }
    default:
      chunk(`écho : ${text}`);
      return finish(id);
  }
}

// Ticket 31 (X3) : les écrans skills, MCP et réglages, aux formes du pont
// (`crates/pa-daemon/src/acp/ya_resources.rs`, `ya_resources/settings.rs`), relevées par
// `bench/tools/capture/acp_frames.py` (cas C-resources) et comparées par `compare_frames.py`.
// L'état vit dans ce processus ; les fichiers des skills sous `<FAKE_ACP_DIR>/agent/skills`.
const SETTING_KEYS = [
  "compaction.enabled",
  "auxiliaryModel",
  "retry.enabled",
  "retry.maxRetries",
  "retry.baseDelayMs",
  "retry.provider.maxRetryDelayMs",
  "retry.failover.enabled",
  "retry.failover.maxRetries",
  "retry.failover.baseDelayMs",
  "retry.failover.maxDelayMs",
  "retry.provider.waitForUsage.pauseUntilReset",
  "retry.provider.waitForUsage.maxPauseMs",
  "retry.provider.waitForUsage.maxParks",
  "mcpServers",
];
const FROZEN = ["reasoningSummary", "bashWaitSeconds", "promptVersion", "codexSessionIdHeader", "existingTestsRule", "editTool", "shellTool"];
const PERMISSIONS_REFUSAL =
  "permissions is not set here: change it for this conversation with the permissions menu, or in ~/.yusagent/settings.json for every new session.";
/**
 * Défauts que donne le pont (`settings.rs`, `defaults` : getters du cœur sur des réglages vides,
 * ticket 31 C1), relevés par le test `the_view_gives_the_core_defaults` (run `ci` 45392710567).
 */
const SETTING_DEFAULTS = {
  "compaction.enabled": true,
  auxiliaryModel: null,
  "retry.enabled": true,
  "retry.maxRetries": 3,
  "retry.baseDelayMs": 2000,
  "retry.provider.maxRetryDelayMs": 60000,
  "retry.failover.enabled": true,
  "retry.failover.maxRetries": 3,
  "retry.failover.baseDelayMs": 1000,
  "retry.failover.maxDelayMs": 30000,
  "retry.provider.waitForUsage.pauseUntilReset": true,
  "retry.provider.waitForUsage.maxPauseMs": 86400000,
  "retry.provider.waitForUsage.maxParks": 8,
  mcpServers: null,
};
const settingValues = Object.fromEntries(SETTING_KEYS.map((key) => [key, null]));
const home = dir ?? process.cwd();
const skillFile = (root, name) => join(home, root, name, "SKILL.md");
const skills = [
  { name: "edit", description: "Edit files.", root: "builtin", source: "builtin", enabled: true },
  { name: "mcp", description: "Call MCP servers.", root: "builtin", source: "builtin", enabled: true },
];
/** Ce que le modèle sait (le prompt de l'ouverture et les annonces), fixé au premier prompt comme le moteur. */
let known = null;
const enabledSkills = () => skills.filter((skill) => skill.enabled).map((skill) => skill.name).sort();
const enabledServers = () =>
  Object.entries(settingValues.mcpServers ?? {})
    .filter(([, server]) => server?.enabled !== false)
    .map(([name]) => name)
    .sort();
const skillsView = () => ({
  skills: skills.map(({ name, description, root, source, enabled }) => ({
    name,
    description,
    path: skillFile(root, name),
    location: join(home, root, name),
    source,
    enabled,
    active: known ? known.skills.includes(name) : enabled,
  })),
  folders: [],
  // Comme le binaire de `main` : son skill fourni `factory` sans description (capture C-resources).
  problems: [{ message: "description is required", path: join(home, "builtin", "factory", "SKILL.md") }],
  restart: false,
});
const settingsView = () => ({ values: { ...settingValues }, defaults: { ...SETTING_DEFAULTS }, restart: [] });
const mcpView = () => ({
  servers: [
    ...Object.entries(settingValues.mcpServers ?? {}).map(([id, server]) => ({
      id,
      label: id,
      source: "user",
      status: server?.enabled === false ? "disabled" : "not_connected",
      signIn: false,
      signedIn: false,
    })),
    { id: "linear", label: "Linear", source: "catalog", status: "not_connected", signIn: true, signedIn: false },
  ],
});
const RESOURCE_METHODS = new Set([
  "_yus/settings/get",
  "_yus/settings/set",
  "_yus/skills/list",
  "_yus/skills/set",
  "_yus/skills/create",
  "_yus/skills/add_folder",
  "_yus/skills/remove_folder",
  "_yus/mcp/list",
  "_yus/mcp/set_token",
  "_yus/mcp/logout",
]);
const invalid = (id, details) => fail(id, -32602, "Invalid params", { details });

function setSetting(id, { key, value }) {
  if (key === "permissions") return invalid(id, PERMISSIONS_REFUSAL);
  if (FROZEN.includes(key)) return invalid(id, `${key} is fixed for each conversation when it starts: it is not set here.`);
  if (!SETTING_KEYS.includes(key)) return invalid(id, `unknown setting: ${key}`);
  settingValues[key] = value ?? null;
  return respond(id, settingsView());
}

function resources(id, method, params = {}) {
  switch (method) {
    case "_yus/settings/get":
      return respond(id, settingsView());
    case "_yus/settings/set":
      return setSetting(id, params);
    case "_yus/skills/list":
      return respond(id, skillsView());
    case "_yus/skills/set": {
      const skill = skills.find((candidate) => skillFile(candidate.root, candidate.name) === params.path);
      if (!skill) return invalid(id, `no skill at ${params.path}`);
      skill.enabled = params.enabled === true;
      return respond(id, skillsView());
    }
    case "_yus/skills/create": {
      if (skills.some((skill) => skill.name === params.name)) return invalid(id, `a skill named ${params.name} exists`);
      const file = skillFile("agent/skills", params.name);
      mkdirSync(join(home, "agent/skills", params.name), { recursive: true });
      writeFileSync(file, `---\nname: ${params.name}\ndescription: ${params.description}\n---\n`);
      skills.unshift({ name: params.name, description: params.description, root: "agent/skills", source: "user", enabled: true });
      return respond(id, { ...skillsView(), created: file });
    }
    case "_yus/skills/add_folder":
    case "_yus/skills/remove_folder":
      return respond(id, skillsView());
    case "_yus/mcp/list":
    case "_yus/mcp/set_token":
    case "_yus/mcp/logout":
      return respond(id, mcpView());
  }
}

/** Au début d'un prompt, comme le moteur (`ya_resource_notice`) : la différence avec ce que le modèle sait. */
function announce() {
  const now = { skills: enabledSkills(), servers: enabledServers() };
  if (!known) {
    known = now;
    return;
  }
  const minus = (a, b) => a.filter((name) => !b.includes(name));
  const details = {
    skills: { added: minus(now.skills, known.skills), changed: [], removed: minus(known.skills, now.skills) },
    mcp: { added: minus(now.servers, known.servers), removed: minus(known.servers, now.servers) },
  };
  known = now;
  const any = [details.skills.added, details.skills.removed, details.mcp.added, details.mcp.removed].some((list) => list.length > 0);
  if (any) update({ sessionUpdate: "session_info_update", _meta: { "_yus/notice": { kind: "resources", details } } });
}

function onMessage(message) {
  const { id, method, params } = message;
  if (method === undefined) {
    const resolve = waiting.get(id);
    if (resolve) {
      waiting.delete(id);
      resolve(message.result ?? message.error);
    }
    return;
  }
  switch (method) {
    case "initialize":
      return respond(id, {
        protocolVersion: scenario.protocolVersion ?? 1,
        // Ticket 28 : capacités du pont de `main` 9937e4ff8 (reprise, liste), comparées par
        // `bench/tools/capture/compare_frames.py`.
        agentCapabilities: {
          loadSession: true,
          promptCapabilities: { image: true, embeddedContext: true },
          mcpCapabilities: { http: true },
          sessionCapabilities: { close: {}, list: {} },
        },
        agentInfo: { name: "fake-agent", version: "0" },
      });
    case "session/new":
      // Ticket 28 : `saved` dans notre `_meta`, comme le pont. Le fichier n'est jamais écrit : une
      // relance repart sur une conversation neuve (pas de `session/load` ici).
      return respond(id, {
        sessionId: SESSION_ID,
        configOptions: config,
        _meta: { "ai.primeintellect.prime-agent": { saved: { sessionId: FILE_ID, path: join(dir ?? process.cwd(), `${FILE_ID}.jsonl`) } } },
      });
    case "session/load":
      return fail(id, -32602, "Invalid params", { details: "The fake agent loads no conversation." });
    case "session/list":
      return respond(id, { sessions: [] });
    case "_unharnessed/sessions/delete":
      return respond(id, {});
    case "session/prompt": {
      promptTurnId += 1;
      eventSequence = 0;
      turnStart({ origin: "prompt" });
      announce();
      const text = (params?.prompt ?? []).map((block) => block.text ?? "").join("");
      void onPrompt(id, text);
      return;
    }
    case "session/cancel":
      if (cancelled) {
        const resolve = cancelled;
        cancelled = null;
        resolve();
      }
      return;
    case "session/set_config_option": {
      const option = config.find((candidate) => candidate.id === params?.configId);
      if (!option || !option.options.some((choice) => choice.value === params?.value)) {
        return fail(id, -32602, "Invalid params");
      }
      option.currentValue = params.value;
      return respond(id, { configOptions: config });
    }
    case "session/close":
      return respond(id, {});
    default:
      if (RESOURCE_METHODS.has(method) && id !== undefined) return resources(id, method, params);
      if (id !== undefined) fail(id, -32601, "Method not found");
  }
}

const input = createInterface({ input: process.stdin });
input.on("line", (line) => {
  if (dir) appendFileSync(join(dir, `received-${process.pid}.jsonl`), line + "\n");
  let message;
  try {
    message = JSON.parse(line);
  } catch {
    return;
  }
  onMessage(message);
});
input.on("close", () => process.exit(0));
process.on("SIGTERM", () => process.exit(0));

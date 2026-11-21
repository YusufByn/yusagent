#!/usr/bin/env node
// Agent ACP factice pour les tests de l'extension (ticket 24, PR 2) : rejoue des trames de la forme
// du pont (`crates/pa-daemon/src/acp/`) et enregistre chaque trame reçue. Aucun modèle.
//
// Pilotage : variable FAKE_ACP_DIR (dossier existant).
// - `<dir>/scenario.json` (facultatif), lu à chaque lancement : { "permissions": "manual" | "auto" | null,
//   "protocolVersion": 1 } ;
// - `<dir>/received-<pid>.jsonl` : chaque ligne reçue, telle quelle ;
// - `<dir>/starts.log` : une ligne par lancement (pid, cwd, arguments).
// Le comportement d'un prompt dépend de son texte (voir `onPrompt`).

import { appendFileSync, existsSync, readFileSync } from "node:fs";
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
    JSON.stringify({ pid: process.pid, cwd: process.cwd(), argv: process.argv.slice(2) }) + "\n",
  );
}

const SESSION_ID = "fake-session-1";
const config = [
  {
    id: "model",
    name: "Model",
    type: "select",
    category: "model",
    currentValue: "capture/model-a",
    options: [
      { value: "capture/model-a", name: "Model A (capture)" },
      { value: "capture/model-b", name: "Model B (capture)" },
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
const permissions = scenario.permissions === undefined ? "manual" : scenario.permissions;
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
const update = (body) =>
  send({
    method: "session/update",
    params: {
      sessionId: SESSION_ID,
      update: {
        ...body,
        // `_yus/*` du corps (PR 3 : `_yus/bashRuns`) gardé à côté de l'espace du pont.
        _meta: { ...body._meta, "ai.primeintellect.prime-agent": { promptTurnId, eventSequence: ++eventSequence, phase: "event" } },
      },
    },
  });
const chunk = (text, kind = "agent_message_chunk") => update({ sessionUpdate: kind, content: { type: "text", text } });
const later = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** Requêtes de l'agent vers le client en attente de réponse : id → résolution. */
const waiting = new Map();
let cancelled = null;
let nextRequest = 0;

function askPermission(toolCallId) {
  const id = `ya-perm-${++nextRequest}`;
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

async function onPrompt(id, text) {
  switch (text) {
    case "bonjour":
      chunk("Bon");
      await later(20);
      chunk("jour");
      return respond(id, { stopReason: "end_turn" });
    case "pense":
      chunk("je réfléchis", "agent_thought_chunk");
      chunk("ok");
      return respond(id, { stopReason: "end_turn" });
    case "outil":
      update({ sessionUpdate: "tool_call", toolCallId: "call-1", title: "Python cell", kind: "execute", status: "in_progress", rawInput: { code: "print(1)" } });
      update({ sessionUpdate: "tool_call_update", toolCallId: "call-1", status: "completed" });
      chunk("fait");
      return respond(id, { stopReason: "end_turn" });
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
      return respond(id, { stopReason: "end_turn" });
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
          { type: "diff", path: `${process.cwd()}/src/a.py`, oldText: "x = 1\n", newText: "x = 2\n", _meta: { "_yus/startLine": 3 } },
          { type: "diff", path: `${process.cwd()}/neuf.txt`, newText: "un\ndeux\n", _meta: { "_yus/startLine": 1 } },
        ],
        _meta: {
          "_yus/bashRuns": [
            { command: "echo lu", exitCode: 0, output: "lu\n", durationMs: 6 },
            { command: "false", exitCode: 1, output: "", durationMs: 3 },
          ],
        },
      });
      return respond(id, { stopReason: "end_turn" });
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
      return respond(id, { stopReason: "end_turn" });
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
    case "auto": {
      const option = config.find((candidate) => candidate.id === "permissions");
      if (option) option.currentValue = "auto";
      update({ sessionUpdate: "config_option_update", configOptions: config });
      return respond(id, { stopReason: "end_turn" });
    }
    default:
      chunk(`écho : ${text}`);
      return respond(id, { stopReason: "end_turn" });
  }
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
        agentCapabilities: { loadSession: false },
        agentInfo: { name: "fake-agent", version: "0" },
      });
    case "session/new":
      return respond(id, { sessionId: SESSION_ID, configOptions: config });
    case "session/prompt": {
      promptTurnId += 1;
      eventSequence = 0;
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

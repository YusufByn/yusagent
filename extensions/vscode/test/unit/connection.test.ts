import assert from "node:assert/strict";
import { test } from "node:test";

import {
  Connection,
  ConnectionClosed,
  METHOD_NOT_FOUND,
  RpcError,
  type Handlers,
} from "../../src/acp/connection";

function harness(overrides: Partial<Handlers> = {}) {
  const sent: Record<string, unknown>[] = [];
  const notifications: [string, unknown][] = [];
  const errors: string[] = [];
  const connection = new Connection((line) => {
    assert.ok(line.endsWith("\n"), "une trame par ligne");
    sent.push(JSON.parse(line) as Record<string, unknown>);
  }, {
    onRequest: overrides.onRequest ?? (() => {
      throw new RpcError(METHOD_NOT_FOUND, "Method not found");
    }),
    onNotification: overrides.onNotification ?? ((method, params) => notifications.push([method, params])),
    onProtocolError: (message) => errors.push(message),
  });
  return { connection, sent, notifications, errors };
}

const tick = () => new Promise((resolve) => setImmediate(resolve));

test("requête : id croissants, réponse corrélée même en morceaux", async () => {
  const { connection, sent } = harness();
  const first = connection.request("initialize", { protocolVersion: 1 });
  const second = connection.request("session/new", { cwd: "/w", mcpServers: [] });
  assert.deepEqual(sent.map((frame) => frame.id), [1, 2]);
  assert.deepEqual(sent[0], { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: 1 } });
  const reply = JSON.stringify({ jsonrpc: "2.0", id: 2, result: { sessionId: "s" } }) + "\n";
  connection.feed(reply.slice(0, 10));
  connection.feed(reply.slice(10) + JSON.stringify({ jsonrpc: "2.0", id: 1, result: { protocolVersion: 1 } }));
  assert.deepEqual(await second, { sessionId: "s" });
  connection.feed("\n");
  assert.deepEqual(await first, { protocolVersion: 1 });
});

test("réponse d'erreur : RpcError avec code et data", async () => {
  const { connection } = harness();
  const pending = connection.request("session/prompt", {});
  connection.feed(JSON.stringify({ jsonrpc: "2.0", id: 1, error: { code: -32603, message: "Internal error", data: { kind: "authentication" } } }) + "\n");
  await assert.rejects(pending, (error: unknown) => {
    assert.ok(error instanceof RpcError);
    assert.equal(error.code, -32603);
    assert.deepEqual(error.data, { kind: "authentication" });
    return true;
  });
});

test("notification reçue transmise ; notification envoyée sans id", () => {
  const { connection, sent, notifications } = harness();
  connection.feed(JSON.stringify({ jsonrpc: "2.0", method: "session/update", params: { a: 1 } }) + "\n");
  assert.deepEqual(notifications, [["session/update", { a: 1 }]]);
  connection.notify("session/cancel", { sessionId: "s" });
  assert.deepEqual(sent, [{ jsonrpc: "2.0", method: "session/cancel", params: { sessionId: "s" } }]);
});

test("méthode inconnue reçue : erreur -32601", async () => {
  const { connection, sent } = harness();
  connection.feed(JSON.stringify({ jsonrpc: "2.0", id: "x", method: "fs/read_text_file", params: {} }) + "\n");
  await tick();
  assert.deepEqual(sent, [{ jsonrpc: "2.0", id: "x", error: { code: -32601, message: "Method not found" } }]);
});

test("requête reçue : réponse différée, une seule réponse", async () => {
  let answer: (value: unknown) => void = () => undefined;
  const { connection, sent } = harness({
    onRequest: () => new Promise((resolve) => (answer = resolve)),
  });
  connection.feed(JSON.stringify({ jsonrpc: "2.0", id: "ya-perm-1", method: "session/request_permission", params: {} }) + "\n");
  await tick();
  assert.equal(sent.length, 0);
  answer({ outcome: { outcome: "cancelled" } });
  await tick();
  assert.deepEqual(sent, [{ jsonrpc: "2.0", id: "ya-perm-1", result: { outcome: { outcome: "cancelled" } } }]);
});

test("$/cancel_request : rappel de la requête visée, puis sa réponse", async () => {
  const { connection, sent, notifications } = harness({
    onRequest: (_method, _params, context) =>
      new Promise((resolve) => context.onCancel(() => resolve({ outcome: { outcome: "cancelled" } }))),
  });
  connection.feed(JSON.stringify({ jsonrpc: "2.0", id: 7, method: "session/request_permission", params: {} }) + "\n");
  connection.feed(JSON.stringify({ jsonrpc: "2.0", method: "$/cancel_request", params: { requestId: "7" } }) + "\n");
  await tick();
  assert.equal(sent.length, 0, "id chaîne \"7\" différent de l'id nombre 7");
  connection.feed(JSON.stringify({ jsonrpc: "2.0", method: "$/cancel_request", params: { requestId: 7 } }) + "\n");
  await tick();
  assert.deepEqual(sent, [{ jsonrpc: "2.0", id: 7, result: { outcome: { outcome: "cancelled" } } }]);
  assert.deepEqual(notifications, [], "$/cancel_request n'est pas transmis comme notification");
});

test("fermeture : requêtes en attente rejetées, plus rien n'est écrit", async () => {
  const { connection, sent } = harness();
  const pending = connection.request("session/prompt", {});
  connection.close("exited");
  await assert.rejects(pending, ConnectionClosed);
  await assert.rejects(connection.request("session/new", {}), ConnectionClosed);
  connection.notify("session/cancel", {});
  assert.equal(sent.length, 1);
});

test("lignes illisibles signalées, jamais fatales", async () => {
  const { connection, errors } = harness();
  const pending = connection.request("initialize", {});
  connection.feed("pas du json\n[1,2]\n{\"jsonrpc\":\"2.0\"}\n{\"jsonrpc\":\"2.0\",\"id\":99,\"result\":1}\n");
  connection.feed(JSON.stringify({ jsonrpc: "2.0", id: 1, result: "ok" }) + "\n");
  assert.equal(await pending, "ok");
  assert.equal(errors.length, 4);
});

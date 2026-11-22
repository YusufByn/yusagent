// yusAgent (ticket 28, C2) : `saved`, `path` et `current` lus dans le `_meta` du pont
// (`ai.primeintellect.prime-agent`, `crates/pa-daemon/src/acp/ya_history.rs`), ou dans
// `_meta.unharnessed` s'il est là ; méthodes servies.

import assert from "node:assert/strict";
import { test } from "node:test";

import { yaResultMeta } from "../../src/ya/meta";
import { features, served } from "../../src/ya/methods";

test("yaResultMeta : saved de session/new dans notre _meta", () => {
  const result = {
    sessionId: "acp-1",
    _meta: { "ai.primeintellect.prime-agent": { saved: { sessionId: "file-1", path: "/s/file-1.jsonl" } } },
  };
  assert.deepEqual(yaResultMeta(result).saved, { sessionId: "file-1", path: "/s/file-1.jsonl" });
});

test("yaResultMeta : path et current d'une ligne de session/list", () => {
  const row = { sessionId: "file-1", _meta: { "ai.primeintellect.prime-agent": { path: "/s/file-1.jsonl", current: true } } };
  assert.deepEqual(yaResultMeta(row), { path: "/s/file-1.jsonl", current: true });
  assert.deepEqual(yaResultMeta({ sessionId: "file-2", _meta: { "ai.primeintellect.prime-agent": { path: "/p" } } }).current, undefined);
});

test("yaResultMeta : _meta.unharnessed lu tel quel, rien sans _meta", () => {
  assert.deepEqual(yaResultMeta({ _meta: { unharnessed: { path: "/u" } } }), { path: "/u" });
  assert.deepEqual(yaResultMeta({}), {});
  assert.deepEqual(yaResultMeta(undefined), {});
});

test("méthodes du ticket 28 servies, historique affiché", () => {
  for (const method of ["session/load", "session/list", "_unharnessed/sessions/delete"]) assert.equal(served(method), true, method);
  assert.equal(features().history, true);
});

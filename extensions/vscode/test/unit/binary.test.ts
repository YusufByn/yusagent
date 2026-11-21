import assert from "node:assert/strict";
import { chmodSync, mkdtempSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { describeFailure, resolveBinary } from "../../src/binary";

function tree() {
  const dir = mkdtempSync(join(tmpdir(), "yusagent-bin-"));
  const exe = join(dir, "yusagent");
  writeFileSync(exe, "#!/bin/sh\n");
  chmodSync(exe, 0o755);
  const plain = join(dir, "plain");
  writeFileSync(plain, "");
  chmodSync(plain, 0o644);
  return { dir, exe, plain };
}

test("chemin absolu exécutable", () => {
  const { exe } = tree();
  assert.deepEqual(resolveBinary(exe, {}, "/"), { ok: true, path: exe });
});

test("nom nu cherché dans le PATH (défaut : yusagent)", () => {
  const { dir, exe } = tree();
  assert.deepEqual(resolveBinary(undefined, { PATH: `/nulle-part:${dir}` }, "/"), { ok: true, path: exe });
  assert.deepEqual(resolveBinary("  ", { PATH: dir }, "/"), { ok: true, path: exe });
});

test("absent : chemin essayé dans le message", () => {
  const missing = resolveBinary("/nulle-part/yusagent", {}, "/");
  assert.equal(missing.ok, false);
  assert.ok(!missing.ok && describeFailure(missing).includes("/nulle-part/yusagent"));
  const notInPath = resolveBinary("yusagent", { PATH: "/nulle-part" }, "/");
  assert.ok(!notInPath.ok && notInPath.reason === "not_found");
});

test("non exécutable", () => {
  const { plain } = tree();
  const result = resolveBinary(plain, {}, "/");
  assert.ok(!result.ok && result.reason === "not_executable");
  assert.ok(!result.ok && describeFailure(result).startsWith("Binaire yusAgent non exécutable"));
});

test("~/ développé, chemin relatif résolu depuis le workspace", () => {
  const home = resolveBinary("~/nulle-part-yusagent", {}, "/");
  assert.ok(!home.ok && home.tried === join(homedir(), "nulle-part-yusagent"));
  const { dir, exe } = tree();
  assert.deepEqual(resolveBinary("./yusagent", {}, dir), { ok: true, path: exe });
});

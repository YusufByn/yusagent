// yusAgent (PR 3, étape 4) : l'« avant » retrouvé en défaisant les fragments d'`edit()`
// (`src/ya/diffs.ts`), et « Rejeter » avec ses garde-fous (`src/ya/reverts.ts`, fichiers en mémoire).

import assert from "node:assert/strict";
import { test } from "node:test";

import type { FileEdit } from "../../src/protocol";
import type { Fragment } from "../../src/ya/cells";
import type { DiffFiles } from "../../src/ya/diff-files";
import { undo } from "../../src/ya/diffs";
import { Reverts } from "../../src/ya/reverts";

/** Ce que fait `edit()` (`skills/edit/src/edit/__init__.py`) : le fragment et le texte d'après. */
function edit(content: string, oldStr: string, newStr: string): { after: string; fragment: Fragment } {
  const at = content.indexOf(oldStr);
  assert.ok(at >= 0 && content.indexOf(oldStr, at + 1) < 0, "old_str une seule fois");
  const startLine = content.slice(0, at).split("\n").length;
  return { after: content.replace(oldStr, newStr), fragment: { path: "/w/a.py", oldText: oldStr, newText: newStr, startLine } };
}

test("undo : un fragment, plusieurs fragments du même fichier, défaits du dernier au premier", () => {
  const before = "a = 1\nb = 2\nc = 3\n";
  const one = edit(before, "b = 2", "b = 20");
  assert.deepEqual(undo(one.after, [one.fragment]), { ok: true, before });
  const two = edit(one.after, "c = 3\n", "c = 30\nd = 4\n");
  const three = edit(two.after, "a = 1", "x = 0\na = 1");
  assert.deepEqual(undo(three.after, [one.fragment, two.fragment, three.fragment]), { ok: true, before });
});

test("undo : fichier créé (null), créé puis modifié dans la même cellule, création qui ne correspond plus", () => {
  assert.deepEqual(undo("un\n", [{ path: "/w/n", newText: "un\n", startLine: 1 }]), { ok: true, before: null });
  // Capture du binaire de main (scénario `display`) : création de `un\n`, puis `un` → `deux` ligne 1.
  const fragments: Fragment[] = [
    { path: "/w/a.txt", newText: "un\n", startLine: 1 },
    { path: "/w/a.txt", oldText: "un", newText: "deux", startLine: 1 },
  ];
  assert.deepEqual(undo("deux\n", fragments), { ok: true, before: null });
  const changed = undo("autre\n", [{ path: "/w/n", newText: "un\n", startLine: 1 }]);
  assert.equal(changed.ok, false);
});

test("undo : edit() puis bash(\"sed -i …\") sur le même fichier (demande de Yusuf)", () => {
  const before = "a = 1\nb = 2\nc = 3\n";
  const { after, fragment } = edit(before, "b = 2", "b = 20");
  // `sed -i 's/b = 20/b = 21/'` : le texte de l'edit a changé → pas de « Rejeter », avec la raison.
  assert.deepEqual(undo(after.replace("b = 20", "b = 21"), [fragment]), {
    ok: false,
    reason: "the edit's new text is no longer at line 2",
  });
  // `sed -i '1i # en-tête'` : une ligne ajoutée avant, le texte n'est plus à la ligne annoncée.
  assert.deepEqual(undo(`# en-tête\n${after}`, [fragment]), { ok: false, reason: "the edit's new text is no longer at line 2" });
  // `sed -i 's/c = 3/c = 30/'` : une autre ligne ; le fragment se défait, le changement de sed reste.
  assert.deepEqual(undo(after.replace("c = 3", "c = 30"), [fragment]), { ok: true, before: "a = 1\nb = 2\nc = 30\n" });
  // `sed -i '$d'` : la fin du fichier retirée, au-delà de la ligne annoncée.
  const short = edit("x\ny\nz = 1\n", "z = 1", "z = 2");
  assert.deepEqual(undo("x\ny\n", [short.fragment]), { ok: false, reason: "the edit's new text is no longer at line 3" });
});

test("undo : position sur la ligne, suppression pure, ambiguïté (C14 a), ligne absente", () => {
  // `new_str` trois fois sur sa ligne (`ab ab ab`) : trois « avant » possibles, ambigu.
  const twice = edit("ab ab xy\n", "xy", "ab");
  assert.deepEqual(undo(twice.after, [twice.fragment]), { ok: false, reason: "the edit's place on line 1 is ambiguous" });
  // `new_str` présent ailleurs, mais une seule fois sur la ligne annoncée : un seul « avant ».
  const elsewhere = edit("ab\nxy cd\n", "xy", "ab");
  assert.deepEqual(undo(elsewhere.after, [elsewhere.fragment]), { ok: true, before: "ab\nxy cd\n" });
  // Suppression pure, même de lignes entières : la colonne n'est pas connue (`garder\nf` + `retirer\n`
  // + `in` est aussi un « avant » valide) ; ambigu (C14 a ; l'option b, colonne envoyée par le pont,
  // le lèverait).
  const removed = edit("garder\nretirer\nfin\n", "retirer\n", "");
  assert.deepEqual(undo(removed.after, [removed.fragment]), { ok: false, reason: "the edit's place on line 2 is ambiguous" });
  // Sauf si la ligne d'arrivée est vide : une seule place.
  const emptyLine = edit("a\nretirer\n", "retirer\n", "");
  assert.deepEqual(undo(emptyLine.after, [emptyLine.fragment]), { ok: true, before: "a\nretirer\n" });
  // Suppression pure en milieu de ligne, plusieurs places possibles : ambigu, pas de « Rejeter ».
  const middle = edit("f(a, b, c)\n", ", b", "");
  assert.deepEqual(undo(middle.after, [middle.fragment]), { ok: false, reason: "the edit's place on line 1 is ambiguous" });
  assert.deepEqual(undo("x\n", [{ path: "/w/a", oldText: "y", newText: "x", startLine: 5 }]), {
    ok: false,
    reason: "the file has no line 5 any more",
  });
  assert.deepEqual(undo("x\n", [{ path: "/w/a", oldText: "y", newText: "x" }]), { ok: false, reason: "the edit has no start line" });
});

/** Des fichiers en mémoire, et ce qui leur arrive. */
function memory(initial: Record<string, string>): DiffFiles & { disk: Map<string, string>; buffer: Map<string, string>; diffs: string[][] } {
  const disk = new Map(Object.entries(initial));
  const buffer = new Map<string, string>();
  const diffs: string[][] = [];
  return {
    disk,
    buffer,
    diffs,
    readDisk: async (path) => disk.get(path),
    readCurrent: async (path) => buffer.get(path) ?? disk.get(path),
    write: async (path, text) => void disk.set(path, text),
    remove: async (path) => void disk.delete(path),
    showDiff: async (path, before, after, title) => void diffs.push([path, before, after, title]),
  };
}

const fileEdit = (path: string, shown: string, created = false): FileEdit => ({ path, shown, created, lines: [], added: 0, removed: 0 });

test("Reverts : restaure, retire un fichier créé, texte fixe une seule fois au message suivant", async () => {
  const files = memory({ "/w/a.py": "a = 20\n", "/w/n.txt": "nouveau\n" });
  const reverts = new Reverts(() => files);
  const fragments: Fragment[] = [
    { path: "/w/a.py", oldText: "a = 2", newText: "a = 20", startLine: 1 },
    { path: "/w/n.txt", newText: "nouveau\n", startLine: 1 },
  ];
  await reverts.record("t1", "s1", [fileEdit("/w/a.py", "a.py"), fileEdit("/w/n.txt", "n.txt", true)], fragments);
  assert.deepEqual(reverts.status("t1", "/w/a.py"), { state: "ready" });
  assert.equal(await reverts.openDiff("t1", "/w/n.txt"), true);
  assert.deepEqual(files.diffs[0], ["/w/n.txt", "", "nouveau\n", "n.txt (before ↔ after the cell)"]);
  assert.deepEqual(await reverts.revert("t1", "/w/a.py", { busy: false, session: "s1" }), { state: "reverted" });
  assert.equal(files.disk.get("/w/a.py"), "a = 2\n");
  assert.deepEqual(await reverts.revert("t1", "/w/n.txt", { busy: false, session: "s1" }), { state: "reverted" });
  assert.equal(files.disk.has("/w/n.txt"), false, "fichier créé retiré");
  assert.deepEqual(reverts.takeNotices(), ["[user-reverted] a.py", "[user-reverted] n.txt"]);
  assert.deepEqual(reverts.takeNotices(), [], "une seule fois");
  assert.deepEqual(await reverts.revert("t1", "/w/a.py", { busy: false, session: "s1" }), { state: "reverted" }, "déjà fait : rien");
  assert.deepEqual(reverts.takeNotices(), []);
});

test("Reverts : garde-fous (tour en cours, fichier changé, tampon non enregistré, autre session), avant introuvable", async () => {
  const files = memory({ "/w/a.py": "a = 20\n", "/w/b.py": "b = 9\n" });
  const reverts = new Reverts(() => files);
  await reverts.record("t1", "s1", [fileEdit("/w/a.py", "a.py"), fileEdit("/w/b.py", "b.py")], [
    { path: "/w/a.py", oldText: "a = 2", newText: "a = 20", startLine: 1 },
    { path: "/w/b.py", oldText: "b = 1", newText: "b = 2", startLine: 1 },
  ]);
  assert.deepEqual(reverts.status("t1", "/w/b.py"), { state: "blocked", reason: "the edit's new text is no longer at line 1" });
  assert.deepEqual(await reverts.revert("t1", "/w/a.py", { busy: true, session: "s1" }), { state: "ready", refused: "Not while a turn runs." });
  files.buffer.set("/w/a.py", "a = 20\n# pas enregistré\n");
  assert.deepEqual(await reverts.revert("t1", "/w/a.py", { busy: false, session: "s1" }), {
    state: "ready",
    refused: "Not reverted: the file changed since this cell (a later edit, or yours).",
  });
  files.buffer.clear();
  files.disk.set("/w/a.py", "a = 21\n");
  assert.equal((await reverts.revert("t1", "/w/a.py", { busy: false, session: "s1" }))?.state, "ready", "changé sur disque : refusé");
  assert.equal(files.disk.get("/w/a.py"), "a = 21\n", "rien d'écrit");
  files.disk.set("/w/a.py", "a = 20\n");
  assert.equal(reverts.expire("s1"), false, "même session : rien");
  assert.equal(reverts.expire("s2"), true);
  assert.deepEqual(reverts.status("t1", "/w/a.py"), { state: "blocked", reason: "the cell ran in an earlier session" });
  assert.deepEqual(await reverts.revert("t1", "/w/a.py", { busy: false, session: "s2" }), {
    state: "blocked",
    reason: "the cell ran in an earlier session",
  });
  assert.equal(files.disk.get("/w/a.py"), "a = 20\n");
  assert.deepEqual(reverts.takeNotices(), []);
});

test("Reverts : deux cellules sur le même fichier, seule la dernière se rejette, puis la précédente", async () => {
  const files = memory({ "/w/a.py": "v3\n" });
  const reverts = new Reverts(() => files);
  // Cellule 1 : v1 → v2 ; son après est lu à sa fin (v2), avant la cellule 2.
  files.disk.set("/w/a.py", "v2\n");
  await reverts.record("c1", "s", [fileEdit("/w/a.py", "a.py")], [{ path: "/w/a.py", oldText: "v1", newText: "v2", startLine: 1 }]);
  files.disk.set("/w/a.py", "v3\n");
  await reverts.record("c2", "s", [fileEdit("/w/a.py", "a.py")], [{ path: "/w/a.py", oldText: "v2", newText: "v3", startLine: 1 }]);
  assert.equal((await reverts.revert("c1", "/w/a.py", { busy: false, session: "s" }))?.state, "ready", "c1 refusée : c2 a changé le fichier");
  assert.deepEqual(await reverts.revert("c2", "/w/a.py", { busy: false, session: "s" }), { state: "reverted" });
  assert.equal(files.disk.get("/w/a.py"), "v2\n");
  assert.deepEqual(await reverts.revert("c1", "/w/a.py", { busy: false, session: "s" }), { state: "reverted" });
  assert.equal(files.disk.get("/w/a.py"), "v1\n");
  assert.deepEqual(reverts.takeNotices(), ["[user-reverted] a.py"], "une ligne par fichier");
});

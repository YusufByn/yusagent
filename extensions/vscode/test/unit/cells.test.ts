// yusAgent (PR 3, étape 2) : cartes de cellule. `readCell` pour les fonctions du kernel
// (`edit()`, `read()`, C7), et lecture des données du pont (`src/ya/cells.ts` : `bash()` réels C6,
// vrais diffs C7, refus C5).

import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, realpathSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { fragmentLines, readCell } from "../../src/cell";
import { bashRuns, cellOfEdits, cellOfRuns, fileEdits, fragments, isDeclined } from "../../src/ya/cells";

test("readCell : edit() d'un fichier existant, diff deviné de ses textes", () => {
  const cell = readCell("await edit('src/a.py', 'x = 1\\n', 'x = 2\\n')");
  assert.equal(cell.kind, "edit");
  assert.deepEqual(cell.files, ["src/a.py"]);
  assert.deepEqual(cell.diff, { lines: [["-", "x = 1"], ["+", "x = 2"]], added: 1, removed: 1 });
});

test("readCell : edit() qui crée (old_str vide), arguments nommés, read()", () => {
  const created = readCell("await edit(path='neuf.txt', old_str='', new_str='un\\ndeux\\n')");
  assert.equal(created.kind, "write");
  assert.deepEqual(created.files, ["neuf.txt"]);
  assert.deepEqual(created.diff?.lines, [["+", "un"], ["+", "deux"]]);
  const mixed = readCell("await edit('a.py', '', 'z')\nawait edit('b.py', old, new)");
  assert.equal(mixed.kind, "edit", "un appel qui ne crée pas : edit");
  assert.deepEqual(mixed.files, ["a.py", "b.py"]);
  const read = readCell("print(await read('README.md'))");
  assert.equal(read.kind, "read");
  assert.deepEqual(read.files, ["README.md"]);
  // `f.read()` (méthode) n'est pas la fonction du kernel.
  assert.equal(readCell("f = open('x')\nf.read()").kind, "read");
  assert.deepEqual(readCell("data = f.read()").files, []);
});

test("fragmentLines : fragment remplacé, fichier créé, création vide", () => {
  assert.deepEqual(fragmentLines("a\nb\n", "a\nc\n"), [[" ", "a"], ["-", "b"], ["+", "c"]]);
  assert.deepEqual(fragmentLines("", "un\ndeux\n"), [["+", "un"], ["+", "deux"]]);
  assert.deepEqual(fragmentLines("", ""), []);
});

test("bashRuns : forme du pont, entrées invalides ignorées, absent → undefined", () => {
  const runs = bashRuns({
    "ai.primeintellect.prime-agent": {},
    "_yus/bashRuns": [{ command: "ls", exitCode: 0, output: "a\n", durationMs: 4 }, { exitCode: 1 }, { command: "x", exitCode: null }],
  });
  assert.deepEqual(runs, [
    { command: "ls", exitCode: 0, output: "a\n", durationMs: 4 },
    { command: "x", exitCode: null, output: "" },
  ]);
  assert.equal(bashRuns({}), undefined);
  assert.equal(bashRuns(undefined), undefined);
  assert.equal(bashRuns({ "_yus/bashRuns": [] }), undefined);
});

test("fragments et fileEdits : un bloc par fichier, chemin relatif au dossier, fichier créé", () => {
  const content = [
    { type: "content", content: { type: "text", text: "out" } },
    { type: "diff", path: "/w/src/a.py", oldText: "x\n", newText: "y\n", _meta: { "_yus/startLine": 3 } },
    { type: "diff", path: "/w/neuf.txt", newText: "z\n", _meta: { "_yus/startLine": 1, "_yus/startColumn": 0 } },
    { type: "diff", path: "/w/src/a.py", oldText: "p\n", newText: "q\n", _meta: { "_yus/startLine": 9, "_yus/startColumn": 4 } },
    { type: "diff", path: "/ailleurs/b.py", oldText: "1", newText: "2" },
  ];
  const all = fragments(content);
  assert.equal(all.length, 4);
  assert.deepEqual(all[1], { path: "/w/neuf.txt", newText: "z\n", startLine: 1, startColumn: 0 });
  // Ticket 26 : la colonne quand le pont l'envoie, rien sinon (binaire ou session plus ancienne).
  assert.equal(all[2].startColumn, 4);
  assert.equal("startColumn" in all[0], false);
  assert.equal("startColumn" in all[3], false);
  const odd = fragments([{ type: "diff", path: "/w/a", newText: "", _meta: { "_yus/startLine": 1, "_yus/startColumn": -1 } }]);
  assert.equal("startColumn" in odd[0], false, "colonne invalide ignorée");
  const edits = fileEdits(all, "/w");
  assert.deepEqual(
    edits.map(({ shown, created, added, removed }) => ({ shown, created, added, removed })),
    [
      { shown: "src/a.py", created: false, added: 2, removed: 2 },
      { shown: "neuf.txt", created: true, added: 1, removed: 0 },
      { shown: "/ailleurs/b.py", created: false, added: 1, removed: 1 },
    ],
  );
  assert.deepEqual(edits[0].lines, [["-", "x"], ["+", "y"], ["…", ""], ["-", "p"], ["+", "q"]]);
  assert.deepEqual(fragments(undefined), []);
});

test("cellOfEdits et cellOfRuns : les vraies données remplacent la supposition", () => {
  const guessed = readCell("await bash('make')");
  const runs = [{ command: "make all\nmake check", exitCode: 2, output: "" }];
  assert.deepEqual(cellOfRuns(guessed, runs), { kind: "shell", files: [], command: "make all" });
  assert.deepEqual(cellOfRuns({ kind: "python", files: [] }, runs).kind, "shell");
  assert.deepEqual(cellOfRuns({ kind: "edit", files: ["a"] }, runs), { kind: "edit", files: ["a"] }, "une édition reste une édition");
  const edits = fileEdits(fragments([{ type: "diff", path: "/w/n.txt", newText: "a\n" }]), "/w");
  const cell = cellOfEdits(guessed, edits);
  assert.equal(cell.kind, "write", "seulement des créations");
  assert.deepEqual(cell.files, ["n.txt"]);
  assert.deepEqual(cell.diff, { lines: [["+", "a"]], added: 1, removed: 0 });
});

test("isDeclined : texte fixe du refus seulement", () => {
  assert.ok(isDeclined("[permission-denied] The user declined this cell; nothing was executed."));
  assert.ok(!isDeclined("Traceback … [permission-denied]"));
  assert.ok(!isDeclined(undefined));
});

test("fileEdits : dossier de l'agent ouvert par un lien, chemin du pont résolu → chemin relatif", () => {
  // Trouvé par real-binary-check.mjs sur macOS : projet `/var/folders/…`, chemin du pont `/private/var/folders/…`.
  const real = realpathSync(mkdtempSync(join(tmpdir(), "ya-cells-")));
  mkdirSync(join(real, "projet"));
  const link = join(real, "lien");
  symlinkSync(join(real, "projet"), link);
  const edits = fileEdits(fragments([{ type: "diff", path: join(real, "projet", "src", "a.py"), newText: "x\n" }]), link);
  assert.equal(edits[0].shown, join("src", "a.py"));
});

test("cellOfRuns : l'en-tête montre la commande sortie en erreur (contrôle de Yusuf : ls puis false)", () => {
  const runs = [
    { command: "ls", exitCode: 0, output: "a\n" },
    { command: "false", exitCode: 1, output: "" },
  ];
  assert.equal(cellOfRuns({ kind: "shell", files: [] }, runs).command, "false");
  assert.equal(cellOfRuns({ kind: "shell", files: [] }, [runs[0]]).command, "ls", "aucune en erreur : la première");
});

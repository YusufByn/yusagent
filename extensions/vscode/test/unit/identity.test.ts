// yusAgent (YA-10, ticket 32, T5) : plus aucun nom d'identité d'Unharnessed dans le code du dépôt
// (`docs/tickets/32-identite-yusagent.md`, §7.1 et décisions du §9). On parcourt les fichiers suivis
// par git sous l'extension (code, webview, agent factice), `crates/`, `bench/` et le runtime Python.
// Les tests unitaires et e2e de l'extension ne sont pas parcourus : ils vérifient, eux, que l'ancien
// nom n'est plus servi ou qu'aucun « Unharnessed » n'est affiché.
//
// Ce qui est cherché (sensible à la casse) :
// - `unharnessed` en minuscules : noms de méthode (`_unharnessed/*`), clé `_meta.unharnessed`,
//   identifiants, nom du binaire. Le nom propre « Unharnessed » avec majuscule, dans la prose des
//   commentaires, est l'attribution (« repris d'Unharnessed, fichier:ligne ») : il n'est pas cherché ;
// - les noms CSS `--uh-*`, `uh-rise`, `uh-pop` ;
// - l'onglet Instructions supprimé (décision 4).
// Les chemins de leur dépôt (`crates/uh-daemon/...`) sont des citations de source : non cherchés.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { test } from "node:test";

import { EXTENSION_ROOT } from "../helpers";

const REPO = resolve(EXTENSION_ROOT, "..", "..");

const SCANNED = [
  "extensions/vscode/src",
  "extensions/vscode/webview",
  "extensions/vscode/test/fake-agent",
  "crates",
  "bench",
  "prime-agent-runtime/src",
];
const TEXT = /\.(rs|py|ts|tsx|mjs|js|css|json|md|sh|toml)$/;

const PATTERNS: [RegExp, string][] = [
  [/unharnessed/, "nom d'Unharnessed (méthode, clé, identifiant)"],
  [/--uh-|\buh-(rise|pop)\b/, "nom CSS d'Unharnessed"],
  [/modelInstructions|InstructionsTab|setInstructions|SettingsInstructions/, "onglet Instructions (supprimé, décision 4)"],
];

/**
 * Exceptions, une par citation légitime. `contains` : le texte exact de la ligne (absent : tout le
 * fichier). Chacune doit servir : une exception sans objet fait échouer le test.
 */
const EXCEPTIONS: { path: string; contains?: string; why: string }[] = [
  // Attribution : notice MIT et adresse du dépôt source.
  {
    path: "extensions/vscode/webview/ya-settings.css",
    contains: "github.com/unharnessed-agent/Unharnessed",
    why: "notice MIT et en-tête de la feuille reprise d'Unharnessed",
  },
  {
    path: "extensions/vscode/webview/ya-settings.css",
    contains: "(`--uh-*` en `--ya-*`, `uh-rise`, `uh-pop` en `ya-rise`, `ya-pop`)",
    why: "l'en-tête de la feuille reprise dit quels noms ont été passés à yusAgent",
  },
  // Commentaires qui citent la source (chemin, commit, méthode chez eux).
  {
    path: "prime-agent-runtime/src/rlm/ya_timeout.py",
    contains: "`unharnessed-runtime/src/rlm/bash.py`",
    why: "fichier source cité (repris d'Unharnessed, 569284b36)",
  },
  {
    path: "crates/pa-cli/src/ya_update_off.rs",
    contains: '"unharnessed',
    why: "titre du commit d'Unharnessed cité (75cfc357f)",
  },
  {
    path: "crates/pa-daemon/src/acp/ya_history.rs",
    contains: "`_unharnessed/sessions/delete` (`crates/uh-daemon/src/acp/mod.rs:638-697`",
    why: "méthode d'Unharnessed citée avec son fichier source",
  },
  {
    path: "crates/pa-daemon/src/acp/ya_history.rs",
    contains: "(Unharnessed's `_meta.unharnessed.saved`)",
    why: "clé d'Unharnessed citée comme origine de `saved`",
  },
  {
    path: "crates/pa-daemon/src/acp/ya_resources.rs",
    contains: "Unharnessed's `_unharnessed/*`",
    why: "méthodes d'Unharnessed citées comme origine des formes de `_yus/*`",
  },
  {
    // Ticket 40 (R5), relevé à X5 : le job de l'extension n'a pas tourné sur la PR #91.
    path: "crates/pa-daemon/src/acp/daemon/ya_providers.rs",
    contains: "Unharnessed's `_unharnessed/providers/*`",
    why: "méthodes d'Unharnessed citées comme origine des formes de `_yus/providers/*`",
  },
  // L'option du banc (décision 5) : elle désigne le dépôt d'Unharnessed qu'on mesure.
  {
    path: "bench/smoke/ab/ya09-bash-count.py",
    why: "option `--unharnessed` et fonction `unharnessed()` : le dépôt mesuré, pas notre produit",
  },
  // Tests qui vérifient que l'ancien nom n'est plus servi (décision 1).
  {
    path: "crates/pa-daemon/src/acp/ya_history_tests.rs",
    contains: '!serves("_unharnessed/sessions/delete")',
    why: "l'ancien nom de la suppression n'est plus servi",
  },
  {
    path: "crates/pa-cli/tests/ya_acp_resume_e2e.rs",
    contains: '"_unharnessed/sessions/delete",',
    why: "l'ancien nom de la suppression répond -32601",
  },
];

/**
 * Noms de forme repris d'Unharnessed et gardés (décision 2 : option 1). Ils ne portent aucune marque
 * et ne sont pas cherchés ; le test vérifie qu'aucun motif ne les attrape.
 */
const KEPT = [
  "turnStart", // début de chaque tour, dans le `_meta` du pont (ticket 25, T1)
  "turnEnd", // fin d'un tour rejoué (ticket 28)
  "saved", // fichier de session dans la réponse de `session/new` et `session/load` (ticket 28, C2)
];

function tracked(): string[] {
  const out = execFileSync("git", ["ls-files", "-z", "--", ...SCANNED], { cwd: REPO, encoding: "utf8" });
  return out.split("\0").filter((path) => TEXT.test(path) && existsSync(join(REPO, path)));
}

test("identité (ticket 32) : aucun nom d'Unharnessed dans le code, hors exceptions commentées", () => {
  const used = new Set<number>();
  const found: string[] = [];
  for (const path of tracked()) {
    const lines = readFileSync(join(REPO, path), "utf8").split("\n");
    lines.forEach((line, at) => {
      for (const [pattern, what] of PATTERNS) {
        if (!pattern.test(line)) continue;
        const allowed = EXCEPTIONS.findIndex((e) => e.path === path && (e.contains === undefined || line.includes(e.contains)));
        if (allowed >= 0) used.add(allowed);
        else found.push(`${path}:${at + 1} (${what}) : ${line.trim().slice(0, 120)}`);
      }
    });
  }
  assert.deepEqual(found, [], `noms d'identité restants :\n${found.join("\n")}`);
  const stale = EXCEPTIONS.filter((_, at) => !used.has(at)).map((e) => `${e.path} : ${e.contains ?? "(fichier)"}`);
  assert.deepEqual(stale, [], `exceptions sans objet :\n${stale.join("\n")}`);
});

test("identité (ticket 32) : les noms gardés ne sont pas cherchés", () => {
  for (const name of KEPT) for (const [pattern] of PATTERNS) assert.equal(pattern.test(name), false, name);
});

test("identité (ticket 32) : fichiers de l'onglet Instructions supprimés", () => {
  for (const path of ["webview/SettingsInstructions.tsx", "src/instructions.ts"]) {
    assert.equal(existsSync(join(EXTENSION_ROOT, path)), false, `${path} supprimé`);
  }
});

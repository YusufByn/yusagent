// yusAgent (YA-10, ticket 24, PR 3, étape 4) : l'« avant » d'un fichier modifié par une cellule,
// retrouvé à partir de son « après » (lu sur disque à la fin de la cellule) en défaisant les
// fragments de ses `edit()` (contenus ACP `diff` du pont : `old_str`, `new_str`, ligne de départ ;
// `skills/edit/src/edit/__init__.py`), du dernier au premier. Logique pure, testée par `node --test`.
//
// Chaque fragment est vérifié avant d'être défait (décision de Yusuf) : `new_str` doit commencer à
// la ligne annoncée. Sinon, ou si plusieurs « avant » sont possibles (pas de colonne : C14 a),
// pas de « Rejeter », avec la raison. `edit()` exige qu'`old_str` apparaisse une seule fois :
// un « avant » candidat qui ne le respecte pas est écarté.
//
// Ticket 26 (K1 a, K2 a) : avec la colonne du pont (`_yus/startColumn`, à partir de 0, en points de
// code comme Python les compte), une seule place est essayée, convertie en unités UTF-16 sur la
// ligne. Pour une suppression pure (`new_str` vide), la vérification ne voit rien : la place vient
// de la colonne seule. Sans colonne (binaire ou session d'avant le ticket 26), la recherche sur la
// ligne ci-dessous (C14 a).

import type { Fragment } from "./cells";

/** `before` : le texte d'avant la cellule, `null` si la cellule a créé le fichier. */
export type Undo = { ok: true; before: string | null } | { ok: false; reason: string };

/** Au-delà, une suppression pure (`new_str` vide) n'est pas cherchée colonne par colonne. */
const MAX_LINE = 10_000;

export function undo(after: string, fragments: Fragment[]): Undo {
  let text: string | null = after;
  for (let at = fragments.length - 1; at >= 0; at--) {
    const fragment = fragments[at];
    if (text === null) return { ok: false, reason: "the cell edited the file before creating it" };
    if (!fragment.oldText) {
      // Création (`old_str` vide, `oldText` absent) : le fichier n'existait pas, son texte était `new_str` en entier.
      if (text !== fragment.newText) return { ok: false, reason: "the file is no longer the text the cell created" };
      text = null;
      continue;
    }
    const line = fragment.startLine;
    if (line === undefined) return { ok: false, reason: "the edit has no start line" };
    const start = lineStart(text, line);
    if (start === undefined) return { ok: false, reason: `the file has no line ${line} any more` };
    const newline = text.indexOf("\n", start);
    // `old_str` commençait sur la ligne `line` : à un indice qui a `line - 1` retours avant lui.
    const last = newline < 0 ? text.length : newline;
    if (fragment.startColumn !== undefined) {
      const position = columnIndex(text, start, last, fragment.startColumn);
      if (position === undefined) return { ok: false, reason: `line ${line} is shorter than the edit's column` };
      if (!text.startsWith(fragment.newText, position)) return { ok: false, reason: `the edit's new text is no longer at line ${line}` };
      const candidate: string = text.slice(0, position) + fragment.oldText + text.slice(position + fragment.newText.length);
      if (occurrences(candidate, fragment.oldText) !== 1) return { ok: false, reason: `the edit's new text is no longer at line ${line}` };
      text = candidate;
      continue;
    }
    if (fragment.newText === "" && last - start > MAX_LINE) return { ok: false, reason: `line ${line} is too long to place the removed text` };
    const befores = new Set<string>();
    let found = false;
    for (let position = start; position <= last; position++) {
      if (!text.startsWith(fragment.newText, position)) continue;
      found = true;
      const candidate = text.slice(0, position) + fragment.oldText + text.slice(position + fragment.newText.length);
      if (occurrences(candidate, fragment.oldText) === 1) befores.add(candidate);
    }
    if (!found || befores.size === 0) return { ok: false, reason: `the edit's new text is no longer at line ${line}` };
    if (befores.size > 1) return { ok: false, reason: `the edit's place on line ${line} is ambiguous` };
    text = [...befores][0];
  }
  return { ok: true, before: text };
}

/**
 * L'indice (unités UTF-16) de la colonne `column` (points de code, à partir de 0) de la ligne qui va
 * de `start` à `last` (son retour à la ligne ou la fin du texte) ; undefined si la ligne est plus courte.
 */
function columnIndex(text: string, start: number, last: number, column: number): number | undefined {
  let at = start;
  for (let counted = 0; counted < column; counted++) {
    if (at >= last) return undefined;
    // Un caractère hors du plan de base (emoji…) : deux unités UTF-16, un point de code.
    at += (text.codePointAt(at) as number) > 0xffff ? 2 : 1;
  }
  return at;
}

/** L'indice du début de la ligne `line` (1 = la première), undefined si le texte en a moins. */
function lineStart(text: string, line: number): number | undefined {
  if (line < 1) return undefined;
  let at = 0;
  for (let current = 1; current < line; current++) {
    const newline = text.indexOf("\n", at);
    if (newline < 0) return undefined;
    at = newline + 1;
  }
  return at;
}

/** Le nombre d'occurrences de `needle` (non vide) dans `text`, sans chevauchement, comme `str.count`. */
function occurrences(text: string, needle: string): number {
  let count = 0;
  for (let at = text.indexOf(needle); at >= 0; at = text.indexOf(needle, at + needle.length)) count++;
  return count;
}

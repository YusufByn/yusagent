// yusAgent (YA-10, ticket 24, PR 3) : ce que notre pont envoie à la fin d'une cellule, pour sa carte
// (`crates/pa-daemon/src/acp/ya_cell_display.rs`, `wire_events.rs`, PR 1) :
// - `_meta` `_yus/bashRuns` : chaque `bash()` lu par la cellule (commande, code de sortie, sortie
//   plafonnée à 8 000 caractères par le kernel, durée) ;
// - un contenu ACP `diff` par `edit()` : `path` absolu, `oldText` absent pour un fichier créé,
//   fragments `old_str`/`new_str`, `_meta` `_yus/startLine` ;
// - une cellule refusée : `failed` avec le texte fixe de `DENIED_TEXT`
//   (`crates/pa-core/src/session_engine/ya_permissions.rs:63-64`).

import { isAbsolute, relative, sep } from "node:path";

import { fragmentLines } from "../cell";
import type { BashRun, Cell, DiffLine, FileEdit } from "../protocol";

export const BASH_RUNS_META = "_yus/bashRuns";
export const START_LINE_META = "_yus/startLine";
/** Début du texte fixe d'une cellule refusée (ou annulée pendant sa demande d'accord). */
export const DENIED_PREFIX = "[permission-denied]";

/** Un fragment d'`edit()`, tel que le pont l'envoie (les étapes suivantes en reconstruisent l'avant). */
export type Fragment = { path: string; oldText?: string; newText: string; startLine?: number };

/** Les `bash()` de la cellule, ou undefined (aucun, ou forme inattendue). */
export function bashRuns(meta: unknown): BashRun[] | undefined {
  const runs = (meta as Record<string, unknown> | undefined)?.[BASH_RUNS_META];
  if (!Array.isArray(runs)) return undefined;
  const out: BashRun[] = [];
  for (const run of runs as Record<string, unknown>[]) {
    if (!run || typeof run.command !== "string") continue;
    out.push({
      command: run.command,
      exitCode: typeof run.exitCode === "number" ? run.exitCode : null,
      output: typeof run.output === "string" ? run.output : "",
      ...(typeof run.durationMs === "number" && { durationMs: run.durationMs }),
    });
  }
  return out.length > 0 ? out : undefined;
}

/** Les contenus ACP `diff` d'un `tool_call_update`, dans l'ordre des `edit()`. */
export function fragments(content: unknown): Fragment[] {
  if (!Array.isArray(content)) return [];
  const out: Fragment[] = [];
  for (const block of content as Record<string, unknown>[]) {
    if (block?.type !== "diff" || typeof block.path !== "string" || typeof block.newText !== "string") continue;
    const line = (block._meta as Record<string, unknown> | undefined)?.[START_LINE_META];
    out.push({
      path: block.path,
      ...(typeof block.oldText === "string" && { oldText: block.oldText }),
      newText: block.newText,
      ...(typeof line === "number" && { startLine: line }),
    });
  }
  return out;
}

/** Les fichiers modifiés par la cellule, un par fichier, dans l'ordre du premier `edit()` de chacun. */
export function fileEdits(all: Fragment[], cwd: string): FileEdit[] {
  const byPath = new Map<string, Fragment[]>();
  for (const fragment of all) byPath.set(fragment.path, [...(byPath.get(fragment.path) ?? []), fragment]);
  return [...byPath].map(([path, edits]) => {
    const lines: DiffLine[] = [];
    for (const edit of edits) {
      if (lines.length > 0) lines.push(["…", ""]);
      lines.push(...fragmentLines(edit.oldText ?? "", edit.newText));
    }
    const count = (sign: DiffLine[0]) => lines.filter((line) => line[0] === sign).length;
    return {
      path,
      shown: shownPath(path, cwd),
      created: edits[0].oldText === undefined,
      lines,
      added: count("+"),
      removed: count("-"),
    };
  });
}

/** La carte d'après les vrais diffs : type, fichiers et lignes changées (C7), à la place de la supposition. */
export function cellOfEdits(cell: Cell | undefined, edits: FileEdit[]): Cell {
  const lines: DiffLine[] = [];
  for (const edit of edits) {
    if (lines.length > 0) lines.push(["…", ""]);
    lines.push(...edit.lines);
  }
  return {
    kind: edits.every((edit) => edit.created) ? "write" : "edit",
    files: edits.map((edit) => edit.shown),
    ...(cell?.command !== undefined && { command: cell.command }),
    diff: {
      lines,
      added: edits.reduce((n, edit) => n + edit.added, 0),
      removed: edits.reduce((n, edit) => n + edit.removed, 0),
    },
  };
}

/** La carte d'après les `bash()` réels (C6) : commande réelle dans l'en-tête, une fois finie. */
export function cellOfRuns(cell: Cell | undefined, runs: BashRun[]): Cell {
  const command = runs[0].command.trim().split("\n", 1)[0];
  const base: Cell = cell ?? { kind: "shell", files: [] };
  if (base.kind === "edit" || base.kind === "write" || base.kind === "view" || base.kind === "read") return base;
  return { ...base, kind: base.kind === "python" ? "shell" : base.kind, command };
}

/** Le texte de résultat d'une cellule refusée. */
export function isDeclined(output: string | undefined): boolean {
  return output?.trimStart().startsWith(DENIED_PREFIX) ?? false;
}

/** Chemin montré : relatif au dossier de l'agent quand il est dedans, sinon absolu. */
function shownPath(path: string, cwd: string): string {
  if (!cwd) return path;
  const inside = relative(cwd, path);
  return inside && !inside.startsWith(`..${sep}`) && inside !== ".." && !isAbsolute(inside) ? inside : path;
}

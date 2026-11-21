import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import type { Cell, DiffLine } from "./protocol";

/**
 * A cell's code with its strings taken out, each one now `"n"` with its text
 * (as Python reads it) at `strings[n]`, and its comments gone: the code a
 * file's text holds, or a comment, is not code the cell runs. `formatted`:
 * the f-strings, whose text the code computes.
 */
type Skeleton = { code: string; strings: string[]; formatted: Set<number> };

/** A string literal's prefix: `r`, `f`, `rb`… */
const PREFIX = /(?:^|\W)([rRbBuUfF]|[rR][bBfF]|[bBfF][rR])$/;
/** A cell that changes text before it writes it edits a file. */
const EDIT = /\.replace\(|\.sub\(/;
/** Python looking for files. */
const PY_SEARCH = /\.r?glob\(|(?<![\w.])os\.(?:walk|listdir|scandir)\(|\.iterdir\(/;
/** A command looking for files or in them. */
const SH_SEARCH = /^(?:cd\s+\S+\s*&&\s*)?(?:rg|grep|egrep|fgrep|ag|ack|fd|find|ls|tree|git\s+grep|git\s+ls-files)\b/;
/** What a backslash and the character after it read as. */
const ESCAPES: Record<string, string> = { "\n": "", "\\": "\\", "'": "'", '"': '"', a: "\x07", b: "\b", f: "\f", n: "\n", r: "\r", t: "\t", v: "\v" };
/** Kept lines shown on each side of a change; those further away fold into `…`. */
const CONTEXT = 3;
/** Past this many line pairs, a replacement's lines are all removed then all added, unmatched. */
const DIFF_CELLS = 250_000;

/**
 * What a Python cell does, guessed from its code: it edits a file (changes
 * the text it writes), writes one, looks at images (`attach_image`), runs a
 * command, looks for files, reads them, or none of these. When it does
 * several, the first of these wins. A guess, not what happened: an edit
 * whose text was not found still reads as an edit.
 */
export function readCell(source: string): Cell {
  const cell = skeleton(source);
  const { code } = cell;
  const names = assignments(cell);
  const reads = new Set<string>();
  const writes = new Set<string>();
  const views = new Set<string>();
  let wrote = false;
  let read = false;
  let viewed = false;
  const add = (files: Set<string>, path: string | undefined) => {
    // An f-string's path whose name the code computes, `src/{name}`, names no file.
    const name = path?.slice(path.lastIndexOf("/") + 1);
    if (path && name && !/[{\n]/.test(name) && path.length <= 300) files.add(path);
  };
  const file = (path: string | undefined, writing: boolean) => {
    if (writing) wrote = true;
    else read = true;
    add(writing ? writes : reads, path);
  };
  for (const call of code.matchAll(/(?<![\w.])(?:io\.|codecs\.)?open\(/g)) {
    const given = args(code, call.index + call[0].length - 1);
    if (given.length === 0) continue;
    file(literal(cell, names, given[0].replace(/^file\s*=\s*/, "")), opensToWrite(cell, names, given, 1));
  }
  for (const call of code.matchAll(/\.(write_text|write_bytes|read_text|read_bytes|open)\(/g)) {
    const on = receiver(code, call.index);
    if (call[1] === "open" && /^(?:io|codecs|os)$/.test(on)) continue;
    const given = args(code, call.index + call[0].length - 1);
    file(literal(cell, names, on), call[1] === "open" ? opensToWrite(cell, names, given, 0) : call[1].startsWith("write"));
  }
  for (const call of code.matchAll(/(?<![\w.])attach_image\(/g)) {
    viewed = true;
    for (const arg of args(code, call.index + call[0].length - 1)) add(views, literal(cell, names, arg));
  }
  // yusAgent (PR 3, C7) : `edit(path, old_str, new_str)` et `read(path)`, fonctions du kernel.
  const edited = yaKernelFiles(cell, names, file);
  const commands: (string | undefined)[] = [];
  for (const call of code.matchAll(/(?<![\w.])(?:bash|os\.system|subprocess\.(?:run|call|check_call|check_output|Popen))\(/g)) {
    const command = args(code, call.index + call[0].length - 1)[0];
    commands.push(command === undefined ? undefined : literal(cell, names, command));
  }
  if (edited) return { kind: edited.kind, files: [...writes], ...(edited.diff && { diff: edited.diff }) };
  if (wrote) {
    const edit = EDIT.test(code) || [...writes].some((path) => reads.has(path));
    if (!edit) {
      const diff = writings(cell);
      return diff ? { kind: "write", files: [...writes], diff } : { kind: "write", files: [...writes] };
    }
    const diff = replacements(cell);
    return diff ? { kind: "edit", files: [...writes], diff } : { kind: "edit", files: [...writes] };
  }
  if (viewed) return { kind: "view", files: [...views] };
  const searches = (command: string | undefined) => command !== undefined && SH_SEARCH.test(command.trim());
  const shell = commands.find((command) => !searches(command));
  if (commands.length > 0 && !commands.every(searches)) return withCommand({ kind: "shell", files: [] }, shell);
  if (commands.length > 0 || PY_SEARCH.test(code)) return withCommand({ kind: "search", files: [] }, commands[0]);
  if (read) return { kind: "read", files: [...reads] };
  return { kind: "python", files: [] };
}

/**
 * The folder a file the code names lies in, as the panel shows it when the
 * file is out of the agent's folder `cwd`: `~` for `home`, with its last
 * slash (`~/notes/`, `/etc/`); undefined when the file is in `cwd`.
 */
export function awayDir(path: string, cwd: string, home: string): string | undefined {
  const full = path === "~" || path.startsWith("~/") ? join(home, path.slice(1)) : resolve(cwd, path);
  const inside = relative(cwd, full);
  if (inside !== ".." && !inside.startsWith(`..${sep}`) && !isAbsolute(inside)) return undefined;
  const dir = dirname(full);
  const shown = dir === home || dir.startsWith(home + sep) ? `~${dir.slice(home.length)}` : dir;
  return shown.endsWith(sep) ? shown : shown + sep;
}

function withCommand(cell: Cell, command: string | undefined): Cell {
  const line = command?.trim().split("\n", 1)[0];
  if (line) cell.command = line;
  return cell;
}

/** Whether an `open` call writes: its mode, the argument at `at` or `mode=`, has `w`, `a`, `x` or `+`. */
function opensToWrite(cell: Skeleton, names: Map<string, string>, given: string[], at: number): boolean {
  const named = given.find((arg) => /^mode\s*=/.test(arg));
  const mode = named === undefined ? given[at] : named.replace(/^mode\s*=\s*/, "");
  if (mode === undefined || /^\w+\s*=/.test(mode)) return false;
  return /[wax+]/.test(literal(cell, names, mode) ?? "");
}

function skeleton(source: string): Skeleton {
  const strings: string[] = [];
  const formatted = new Set<number>();
  let code = "";
  let i = 0;
  while (i < source.length) {
    const c = source[i];
    if (c === "#") {
      while (i < source.length && source[i] !== "\n") i++;
      continue;
    }
    if (c !== '"' && c !== "'") {
      code += c;
      i++;
      continue;
    }
    const prefix = PREFIX.exec(code.slice(-3));
    const flags = prefix?.[1].toLowerCase() ?? "";
    if (prefix) code = code.slice(0, -prefix[1].length);
    const quote = source.startsWith(c.repeat(3), i) ? c.repeat(3) : c;
    let end = i + quote.length;
    // A backslash keeps the next character in, even in a raw string; a one-quote string ends with its line.
    while (end < source.length && !source.startsWith(quote, end)) {
      if (source[end] === "\\") end++;
      else if (quote.length === 1 && source[end] === "\n") break;
      end++;
    }
    const text = source.slice(i + quote.length, end);
    if (flags.includes("f")) formatted.add(strings.length);
    strings.push(flags.includes("r") ? text : unescape(text));
    code += `"${strings.length - 1}"`;
    i = source.startsWith(quote, end) ? end + quote.length : end;
  }
  return { code, strings, formatted };
}

/** A string's text as Python reads it: `\\n` a line break, `\\x41` an A; an unknown escape keeps its backslash. */
function unescape(text: string): string {
  return text.replace(/\\(x[0-9a-fA-F]{2}|u[0-9a-fA-F]{4}|U[0-9a-fA-F]{8}|[0-7]{1,3}|[\s\S])/g, (all, escape: string) => {
    if (escape.length === 1 && !/[0-7]/.test(escape)) return ESCAPES[escape] ?? all;
    const point = /^[xuU]/.test(escape) ? parseInt(escape.slice(1), 16) : parseInt(escape, 8);
    return point <= 0x10ffff ? String.fromCodePoint(point) : all;
  });
}

/**
 * The text an expression at `at` holds, when the code writes it out: a
 * string (not an f-string, whose text the code computes), a name set to one
 * before there, or their `dedent`.
 */
function texts(cell: Skeleton): (expression: string, at: number) => string | undefined {
  const assigned = [...cell.code.matchAll(/^[ \t]*(\w+)[ \t]*=(?!=)(.+)$/gm)];
  const text = (expression: string, at: number): string | undefined => {
    const expr = expression.trim();
    let match: RegExpExecArray | null;
    if ((match = /^"(\d+)"$/.exec(expr))) {
      const n = Number(match[1]);
      return cell.formatted.has(n) ? undefined : cell.strings[n];
    }
    if ((match = /^\(([^]*)\)$/.exec(expr))) return text(match[1], at);
    if ((match = /^(?:textwrap\.)?dedent\(([^]*)\)$/.exec(expr))) {
      const inner = text(match[1], at);
      return inner === undefined ? undefined : dedent(inner);
    }
    if (!/^\w+$/.test(expr)) return undefined;
    // The name as set last before here.
    for (let i = assigned.length - 1; i >= 0; i--) {
      const set = assigned[i];
      if (set[1] === expr && set.index < at) return text(set[2], set.index);
    }
    return undefined;
  };
  return text;
}

/**
 * What an edit's `.replace(old, new)` calls change, when its code writes
 * both texts out (texts). Each replacement's lines, kept, removed or added,
 * a `…` between two. Only the texts the code holds: no line of the file
 * around them.
 */
function replacements(cell: Skeleton): Cell["diff"] {
  const { code } = cell;
  const text = texts(cell);
  const lines: DiffLine[] = [];
  for (const call of code.matchAll(/\.replace\(/g)) {
    const given = args(code, call.index + call[0].length - 1);
    if (given.length < 2) continue;
    const before = text(given[0], call.index);
    const after = text(given[1], call.index);
    if (before === undefined || after === undefined || before === after) continue;
    if (lines.length > 0) lines.push(["…", ""]);
    lines.push(...hunk(before, after));
  }
  if (lines.length === 0) return undefined;
  const count = (sign: DiffLine[0]) => lines.filter((line) => line[0] === sign).length;
  return { lines, added: count("+"), removed: count("-") };
}

/**
 * What a cell that writes files writes, when its code writes the text out
 * (texts): what `.write_text`, `.write_bytes` or `.write` is given, all its
 * lines added, a `…` between two. Whether a file was there before, and what
 * it held, the code does not say: nothing is removed.
 */
function writings(cell: Skeleton): Cell["diff"] {
  const { code } = cell;
  const text = texts(cell);
  const lines: DiffLine[] = [];
  for (const call of code.matchAll(/\.(write_text|write_bytes|write)\(/g)) {
    if (call[1] === "write" && /^sys\.std(?:out|err)$/.test(receiver(code, call.index))) continue;
    const given = args(code, call.index + call[0].length - 1);
    const written = given.length > 0 ? text(given[0], call.index) : undefined;
    if (!written) continue;
    const rows = written.split("\n");
    // A text that ends with a line break: nothing after it.
    if (rows.length > 1 && rows.at(-1) === "") rows.pop();
    if (lines.length > 0) lines.push(["…", ""]);
    lines.push(...rows.map((row): DiffLine => ["+", row]));
  }
  if (lines.length === 0) return undefined;
  return { lines, added: lines.filter((line) => line[0] === "+").length, removed: 0 };
}

/** `textwrap.dedent`: the white space every non-blank line starts with, taken off. */
function dedent(text: string): string {
  const lines = text.split("\n");
  const indents = lines.filter((line) => line.trim()).map((line) => /^[ \t]*/.exec(line)![0]);
  let common = indents[0] ?? "";
  for (const indent of indents) while (!indent.startsWith(common)) common = common.slice(0, -1);
  return lines.map((line) => (line.trim() ? line.slice(common.length) : line.replace(/^[ \t]+$/, ""))).join("\n");
}

/**
 * yusAgent (PR 3, C7) : les appels `edit()` et `read()` du kernel (`skills/edit`, prompt v2).
 * Les fichiers vont dans les écritures ou les lectures (`file`) ; pour `edit()`, le type (`write`
 * si chaque appel crée son fichier, `old_str=""`) et le diff deviné de ses textes quand le code
 * les écrit. Undefined sans `edit()`. Une supposition : les vrais diffs arrivent à la fin.
 */
function yaKernelFiles(
  cell: Skeleton,
  names: Map<string, string>,
  file: (path: string | undefined, writing: boolean) => void,
): { kind: "edit" | "write"; diff?: Cell["diff"] } | undefined {
  const { code } = cell;
  const text = texts(cell);
  const argument = (given: string[], at: number, name: string): string | undefined => {
    const named = given.find((arg) => new RegExp(`^${name}\\s*=(?!=)`).test(arg));
    if (named !== undefined) return named.replace(/^\w+\s*=\s*/, "");
    const positional = given.slice(0, at + 1);
    return positional.length > at && !/^\w+\s*=(?!=)/.test(positional[at]) ? positional[at] : undefined;
  };
  for (const call of code.matchAll(/(?<![\w.])read\(/g)) {
    const path = argument(args(code, call.index + call[0].length - 1), 0, "path");
    if (path !== undefined) file(literal(cell, names, path), false);
  }
  let calls = 0;
  let created = true;
  const lines: DiffLine[] = [];
  for (const call of code.matchAll(/(?<![\w.])edit\(/g)) {
    const given = args(code, call.index + call[0].length - 1);
    const path = argument(given, 0, "path");
    if (path === undefined) continue;
    calls++;
    file(literal(cell, names, path), true);
    const oldArg = argument(given, 1, "old_str");
    const newArg = argument(given, 2, "new_str");
    const before = oldArg === undefined ? undefined : text(oldArg, call.index);
    const after = newArg === undefined ? undefined : text(newArg, call.index);
    if (before !== "") created = false;
    if (before === undefined || after === undefined || before === after) continue;
    if (lines.length > 0) lines.push(["…", ""]);
    lines.push(...fragmentLines(before, after));
  }
  if (calls === 0) return undefined;
  if (lines.length === 0) return { kind: created ? "write" : "edit" };
  const count = (sign: DiffLine[0]) => lines.filter((line) => line[0] === sign).length;
  return { kind: created ? "write" : "edit", diff: { lines, added: count("+"), removed: count("-") } };
}

/**
 * yusAgent (PR 3) : les lignes d'un fragment `old` → `new` (un `edit()`) ; `old` vide : un
 * fichier créé, toutes ses lignes ajoutées.
 */
export function fragmentLines(before: string, after: string): DiffLine[] {
  if (before !== "") return hunk(before, after);
  const rows = after.split("\n");
  if (rows.length > 1 && rows.at(-1) === "") rows.pop();
  return after === "" ? [] : rows.map((row): DiffLine => ["+", row]);
}

/** One replacement, `before` to `after`, line by line; kept lines far from a change fold into `…`. */
function hunk(before: string, after: string): DiffLine[] {
  const a = before.split("\n");
  const b = after.split("\n");
  // Texts that both end with a line break: nothing after it.
  if (a.length > 1 && b.length > 1 && a.at(-1) === "" && b.at(-1) === "") {
    a.pop();
    b.pop();
  }
  const rows = lineDiff(a, b);
  const near = rows.map(() => false);
  rows.forEach(([sign], at) => {
    if (sign === " ") return;
    for (let i = Math.max(0, at - CONTEXT); i <= Math.min(rows.length - 1, at + CONTEXT); i++) near[i] = true;
  });
  const out: DiffLine[] = [];
  rows.forEach((row, at) => {
    if (near[at]) out.push(row);
    else if (out.length > 0 && out.at(-1)![0] !== "…") out.push(["…", ""]);
  });
  if (out.at(-1)?.[0] === "…") out.pop();
  return out;
}

/** The lines both texts keep, in order (their longest common run); the others removed, then added. */
function lineDiff(a: string[], b: string[]): DiffLine[] {
  const kept = (lines: string[]) => lines.map((line): DiffLine => [" ", line]);
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start++;
  let end = 0;
  while (end < a.length - start && end < b.length - start && a[a.length - 1 - end] === b[b.length - 1 - end]) end++;
  const x = a.slice(start, a.length - end);
  const y = b.slice(start, b.length - end);
  const middle: DiffLine[] = [];
  if (x.length * y.length > DIFF_CELLS) {
    middle.push(...x.map((line): DiffLine => ["-", line]), ...y.map((line): DiffLine => ["+", line]));
  } else {
    // longest[i][j]: the longest common run of x[i..] and y[j..].
    const longest = Array.from({ length: x.length + 1 }, () => new Uint32Array(y.length + 1));
    for (let i = x.length - 1; i >= 0; i--) {
      for (let j = y.length - 1; j >= 0; j--) {
        longest[i][j] = x[i] === y[j] ? longest[i + 1][j + 1] + 1 : Math.max(longest[i + 1][j], longest[i][j + 1]);
      }
    }
    let i = 0;
    let j = 0;
    while (i < x.length || j < y.length) {
      if (i < x.length && j < y.length && x[i] === y[j]) {
        middle.push([" ", x[i]]);
        i++;
        j++;
      } else if (j >= y.length || (i < x.length && longest[i + 1][j] >= longest[i][j + 1])) {
        middle.push(["-", x[i++]]);
      } else {
        middle.push(["+", y[j++]]);
      }
    }
  }
  return [...kept(a.slice(0, start)), ...middle, ...kept(a.slice(a.length - end))];
}

/** `name = <a string, a path>` lines, in order: a later one wins. */
function assignments(cell: Skeleton): Map<string, string> {
  const names = new Map<string, string>();
  for (const line of cell.code.split("\n")) {
    const assign = /^\s*(\w+)\s*=(?!=)\s*(.+?)\s*$/.exec(line);
    if (!assign) continue;
    const value = literal(cell, names, assign[2]);
    if (value !== undefined) names.set(assign[1], value);
  }
  return names;
}

/**
 * The text an expression names, when the code says it: `"a.ts"`,
 * `Path("a.ts")`, a name set to one, `root / "a.ts"` (its end only),
 * `os.path.join(root, "a.ts")` (its end only), `["git", "status"]`.
 */
function literal(cell: Skeleton, names: Map<string, string>, expression: string): string | undefined {
  const expr = expression.trim();
  const text = (n: string) => cell.strings[Number(n)];
  const one = (value: string | undefined) => (value && !value.includes("\n") && value.length <= 300 ? value : undefined);
  let match: RegExpExecArray | null;
  if ((match = /^"(\d+)"$/.exec(expr))) return text(match[1]);
  if ((match = /^\((.*)\)$/.exec(expr))) return literal(cell, names, match[1]);
  if ((match = /^(?:pathlib\.)?Path\((.*)\)$/.exec(expr))) return literal(cell, names, match[1]);
  if (/^\w+$/.test(expr)) return names.get(expr);
  if ((match = /\/\s*"(\d+)"$/.exec(expr))) return one(text(match[1]));
  if ((match = /^os\.path\.join\(.*"(\d+)"\s*\)$/.exec(expr))) return one(text(match[1]));
  if ((match = /^\[\s*((?:"\d+"\s*,\s*)*"\d+")\s*,?\s*\]$/.exec(expr))) {
    return [...match[1].matchAll(/"(\d+)"/g)].map((part) => text(part[1])).join(" ");
  }
  return undefined;
}

/** The arguments of the call whose `(` is at `open`, split where its commas are. */
function args(code: string, open: number): string[] {
  const parts: string[] = [];
  let depth = 0;
  let start = open + 1;
  for (let i = open; i < code.length; i++) {
    const c = code[i];
    if (c === "(" || c === "[" || c === "{") depth++;
    else if (c === ")" || c === "]" || c === "}") {
      if (--depth === 0) {
        parts.push(code.slice(start, i));
        break;
      }
    } else if (c === "," && depth === 1) {
      parts.push(code.slice(start, i));
      start = i + 1;
    }
  }
  return parts.map((part) => part.trim()).filter(Boolean);
}

/** What a method is called on, the `.` at `dot`: `p`, `Path("0")`, `(root / "0")`. */
function receiver(code: string, dot: number): string {
  let i = dot;
  let depth = 0;
  while (i > 0) {
    const c = code[i - 1];
    if (c === ")" || c === "]") depth++;
    else if (c === "(" || c === "[") {
      if (depth === 0) break;
      depth--;
    } else if (depth === 0 && !/[\w."]/.test(c)) break;
    i--;
  }
  return code.slice(i, dot);
}

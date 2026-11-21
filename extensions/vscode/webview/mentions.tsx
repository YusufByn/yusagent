import { createContext, useContext, useMemo, type ReactNode } from "react";
import { mentions } from "../src/mentions";

/** The rows the `@` menu shows at most. */
const SHOWN = 50;

/**
 * The folder's files as the `@` menu has them, its subfolders' too, and the
 * ones open in an editor; a mention may also name a folder (ending with `/`).
 * `null` until listed.
 */
export type Paths = { entries: string[]; open: string[]; known: Set<string> };

export const PathsContext = createContext<Paths | null>(null);

export function toPaths(files: string[], open: string[]): Paths {
  const folders = new Set<string>();
  for (const file of files) {
    for (let at = file.indexOf("/"); at !== -1; at = file.indexOf("/", at + 1)) folders.add(file.slice(0, at + 1));
  }
  const listed = new Set(files);
  const entries = [...files, ...open.filter((file) => !listed.has(file))];
  return { entries, open, known: new Set([...folders, ...entries]) };
}

/** Whether a mention's path names a file or folder of `paths`; a folder without its `/` too. */
export function knows(paths: Paths | null): (path: string) => boolean {
  return (path) => !!paths && (paths.known.has(path) || paths.known.has(`${path}/`));
}

/**
 * The files matching what is typed after the `@`, best first: by name,
 * then anywhere in the path, then by its letters in order. Nothing typed:
 * the open files, then the others, the least deep first.
 */
export function findPaths(paths: Paths, query: string): string[] {
  const open = new Set(paths.open);
  const q = query.toLowerCase();
  if (!q) {
    const depth = (path: string) => path.split("/").length;
    const rest = paths.entries.filter((path) => !open.has(path)).sort((a, b) => depth(a) - depth(b));
    return [...paths.open, ...rest].slice(0, SHOWN);
  }
  const ranked: { path: string; tier: number }[] = [];
  for (const path of paths.entries) {
    const tier = rank(path.toLowerCase(), q);
    if (tier !== undefined) ranked.push({ path, tier });
  }
  ranked.sort(
    (a, b) =>
      a.tier - b.tier ||
      Number(open.has(b.path)) - Number(open.has(a.path)) ||
      a.path.length - b.path.length ||
      a.path.localeCompare(b.path),
  );
  return ranked.slice(0, SHOWN).map(({ path }) => path);
}

function rank(path: string, q: string): number | undefined {
  const name = nameOf(path);
  if (name.startsWith(q)) return 0;
  if (name.includes(q)) return 1;
  if (path.includes(q)) return 2;
  let at = 0;
  for (const char of path) if (char === q[at]) at++;
  return at === q.length ? 3 : undefined;
}

/** A path's last part: a folder keeps its `/`. */
export function nameOf(path: string): string {
  return path.slice(path.lastIndexOf("/", path.length - 2) + 1);
}

/** `text` with a pill under each mention of a known path. */
export function MentionText({ text }: { text: string }) {
  const paths = useContext(PathsContext);
  const found = useMemo(() => mentions(text, knows(paths)), [text, paths]);
  const parts: ReactNode[] = [];
  let from = 0;
  for (const { start, end } of found) {
    parts.push(text.slice(from, start));
    parts.push(
      <mark className="mention" key={start}>
        {text.slice(start, end)}
      </mark>,
    );
    from = end;
  }
  parts.push(text.slice(from));
  return <>{parts}</>;
}

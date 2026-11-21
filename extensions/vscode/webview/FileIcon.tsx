import type { CSSProperties } from "react";
import data from "./seti.json";

/**
 * VS Code's default file icons (scripts/seti.mjs): each icon a glyph, then its
 * colors on a dark and a light theme; without one, it takes the text's.
 */
const seti = data as {
  file: number;
  names: Record<string, number>;
  exts: Record<string, number>;
  langNames: Record<string, number>;
  langExts: Record<string, number>;
  icons: (string | null)[][];
};

/** A file's icon as VS Code's default theme picks it: by its name, its extension, then its language. */
export function FileIcon({ path }: { path: string }) {
  const [glyph, dark, light] = seti.icons[iconOf(path)];
  return (
    <span className="seti" style={{ "--seti-dark": dark ?? undefined, "--seti-light": light ?? undefined } as CSSProperties}>
      {glyph}
    </span>
  );
}

function iconOf(path: string): number {
  const name = path.slice(path.lastIndexOf("/") + 1).toLowerCase();
  // `a.test.ts` has the extensions `test.ts`, then `ts`: the longest one wins.
  const exts = [...name.matchAll(/\./g)].map((dot) => name.slice(dot.index + 1));
  const by = (table: Record<string, number>) => exts.map((ext) => table[ext]).find((icon) => icon !== undefined);
  return seti.names[name] ?? by(seti.exts) ?? seti.langNames[name] ?? by(seti.langExts) ?? seti.file;
}

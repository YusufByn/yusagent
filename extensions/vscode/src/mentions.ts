// `@` mentions in a message: a path of the workspace after an `@` that
// starts a word, quoted when it holds a space (`@"docs/read me.md"`), a
// folder ending with `/`. The panel lays a pill under each one it knows; the
// extension links each one that exists to the prompt.

/** A mention in a text: from its `@` to its end, and the path it names. */
export type Mention = { start: number; end: number; path: string };

const MENTION = /(^|\s)@(?:"([^"\n]+)"|([^\s"]+))/g;
/** What ends a sentence rather than a path: `see @a.ts.` names `a.ts`. */
const TRAILING = /[.,;:!?)\]]+$/;

/** The mentions in `text` whose path `known` says names something. */
export function mentions(text: string, known: (path: string) => boolean): Mention[] {
  const found: Mention[] = [];
  for (const match of text.matchAll(MENTION)) {
    const start = match.index + match[1].length;
    const quoted = match[2];
    if (quoted !== undefined) {
      if (known(quoted)) found.push({ start, end: start + quoted.length + 3, path: quoted });
      continue;
    }
    const raw = match[3];
    const path = known(raw) ? raw : raw.replace(TRAILING, "");
    if (path && known(path)) found.push({ start, end: start + 1 + path.length, path });
  }
  return found;
}

/** How `path` is written after its `@`. */
export function mentionText(path: string): string {
  return /\s/.test(path) ? `@"${path}"` : `@${path}`;
}

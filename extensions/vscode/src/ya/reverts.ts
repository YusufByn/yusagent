// yusAgent (YA-10, ticket 24, PR 3, étape 4, E6 a) : « Rejeter » une modification faite par une
// cellule. `edit()` écrit pendant la cellule (approuvée en entier avant de tourner, D2) : rejeter
// veut dire restaurer le fichier après coup, puis le dire au modèle par un texte fixe à la fin du
// message suivant (C11).
//
// Pour chaque fichier modifié par une cellule (C8 : par fichier et par cellule) :
// - à la fin de la cellule, son « après » est lu sur disque et son « avant » retrouvé en défaisant
//   les fragments (`src/ya/diffs.ts`) ; si c'est impossible, pas de « Rejeter », avec la raison ;
// - « Rejeter » est refusé pendant un tour (garde-fou 1), pour une cellule d'une session
//   précédente (C13), et si le fichier n'est plus l'« après » de la cellule (garde-fou 2 : texte de
//   l'éditeur, modifications non enregistrées comprises, sinon disque) ;
// - sinon le fichier reprend son « avant » (fichier créé : retiré, à la corbeille sous VS Code,
//   C12), et `[user-reverted] <chemin relatif>` attend le message suivant.

import type { FileEdit, RevertStatus } from "../protocol";
import type { Fragment } from "./cells";
import type { DiffFiles } from "./diff-files";
import { undo } from "./diffs";

/** Le texte fixe ajouté au message suivant, une ligne par fichier rejeté (C11). */
export const REVERTED_PREFIX = "[user-reverted]";

type Entry = {
  session: string | undefined;
  shown: string;
  fragments: Fragment[];
  /** L'« après » de la cellule, lu sur disque à sa fin. */
  after?: string;
  /** L'« avant » : `null` pour un fichier que la cellule a créé. */
  before?: string | null;
  status: RevertStatus;
};

export class Reverts {
  private readonly entries = new Map<string, Entry>();
  private notices: string[] = [];

  constructor(private readonly files: () => DiffFiles) {}

  /** À la fin d'une cellule : son « après » sur disque et son « avant », fichier par fichier. */
  async record(toolId: string, session: string | undefined, edits: FileEdit[], fragments: Fragment[]): Promise<void> {
    await Promise.all(
      edits.map(async (edit) => {
        const entry: Entry = {
          session,
          shown: edit.shown,
          fragments: fragments.filter((fragment) => fragment.path === edit.path),
          status: { state: "checking" },
        };
        this.entries.set(key(toolId, edit.path), entry);
        let after: string | undefined;
        try {
          after = await this.files().readDisk(edit.path);
        } catch (error) {
          entry.status = { state: "blocked", reason: `the file could not be read: ${String(error)}` };
          return;
        }
        if (after === undefined) {
          entry.status = { state: "blocked", reason: "the file no longer exists" };
          return;
        }
        const undone = undo(after, entry.fragments);
        if (!undone.ok) {
          entry.status = { state: "blocked", reason: undone.reason };
          return;
        }
        entry.after = after;
        entry.before = undone.before;
        entry.status = { state: "ready" };
      }),
    );
  }

  status(toolId: string, path: string): RevertStatus | undefined {
    return this.entries.get(key(toolId, path))?.status;
  }

  /** Les cellules d'une autre session que `session` ne se rejettent plus (C13). */
  expire(session: string | undefined): boolean {
    let changed = false;
    for (const entry of this.entries.values()) {
      if (entry.session === session || entry.status.state === "blocked" || entry.status.state === "reverted") continue;
      entry.status = { state: "blocked", reason: "the cell ran in an earlier session" };
      changed = true;
    }
    return changed;
  }

  /** « Rejeter » : le fichier reprend son « avant », ou la raison du refus est notée. */
  async revert(toolId: string, path: string, now: { busy: boolean; session: string | undefined }): Promise<RevertStatus | undefined> {
    const entry = this.entries.get(key(toolId, path));
    if (!entry || entry.status.state !== "ready" || entry.after === undefined || entry.before === undefined) return entry?.status;
    const refuse = (refused: string): RevertStatus => (entry.status = { state: "ready", refused });
    if (now.busy) return refuse("Not while a turn runs.");
    if (now.session !== entry.session) {
      entry.status = { state: "blocked", reason: "the cell ran in an earlier session" };
      return entry.status;
    }
    let current: string | undefined;
    try {
      current = await this.files().readCurrent(path);
    } catch (error) {
      return refuse(`Not reverted: the file could not be read (${String(error)}).`);
    }
    if (current !== entry.after) {
      return refuse("Not reverted: the file changed since this cell (a later edit, or yours).");
    }
    try {
      if (entry.before === null) await this.files().remove(path);
      else await this.files().write(path, entry.before);
    } catch (error) {
      return refuse(`Not reverted: ${String(error)}`);
    }
    entry.status = { state: "reverted" };
    const notice = `${REVERTED_PREFIX} ${entry.shown}`;
    if (!this.notices.includes(notice)) this.notices.push(notice);
    return entry.status;
  }

  /** L'avant et l'après de la cellule dans `vscode.diff` ; rien si l'avant n'a pas pu être retrouvé. */
  async openDiff(toolId: string, path: string): Promise<boolean> {
    const entry = this.entries.get(key(toolId, path));
    if (!entry || entry.after === undefined || entry.before === undefined) return false;
    await this.files().showDiff(path, entry.before ?? "", entry.after, `${entry.shown} (before ↔ after the cell)`);
    return true;
  }

  /** Les lignes à ajouter au message suivant ; elles ne partent qu'une fois. */
  takeNotices(): string[] {
    const notices = this.notices;
    this.notices = [];
    return notices;
  }
}

function key(toolId: string, path: string): string {
  return `${toolId}\u0000${path}`;
}

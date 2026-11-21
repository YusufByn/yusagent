// yusAgent (YA-10, ticket 24, PR 3, étape 4) : les accès aux fichiers des diffs, derrière une
// interface. Par défaut, Node (`fs`) : c'est ce qu'utilisent les tests unitaires. L'extension
// branche la version VS Code à l'activation (`src/ya/diff-files-vscode.ts` : tampon de l'éditeur,
// `workspace.applyEdit` puis enregistrement, corbeille, `vscode.diff`).

import { readFile, rm, writeFile } from "node:fs/promises";

export interface DiffFiles {
  /** Le texte sur disque (UTF-8), undefined si le fichier n'existe pas. */
  readDisk(path: string): Promise<string | undefined>;
  /** Le texte que voit l'utilisateur : celui de l'éditeur s'il est ouvert (non enregistré compris), sinon le disque. */
  readCurrent(path: string): Promise<string | undefined>;
  /** Remplace tout le texte du fichier, enregistré. */
  write(path: string, text: string): Promise<void>;
  /** Retire un fichier que la cellule a créé (VS Code : à la corbeille, C12). */
  remove(path: string): Promise<void>;
  /** L'avant et l'après d'une cellule côte à côte, en lecture seule (C9). */
  showDiff(path: string, before: string, after: string, title: string): Promise<void>;
}

const nodeFiles: DiffFiles = {
  async readDisk(path) {
    try {
      return await readFile(path, "utf8");
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code === "ENOENT") return undefined;
      throw error;
    }
  },
  readCurrent(path) {
    return this.readDisk(path);
  },
  async write(path, text) {
    await writeFile(path, text, "utf8");
  },
  async remove(path) {
    await rm(path);
  },
  async showDiff() {},
};

let current: DiffFiles = nodeFiles;

export function diffFiles(): DiffFiles {
  return current;
}

/** À l'activation de l'extension : la version VS Code. */
export function setDiffFiles(files: DiffFiles): void {
  current = files;
}

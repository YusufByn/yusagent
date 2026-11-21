// yusAgent (YA-10, ticket 24, PR 3, étape 4) : les accès aux fichiers des diffs sous VS Code
// (`src/ya/diff-files.ts`).
// - « après » d'une cellule : lu sur disque (`workspace.fs`), pas dans un éditeur ouvert, qui ne
//   recharge le fichier qu'un peu après l'écriture du kernel ;
// - texte actuel (garde-fou 2) : celui de l'éditeur s'il est ouvert, modifications non enregistrées
//   comprises, sinon le disque ;
// - restauration par `workspace.applyEdit` (annulable dans l'éditeur), puis enregistrement ;
// - fichier créé par la cellule : à la corbeille (`useTrash`, C12) ;
// - `vscode.diff` entre deux documents en lecture seule (C9), schéma `yusagent-diff`.

import * as vscode from "vscode";

import type { DiffFiles } from "./diff-files";

export class DiffDocuments implements vscode.TextDocumentContentProvider, vscode.Disposable {
  static readonly scheme = "yusagent-diff";
  private readonly texts = new Map<string, string>();
  private next = 0;
  private readonly closing = vscode.workspace.onDidCloseTextDocument((document) => this.texts.delete(document.uri.toString()));

  provideTextDocumentContent(uri: vscode.Uri): string {
    return this.texts.get(uri.toString()) ?? "";
  }

  /** Un document en lecture seule, nommé comme le fichier (sa langue en vient). */
  uri(path: string, side: "before" | "after", text: string): vscode.Uri {
    const name = path.slice(path.lastIndexOf("/") + 1);
    const uri = vscode.Uri.from({ scheme: DiffDocuments.scheme, path: `/${++this.next}/${side}/${name}` });
    this.texts.set(uri.toString(), text);
    return uri;
  }

  dispose(): void {
    this.closing.dispose();
  }
}

export function vscodeDiffFiles(documents: DiffDocuments): DiffFiles {
  const decoder = new TextDecoder("utf-8");
  const readDisk = async (path: string): Promise<string | undefined> => {
    try {
      return decoder.decode(await vscode.workspace.fs.readFile(vscode.Uri.file(path)));
    } catch (error) {
      if (error instanceof vscode.FileSystemError && error.code === "FileNotFound") return undefined;
      throw error;
    }
  };
  return {
    readDisk,
    async readCurrent(path) {
      const open = vscode.workspace.textDocuments.find((document) => document.uri.scheme === "file" && document.uri.fsPath === path);
      return open ? open.getText() : readDisk(path);
    },
    async write(path, text) {
      const document = await vscode.workspace.openTextDocument(vscode.Uri.file(path));
      const edit = new vscode.WorkspaceEdit();
      edit.replace(document.uri, new vscode.Range(document.positionAt(0), document.positionAt(document.getText().length)), text);
      if (!(await vscode.workspace.applyEdit(edit))) throw new Error("the editor refused the edit");
      if (!(await document.save())) throw new Error("the file was restored in the editor but could not be saved");
    },
    async remove(path) {
      await vscode.workspace.fs.delete(vscode.Uri.file(path), { useTrash: true });
    },
    async showDiff(path, before, after, title) {
      await vscode.commands.executeCommand(
        "vscode.diff",
        documents.uri(path, "before", before),
        documents.uri(path, "after", after),
        title,
        { preview: true },
      );
    },
  };
}

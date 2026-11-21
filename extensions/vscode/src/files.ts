import { execFile } from "node:child_process";
import { existsSync } from "node:fs";
import { isAbsolute, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import * as vscode from "vscode";
import { mentions } from "./mentions";

/** The files the `@` menu gets at most. */
const LIMIT = 20_000;

/** A mention's link in a prompt: the agent gets the path, not the content. */
export type Link = { type: "resource_link"; uri: string; name: string };

/**
 * The agent's folder as the `@` menu lists it, and the mentions of a
 * message, linked to its prompt. In a git repository, its files less the
 * ignored ones; elsewhere, VS Code's search less its excluded ones.
 */
export class Files {
  constructor(
    /** The agent's folder. */
    readonly cwd: string,
    private readonly log: vscode.LogOutputChannel,
  ) {}

  /** The files, relative to the folder and `/`-separated; the open ones among them first. */
  async list(): Promise<{ files: string[]; open: string[] }> {
    // Without a folder, the agent runs in the home directory: not listed.
    if (!vscode.workspace.workspaceFolders?.length) return { files: [], open: [] };
    const git = await this.gitFiles();
    const files = git ?? (await this.searchFiles());
    const open = vscode.window.tabGroups.all
      .flatMap((group) => group.tabs)
      .flatMap((tab) => (tab.input instanceof vscode.TabInputText ? [this.relative(tab.input.uri)] : []))
      .filter((path): path is string => path !== undefined);
    this.log.info(`files: ${files.length} listed by ${git ? "git" : "search"}, ${open.length} open`);
    return { files, open: [...new Set(open)] };
  }

  /** The mentions of `text` that name something, as links: each path once. */
  links(text: string): Link[] {
    const exists = (path: string) => resolve(this.cwd, path) !== this.cwd && existsSync(resolve(this.cwd, path));
    const uris = new Map<string, Link>();
    for (const { path } of mentions(text, exists)) {
      const uri = pathToFileURL(resolve(this.cwd, path)).href;
      if (!uris.has(uri)) uris.set(uri, { type: "resource_link", uri, name: path });
    }
    return [...uris.values()];
  }

  private gitFiles(): Promise<string[] | undefined> {
    const args = ["ls-files", "--cached", "--others", "--exclude-standard", "-z"];
    return new Promise((done) => {
      execFile("git", args, { cwd: this.cwd, maxBuffer: 256 * 1024 * 1024 }, (error, stdout) => {
        done(error ? undefined : [...new Set(stdout.split("\0").filter(Boolean))].slice(0, LIMIT));
      });
    });
  }

  private async searchFiles(): Promise<string[]> {
    const config = vscode.workspace.getConfiguration();
    const globs = ["files.exclude", "search.exclude"].flatMap((key) =>
      Object.entries(config.get<Record<string, unknown>>(key) ?? {})
        .filter(([, on]) => on === true)
        .map(([glob]) => glob),
    );
    const exclude = globs.length > 0 ? `{${globs.join(",")}}` : null;
    const uris = await vscode.workspace.findFiles(new vscode.RelativePattern(this.cwd, "**/*"), exclude, LIMIT);
    return uris
      .map((uri) => this.relative(uri))
      .filter((path): path is string => path !== undefined)
      .sort();
  }

  /** `uri`'s path in the folder; `undefined` outside it. */
  private relative(uri: vscode.Uri): string | undefined {
    if (uri.scheme !== "file") return undefined;
    const path = relative(this.cwd, uri.fsPath);
    if (!path || path.startsWith("..") || isAbsolute(path)) return undefined;
    return path.split(sep).join("/");
  }
}

/**
 * A replayed prompt as it was written: the agent keeps its mentions' links
 * as its last lines (`file:///…`), the ones its mentions account for go.
 */
export function withoutLinks(text: string): string {
  const lines = text.split("\n");
  let at = lines.length;
  while (at > 0 && /^file:\/\/\S+$/.test(lines[at - 1])) at--;
  if (at === lines.length || at === 0) return text;
  const body = lines.slice(0, at).join("\n");
  const linked = lines.slice(at).map(filePath);
  const names = (path: string) => (abs: string | undefined) =>
    abs !== undefined && (abs === path || abs.endsWith(sep + path.replace(/\/$/, "").split("/").join(sep)));
  const found = mentions(body, (path) => linked.some(names(path)));
  return linked.every((abs) => found.some(({ path }) => names(path)(abs))) ? body : text;
}

/** A conversation's title without its mentions' links. */
export function titleWithoutLinks(title: string): string {
  return title.replace(/\s*file:\/\/\S*/g, "").trim();
}

function filePath(uri: string): string | undefined {
  try {
    return fileURLToPath(uri);
  } catch {
    return undefined;
  }
}

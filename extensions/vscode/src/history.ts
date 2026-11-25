import type * as vscode from "vscode";
import { errorText, type SavedSession } from "./agent";
import type { Conversations } from "./conversations";
import { titleWithoutLinks } from "./files";
import type { HistoryState, SavedChat } from "./protocol";
import type { ShownAgent } from "./shown";
import { yaResultMeta } from "./ya/meta"; // yusAgent (ticket 28)

// Deletes a saved conversation (crates/uh-daemon/src/acp/history.rs).
const DELETE = "_yus/sessions/delete";

/** A `session/list` row, the parts read here. */
type Listed = {
  sessionId: string;
  title?: string;
  updatedAt?: string;
  _meta?: unknown;
};

/**
 * The saved conversations of the panel's folder, for the history menu. The
 * agent lists them from its sessions folder, a file each; the list is read
 * again each time the menu opens, and after a delete. Which one is in the
 * panel, and which ones work in the background, the conversations tell.
 */
export class History {
  private chats: Omit<SavedChat, "current" | "mark">[] | null = null;
  private files = new Map<string, string>();
  private error: string | undefined;
  /** yusAgent (ticket 28) : la ligne `current` du pont, tant que la conversation du panneau n'a pas d'id. */
  private listedCurrent: string | undefined;
  private readonly listeners = new Set<() => void>();

  constructor(
    private readonly agent: ShownAgent,
    private readonly conversations: Conversations,
    private readonly cwd: string,
    private readonly log: vscode.LogOutputChannel,
  ) {
    conversations.onMarks(() => this.changed());
    conversations.onShow(() => {
      this.listedCurrent = undefined;
      this.changed();
    });
  }

  get current(): HistoryState {
    const shown = this.conversations.shown.id ?? this.listedCurrent;
    const chats = this.chats?.map((chat) => {
      const mark = this.conversations.mark(chat.id);
      return { ...chat, current: chat.id === shown, ...(mark ? { mark } : {}) };
    });
    return { chats: chats ?? null, unread: this.conversations.unseen, error: this.error };
  }

  onChange(listener: () => void): { dispose(): void } {
    this.listeners.add(listener);
    return { dispose: () => this.listeners.delete(listener) };
  }

  /** A listed conversation, to open; `undefined` for the one in the panel. */
  saved(id: string): SavedSession | undefined {
    const path = this.files.get(id);
    const chat = this.chats?.find((chat) => chat.id === id);
    return path && chat && id !== (this.conversations.shown.id ?? this.listedCurrent) ? { id, path } : undefined;
  }

  async list(): Promise<void> {
    try {
      const { sessions } = await this.agent.request<{ sessions: Listed[] }>("session/list", { cwd: this.cwd });
      // yusAgent (ticket 28, C2) : `path` et `current` dans le `_meta` du pont (`src/ya/meta.ts`).
      this.files = new Map(sessions.map((row) => [row.sessionId, yaResultMeta(row).path ?? ""]));
      this.listedCurrent = sessions.find((row) => yaResultMeta(row).current)?.sessionId;
      this.chats = sessions.map((row) => ({
        id: row.sessionId,
        title: row.title && titleWithoutLinks(row.title),
        updated: row.updatedAt ? Date.parse(row.updatedAt) : 0,
      }));
      this.error = undefined;
      const rows = (this.current.chats ?? []).map((chat) => {
        const how = chat.current ? " (current)" : chat.mark ? ` (${chat.mark})` : "";
        return `${chat.id} "${chat.title ?? ""}"${how}`;
      });
      const listed = `${this.chats.length} conversation${this.chats.length === 1 ? "" : "s"}`;
      const button = `history button: ${this.conversations.unseen ? "unread" : "read"}`;
      this.log.info(`history: ${listed}${rows.length ? `: ${rows.join(", ")}` : ""}; ${button}`);
    } catch (error) {
      this.error = errorText(error);
      this.log.warn(`history: not listed: ${this.error}`);
    }
    this.changed();
  }

  async remove(id: string): Promise<void> {
    try {
      await this.agent.request(DELETE, { sessionId: id });
      this.log.info(`history: ${id} deleted`);
      await this.list();
    } catch (error) {
      this.error = errorText(error);
      this.log.warn(`history: delete refused: ${this.error}`);
      this.changed();
    }
  }

  private changed(): void {
    for (const listener of this.listeners) listener();
  }
}

import { existsSync } from "node:fs";
import type * as vscode from "vscode";
import { Agent, type SavedSession } from "./agent";
import { Chat } from "./chat";
import type { Files } from "./files";
import { Kernel } from "./kernel";
import type { ChatMark, Draft } from "./protocol";
import { Session } from "./session";
import type { ShownAgent } from "./shown";
import { dropEmpty } from "./ya/empty"; // yusAgent (ticket 28, D2)

/** A conversation in the background sleeps after this long without work, like Prime's default. */
const SLEEP_AFTER_MS = 90 * 60_000;
const NO_DRAFT: Draft = { text: "", images: [] };

/**
 * One conversation and its own agent process: its session, its chat, its
 * kernel. It keeps working while another one is in the panel, until it is
 * closed (deleted, asleep, VS Code closing).
 */
export class Conversation {
  readonly agent: Agent;
  readonly session: Session;
  readonly chat: Chat;
  readonly kernel: Kernel;
  /** The message box while another conversation is in the panel. */
  draft: Draft = NO_DRAFT;
  /** yusAgent (ticket 28, D2) : ouverte neuve, pas depuis un fichier (rien à garder si elle reste vide). */
  fresh = false;

  constructor(
    readonly tag: number,
    binary: string,
    cwd: string,
    version: string,
    files: Files,
    log: vscode.LogOutputChannel,
    onNew: () => void,
  ) {
    this.agent = new Agent(binary, cwd, version, log);
    // Before `start`: they listen to the agent's notifications.
    this.session = new Session(this.agent, cwd, log);
    this.chat = new Chat(this.agent, this.session, files, log, onNew);
    this.kernel = new Kernel(this.agent, this.session, log);
  }

  /** Its id in the history, once its session opened. */
  get id(): string | undefined {
    return this.session.saved?.id ?? this.agent.resumed?.id;
  }

  /** Its saved file, once it has one on disk. */
  get file(): SavedSession | undefined {
    const saved = this.session.saved ?? this.agent.resumed;
    return saved && existsSync(saved.path) ? saved : undefined;
  }

  /** It works, or will on its own: a turn, waiting messages, a goal, a background command. */
  get busy(): boolean {
    return this.chat.working || this.chat.waiting || this.chat.goalGoing || this.kernel.running;
  }

  /**
   * Nothing was ever asked in it: there is nothing to come back to. yusAgent (ticket 28, D1) : jugé
   * sur le chat (aucun tour, rien en attente, rien en cours) ; notre démon écrit le fichier dès
   * `session/new` (M1).
   */
  get empty(): boolean {
    return !this.busy && !this.chat.hasTurns;
  }

  /** The agent starts again on its file, which comes back; before its first message, on a new one. */
  async restart(): Promise<void> {
    const saved = this.file;
    this.chat.reset(saved !== undefined);
    await this.agent.restart(saved);
  }
}

/**
 * The open conversations, one agent each, and the one in the panel. A new
 * conversation starts another agent; one already open comes back as it is,
 * one that is not starts again on its file. A conversation the panel left
 * keeps working; it sleeps (its agent stops, its kernel saved) after a while
 * without work.
 */
export class Conversations {
  private readonly open: Conversation[] = [];
  private shownOne: Conversation | undefined;
  private nextTag = 1;
  /** Whether each one's turn ran when last looked at: its end, unseen, marks it. */
  private readonly worked = new Map<Conversation, boolean>();
  /** Done in the background and not seen since, by id; it outlives a sleep. */
  private readonly unread = new Set<string>();
  private readonly timers = new Map<Conversation, NodeJS.Timeout>();
  /** Agents still stopping, by id: one holds its file until it exits. */
  private readonly stopping = new Map<string, Promise<void>>();
  private readonly showListeners = new Set<() => void>();
  private readonly createListeners = new Set<(conversation: Conversation) => void>();
  private readonly markListeners = new Set<() => void>();

  constructor(
    private readonly binary: string,
    private readonly cwd: string,
    private readonly version: string,
    private readonly files: Files,
    private readonly shownAgent: ShownAgent,
    private readonly log: vscode.LogOutputChannel,
    private readonly sleepAfter = SLEEP_AFTER_MS,
  ) {}

  /** The conversation in the panel; there is one from `start` on. */
  get shown(): Conversation {
    if (!this.shownOne) throw new Error("no conversation yet");
    return this.shownOne;
  }

  isShown(conversation: Conversation): boolean {
    return conversation === this.shownOne;
  }

  /** Another conversation is in the panel. */
  onShow(listener: () => void): { dispose(): void } {
    this.showListeners.add(listener);
    return { dispose: () => this.showListeners.delete(listener) };
  }

  /** A conversation is created, before its agent starts. */
  onCreate(listener: (conversation: Conversation) => void): { dispose(): void } {
    this.createListeners.add(listener);
    return { dispose: () => this.createListeners.delete(listener) };
  }

  /** A conversation starts or stops working, or its end is seen. */
  onMarks(listener: () => void): { dispose(): void } {
    this.markListeners.add(listener);
    return { dispose: () => this.markListeners.delete(listener) };
  }

  /** A conversation ended in the background, unseen since. */
  get unseen(): boolean {
    return this.unread.size > 0;
  }

  /** The history's mark for a saved conversation. */
  mark(id: string): ChatMark | undefined {
    const conversation = this.open.find((open) => open.id === id);
    if (conversation?.chat.working) return "working";
    return this.unread.has(id) ? "unread" : undefined;
  }

  /** The first conversation, a new one. */
  start(): void {
    this.begin(undefined, NO_DRAFT);
  }

  /** A new conversation in the panel; `draft` stays with the one it leaves. */
  newChat(draft: Draft = NO_DRAFT): void {
    this.begin(undefined, draft);
  }

  /** A saved conversation in the panel: as it is when it is open, else its agent starts on its file. */
  openSaved(saved: SavedSession, draft: Draft = NO_DRAFT): void {
    const open = this.open.find((conversation) => conversation.id === saved.id);
    if (open) this.show(open, draft);
    else this.begin(saved, draft);
  }

  /** The conversation in the panel starts again on its file. */
  restart(): Promise<void> {
    this.log.info(`conversations: [${this.shown.tag}] restarts`);
    return this.shown.restart();
  }

  /** Before a delete: its agent holds the file. Not the one in the panel. */
  async release(id: string): Promise<void> {
    const conversation = this.open.find((open) => open.id === id);
    this.unread.delete(id);
    if (conversation && conversation !== this.shownOne) await this.close(conversation, "deleted");
  }

  /** VS Code closes: every agent stops at once, each saving its kernel. */
  async stopAll(): Promise<void> {
    for (const timer of this.timers.values()) clearTimeout(timer);
    this.timers.clear();
    await Promise.all(this.open.map((conversation) => conversation.agent.stop()));
  }

  /** A new agent, on a new conversation or on `saved`, in the panel. */
  private begin(saved: SavedSession | undefined, draft: Draft): void {
    const conversation = this.create(saved);
    // A saved one comes back as the agent replays it.
    if (saved) conversation.chat.reset(true);
    this.show(conversation, draft);
    const stopping = saved && this.stopping.get(saved.id);
    if (stopping) void stopping.then(() => conversation.agent.start(saved));
    else conversation.agent.start(saved);
  }

  private create(saved?: SavedSession): Conversation {
    const tag = this.nextTag++;
    const log = tagged(this.log, tag);
    const conversation = new Conversation(tag, this.binary, this.cwd, this.version, this.files, log, () =>
      this.newChat(),
    );
    conversation.fresh = !saved; // yusAgent (ticket 28, D2)
    this.open.push(conversation);
    this.shownAgent.attach(conversation.agent);
    const update = () => this.update(conversation);
    conversation.agent.onChange(update);
    conversation.chat.onPost(update);
    conversation.kernel.onChange(update);
    for (const listener of this.createListeners) listener(conversation);
    this.log.info(`conversations: [${tag}] ${saved ? `opens ${saved.id}` : "new"}`);
    return conversation;
  }

  private show(next: Conversation, draft: Draft): void {
    const left = this.shownOne;
    if (left === next) return;
    if (left) left.draft = draft;
    this.shownOne = next;
    this.update(next);
    this.shownAgent.show(next.agent);
    this.log.info(`conversations: [${next.tag}] in the panel`);
    for (const listener of this.showListeners) listener();
    if (!left) return;
    if (left.empty) void this.leaveEmpty(left, next);
    else this.update(left);
  }

  /**
   * yusAgent (ticket 28, D2) : une conversation vide se ferme ; ouverte neuve, elle est ensuite
   * supprimée par l'agent de celle qui la remplace, une fois sa session ouverte (session du démon
   * arrêtée, fichier retiré ; `src/ya/empty.ts`).
   */
  private async leaveEmpty(left: Conversation, next: Conversation): Promise<void> {
    const id = left.fresh ? left.session.saved?.id : undefined;
    await this.close(left, "nothing asked in it");
    if (id) await dropEmpty(next, id, tagged(this.log, left.tag));
  }

  /** It started or stopped working: its mark, and whether it may sleep. */
  private update(conversation: Conversation): void {
    if (!this.open.includes(conversation)) return;
    const working = conversation.chat.working;
    const before = this.worked.get(conversation) ?? false;
    this.worked.set(conversation, working);
    const away = conversation !== this.shownOne;
    const { id } = conversation;
    if (before && !working && away && id) {
      this.unread.add(id);
      this.log.info(`conversations: [${conversation.tag}] done in the background`);
    }
    if (before !== working) this.marked();
    // Seen in the panel; a saved one has its id once its agent starts.
    if (!away && id && this.unread.delete(id)) this.marked();
    if (!away || conversation.busy) this.disarm(conversation);
    else if (!this.timers.has(conversation)) this.arm(conversation);
  }

  private arm(conversation: Conversation): void {
    const timer = setTimeout(() => {
      this.timers.delete(conversation);
      if (conversation === this.shownOne || conversation.busy) return;
      void this.close(conversation, `asleep after ${duration(this.sleepAfter)} without work`);
    }, this.sleepAfter);
    this.timers.set(conversation, timer);
  }

  private disarm(conversation: Conversation): void {
    clearTimeout(this.timers.get(conversation));
    this.timers.delete(conversation);
  }

  private async close(conversation: Conversation, why: string): Promise<void> {
    const at = this.open.indexOf(conversation);
    if (at < 0) return;
    this.open.splice(at, 1);
    this.disarm(conversation);
    this.worked.delete(conversation);
    this.shownAgent.detach(conversation.agent);
    this.log.info(`conversations: [${conversation.tag}] closes (${why})`);
    const { id } = conversation;
    const stopped = conversation.agent.stop();
    if (id) this.stopping.set(id, stopped);
    await stopped;
    if (id && this.stopping.get(id) === stopped) this.stopping.delete(id);
    this.marked();
  }

  private marked(): void {
    for (const listener of this.markListeners) listener();
  }
}

/** `90 min`, `3 s`. */
function duration(ms: number): string {
  return ms < 60_000 ? `${ms / 1000} s` : `${ms / 60_000} min`;
}

/** The log, each line marked with its conversation: `[2] chat: turn 1 done`. */
function tagged(log: vscode.LogOutputChannel, tag: number): vscode.LogOutputChannel {
  const mark = (line: string) => `[${tag}] ${line}`;
  const marked: vscode.LogOutputChannel = Object.create(log);
  marked.info = (line: string, ...args: unknown[]) => log.info(mark(line), ...args);
  marked.warn = (line: string, ...args: unknown[]) => log.warn(mark(line), ...args);
  marked.error = (line: string | Error, ...args: unknown[]) => log.error(typeof line === "string" ? mark(line) : line, ...args);
  return marked;
}

import type * as vscode from "vscode";
import { errorText, type Agent } from "./agent";
import { homedir } from "node:os";
import { awayDir, readCell } from "./cell";
import { withoutLinks, type Files } from "./files";
import type {
  Attachment,
  Cell,
  ChatCommand,
  ChatEntry,
  ChatPatch,
  ChatState,
  EntryHead,
  Goal,
  HostMessage,
  QueuedMessage,
  Thumb,
  TurnHead,
  TurnItem,
  Usage,
} from "./protocol";
import type { Session } from "./session";
// yusAgent (YA-10, ticket 24, PR I) : adaptation à notre pont, dans des modules à part.
import { authText, isAuthFailure } from "./ya/auth";
import { features } from "./ya/methods";
import { yaMeta } from "./ya/meta";
import { noticeLines } from "./ya/notice";
import { bashRuns, cellOfEdits, cellOfRuns, fileEdits, fragments, isDeclined, type Fragment } from "./ya/cells";
import { diffFiles } from "./ya/diff-files";
import { Reverts } from "./ya/reverts";

/** How long streamed text gathers before the panel gets it. */
const FLUSH_MS = 40;
/** A tool's output past this is cut for the panel; the agent has it all. */
const OUTPUT_LIMIT = 20_000;
/** The agent's kernel came back (crates/uh-daemon/src/acp/kernel.rs). */
const RESTORED = "_unharnessed/kernel/restored";
/** A message into the running turn (crates/uh-daemon/src/acp/steer.rs). */
const STEER = "_unharnessed/session/steer";

/** The panel's own commands, beside the agent's. */

const PANEL_COMMANDS: ChatCommand[] = [{ name: "new", description: "Start a new conversation", kind: "panel" }];

// yusAgent (ticket 25, E2) : `heartbeat`, réveil planifié qui n'existe qu'avec le démon.
const WAKES = { bash: "a background command ended", quota: "a usage limit reset", goal: "a goal's pause ended", heartbeat: "a scheduled heartbeat" };

type Turn = TurnHead & { items: TurnItem[] };
/**
 * `edit`: the turn whose prompt it replaces. `quiet`: the goal's pause or
 * play, from the panel's buttons; shown neither waiting nor as a prompt.
 * `held`: back in the message box to be changed; the queue waits at it.
 */
type Queued = { id: number; text: string; images: Attachment[]; edit?: number; quiet?: boolean; held?: boolean };
type Tool = Extract<TurnItem, { kind: "tool" }>;
type Compaction = Extract<TurnItem, { kind: "compaction" }>;
/** A compaction's mark: its turn, and where in it. */
type Mark = { turn: Turn; at: number };
export type ChatMessage = Extract<HostMessage, { type: "chat" | "chatPatch" | "chatEdit" }>;

/** `_meta.unharnessed` on an update (crates/uh-daemon/src/acp/meta.rs), the parts read here. */
type Meta = {
  promptTurnId?: number;
  phase?: "event" | "responseBoundary" | "terminalQuiescence";
  outcome?: "result" | "error";
  turnStart?: { origin: "prompt" | "wake" | "steer"; wake?: keyof typeof WAKES; rewindTo?: string };
  command?: { name: string; text: string; severity: string };
  thinking?: { state: "start" | "end"; messageId: string };
  /** The provider cut the agent's answer at the output limit; it went on. */
  outputLimit?: boolean;
  /** How a turn of a loaded conversation ended. */
  turnEnd?: { state: "done" | "cancelled" | "failed"; error?: string };
  compaction?: { tokensBefore?: number; summary?: string };
  goal?: { status: string; objective?: string; createdAt?: number; updatedAt?: number; lastReason?: string };
  ipython?: { attachments?: unknown[]; errorEname?: string };
  usage?: Usage;
};

/** A `session/update`'s `update`, the parts read here. */
type Update = {
  sessionUpdate: string;
  messageId?: string;
  /** A chunk's block (text, or an image of a prompt), or a tool's output. */
  content?: { type?: string; text?: string; mimeType?: string; data?: string } | { content?: { text?: string } }[];
  toolCallId?: string;
  title?: string;
  status?: string;
  rawInput?: unknown;
  availableCommands?: {
    name: string;
    description: string;
    input?: { hint?: string } | null;
    _meta?: { unharnessed?: { kind?: string; source?: string } };
  }[];
  _meta?: { unharnessed?: Meta };
};

/**
 * The conversation in the session: what the user sent, what the agent
 * streamed back, the turns it started on its own (a background command
 * ending, a usage limit resetting). One turn runs at a time: a message
 * sent meanwhile waits, then goes when the turn ends, unless it is sent
 * into the turn; a stop pauses the waiting ones. A saved conversation
 * comes back as the agent replays it, turn after turn, before its session
 * opens. It outlives the panel; the panel gets it whole when it opens,
 * then what changed.
 */
export class Chat {
  private entries: ChatEntry[] = [];
  private queue: Queued[] = [];
  /** Stopped: the waiting messages wait for play. */
  private paused = false;
  /** For the running turn, not taken yet: the turn ending without them, they come first in line. */
  private steering: Queued[] = [];
  /** Those of them the agent has: the others wait for it to say the turn started. */
  private steered = new Set<Queued>();
  private commands: ChatCommand[] = PANEL_COMMANDS;
  private nextId = 1;
  /** Our prompt, sent and not answered yet. */
  private sent: Turn | undefined;
  /** The turn the agent's updates go to: ours, or one it started. */
  private live: Turn | undefined;
  /** The agent's turn ids, ours for this process. */
  private turns = new Map<number, Turn>();
  /** When each turn began here; a turn the agent replays has no time. */
  private began = new WeakMap<Turn, number>();
  /** A tool's whole output, when the panel's is cut. */
  private wholeOutput = new WeakMap<Tool, string>();
  /** Each prompt's images, whole, for when it is edited: the panel holds their thumbnails. */
  private images = new Map<number, Attachment[]>();
  /** The last streamed item: chunks of the same message and kind join it. */
  private streaming: { item: TurnItem; key: string } | undefined;
  /** The reasoning under way, timed from when it opened. */
  private thinking: { turn: Turn; at: number } | undefined;
  /** The goal's status, to tell when it changes. */
  private goal: string | undefined;
  /** The goal the panel shows: one reached or failed stays until the next message. */
  private goalState: Goal | null = null;
  private goalChanged = false;
  private usage: Usage | null = null;
  private usageChanged = false;
  /** The compaction under way, marked in the turn it runs in. */
  private compacting: Mark | undefined;
  /** Bumped per agent process, so a late answer never lands on the next one. */
  private run = 0;
  /** A saved conversation comes back: the panel gets it once it is whole. */
  private loading = false;
  /** The kernel came back while the conversation did: its line goes after it. */
  private restored: boolean | undefined;
  private dirtyHeads = new Set<number>();
  private dirtyItems = new Map<number, Set<number>>();
  private commandsChanged = false;
  /** yusAgent (PR 3, étape 4) : « Rejeter » les modifications des cellules (`src/ya/reverts.ts`). */
  private readonly yaReverts = new Reverts(diffFiles);
  private timer: NodeJS.Timeout | undefined;
  private readonly listeners = new Set<(message: ChatMessage) => void>();

  constructor(
    private readonly agent: Agent,
    private readonly session: Session,
    private readonly files: Files,
    private readonly log: vscode.LogOutputChannel,
    /** `/new`: another conversation takes the panel. */
    private readonly onNew: () => void,
  ) {
    agent.onNotification("session/update", (params) => this.onUpdate(params));
    // yusAgent (PR 3) : les demandes d'accord vont à la boîte (`webview/ya-permission.tsx`), plus de ligne.
    agent.onNotification(RESTORED, (params) => this.onRestored(params));
    agent.onChange((state) => {
      this.run++;
      this.turns.clear();
      this.streaming = undefined;
      this.goal = undefined;
      this.showGoal(null);
      this.commands = PANEL_COMMANDS;
      this.commandsChanged = true;
      // A new process reports its own conversation.
      this.usage = null;
      this.usageChanged = true;
      for (const turn of [this.sent, this.live]) {
        if (turn?.state === "running") this.end(turn, "failed", "The agent stopped.");
      }
      this.compacting = undefined;
      for (const entry of this.entries) {
        if (entry.kind === "turn" && entry.rewindTo !== undefined) {
          delete entry.rewindTo;
          this.dirtyHeads.add(entry.id);
        }
      }
      this.sent = this.live = undefined;
      if (state.kind !== "ready") {
        this.requeue();
        this.pause();
      }
      // The agent stopped before the conversation came back.
      if (state.kind === "failed" && this.loading) {
        this.loading = false;
        this.post({ type: "chat", chat: this.current });
      }
      // A conversation went on before: the agent no longer remembers it.
      const last = this.entries.at(-1);
      if (state.kind === "ready" && last && last.kind !== "restart") {
        const divider: ChatEntry = { kind: "restart", id: this.nextId++ };
        this.entries.push(divider);
        this.dirtyHeads.add(divider.id);
      }
      this.changed();
    });
    session.onChange(() => {
      const state = session.current;
      if (this.loading && (state?.kind === "open" || state?.kind === "failed")) this.loaded();
      // yusAgent (PR 3, C13) : une autre session, les cellules d'avant ne se rejettent plus.
      if (this.yaReverts.expire(this.session.sessionId)) this.yaRefreshAllTools();
      // Messages sent before the session opened go once it is.
      this.next();
    });
  }

  get current(): ChatState {
    return {
      entries: this.entries,
      busy: this.busy,
      loading: this.loading,
      queue: this.queued(),
      paused: this.paused,
      commands: this.commands,
      usage: this.usage,
      goal: this.goalState,
    };
  }

  onPost(listener: (message: ChatMessage) => void): { dispose(): void } {
    this.listeners.add(listener);
    return { dispose: () => this.listeners.delete(listener) };
  }

  /** A turn runs: ours, or one the agent started. */
  get working(): boolean {
    return this.busy;
  }

  /** yusAgent (ticket 28, D1) : au moins un tour, rejoué ou d'ici. */
  get hasTurns(): boolean {
    return this.entries.some((entry) => entry.kind === "turn");
  }

  /** Messages wait for the session to open, the running turn to end, or play. */
  get waiting(): boolean {
    return this.queue.length > 0 || this.steering.length > 0;
  }

  /** A goal goes on: the agent continues it on its own. */
  get goalGoing(): boolean {
    return this.goalState?.status === "active";
  }

  /**
   * `/new` starts a new conversation; anything else goes, now or after the
   * running turn, or `now` into it. An `edit` replaces that turn's prompt:
   * the agent goes back to before it, and the turns from it on leave the
   * conversation; or that waiting message, in its place.
   */
  send(text: string, images: Attachment[], edit?: number, now = false): void {
    if (text.trim() === "/new" && images.length === 0) {
      this.onNew();
      return;
    }
    const waiting = this.queue.find((message) => message.id === edit && !message.quiet);
    if (waiting) {
      Object.assign(waiting, { text, images, held: false });
      this.log.info(`chat: waiting message ${waiting.id} changed (${text.length} chars, ${count(images.length, "image")})`);
      if (now) this.sendNow(waiting.id);
      else this.next();
      return;
    }
    const message = { id: this.nextId++, text, images, edit };
    // A goal reached or failed leaves with the next message.
    if (this.goalState?.status === "complete" || this.goalState?.status === "error") this.showGoal(null);
    if (now && edit === undefined) {
      this.now(message);
      return;
    }
    if (this.busy || !this.session.sessionId) {
      this.queue.push(message);
      this.log.info(`chat: queued (${this.queue.length} waiting)`);
      this.changed();
      return;
    }
    void this.start(message);
  }

  /**
   * `/name` for the skill `name`, as the agent takes it: `/skill:name`. A
   * command or prompt of that name keeps it.
   */
  private skillCommand(text: string): string {
    const name = /^\/([^\s:]+)(?=\s|$)/.exec(text)?.[1];
    if (!name || this.commands.some((command) => command.name === name)) return text;
    const skill = this.commands.some((command) => command.kind === "skill" && command.name === `skill:${name}`);
    return skill ? `/skill:${text.slice(1)}` : text;
  }

  /** Stops the running turn; the waiting messages wait for play. */
  cancel(): void {
    this.pause();
    const sessionId = this.session.sessionId;
    if (!sessionId || !this.busy) return;
    this.log.info("chat: stop requested");
    this.agent.notify("session/cancel", { sessionId });
    // A turn the agent started answers no request: it ends here.
    if (this.live && this.live !== this.sent) this.end(this.live, "cancelled");
  }

  /** The goal waits: the running turn stops, then `/goal pause` goes. */
  pauseGoal(): void {
    if (this.queue.some((message) => message.quiet)) return;
    this.log.info("chat: goal paused from the panel");
    this.cancel();
    this.control("/goal pause");
  }

  /** The goal goes on: `/goal resume`, once nothing runs. */
  resumeGoal(): void {
    if (this.queue.some((message) => message.quiet)) return;
    this.log.info("chat: goal resumed from the panel");
    this.control("/goal resume");
  }

  /** The goal goes: the running turn stops, then `/goal clear`. One reached or failed just leaves the box. */
  endGoal(): void {
    if (this.goalState?.status === "complete" || this.goalState?.status === "error") {
      this.showGoal(null);
      return;
    }
    this.log.info("chat: goal ended from the panel");
    this.cancel();
    this.control("/goal clear");
  }

  /** A goal command from the panel's buttons: first in line. */
  private control(text: string): void {
    this.queue.unshift({ id: this.nextId++, text, images: [], quiet: true });
    this.next();
  }

  /** A sent prompt back in the message box, to be changed and sent again; or a waiting one, held meanwhile. */
  edit(id: number): void {
    const waiting = this.queue.find((message) => message.id === id && !message.quiet);
    if (waiting) {
      this.release();
      waiting.held = true;
      const { text, images } = waiting;
      this.log.info(`chat: waiting message ${id} back in the message box to change (${text.length} chars, ${count(images.length, "image")})`);
      this.post({ type: "chatEdit", id, text, images, queued: true });
      this.changed();
      return;
    }
    const turn = this.entries.find((entry) => entry.id === id);
    if (turn?.kind !== "turn" || !turn.prompt || turn.rewindTo === undefined) return;
    const { text } = turn.prompt;
    const images = this.images.get(id) ?? [];
    this.log.info(`chat: turn ${id} back in the message box to edit (${text.length} chars, ${count(images.length, "image")})`);
    this.post({ type: "chatEdit", id, text, images, queued: false });
  }

  /** The waiting message being changed (`id`, else any) stays as it was: the queue goes on. */
  release(id?: number): void {
    const held = this.queue.filter((message) => message.held && (id === undefined || message.id === id));
    if (held.length === 0) return;
    for (const message of held) message.held = false;
    this.log.info(`chat: waiting message ${held.map((message) => message.id).join(", ")} left as it was`);
    this.next();
  }

  /** Tool `id`'s code, or its whole output, uncut; a Python cell's code says so. */
  toolPart(id: string, part: "input" | "output"): { text: string; python: boolean } | undefined {
    for (const entry of this.entries) {
      if (entry.kind !== "turn") continue;
      const tool = entry.items.find((item): item is Tool => item.kind === "tool" && item.id === id);
      if (!tool) continue;
      const text = part === "input" ? tool.input : (this.wholeOutput.get(tool) ?? tool.output);
      return text === undefined ? undefined : { text, python: tool.cell !== undefined };
    }
    return undefined;
  }

  unqueue(id: number): void {
    this.queue = this.queue.filter((message) => message.id !== id);
    this.log.info(`chat: waiting message ${id} removed`);
    this.next();
  }

  /** Waiting message `id` goes to place `to` among the waiting ones. */
  move(id: number, to: number): void {
    const message = this.queue.find((waiting) => waiting.id === id && !waiting.quiet);
    if (!message) return;
    const others = this.queue.filter((waiting) => waiting !== message);
    const before = others.filter((waiting) => !waiting.quiet)[to];
    others.splice(before ? others.indexOf(before) : others.length, 0, message);
    this.queue = others;
    const shown = this.queue.filter((waiting) => !waiting.quiet);
    this.log.info(`chat: waiting message ${id} moved to ${shown.indexOf(message) + 1} of ${shown.length}`);
    this.next();
  }

  /** Waiting message `id`, now: into the running turn, else ahead of the others. */
  sendNow(id: number): void {
    const at = this.queue.findIndex((message) => message.id === id && !message.quiet && !message.held);
    if (at < 0) return;
    const [message] = this.queue.splice(at, 1);
    this.now(message);
  }

  /** The paused queue plays again. */
  resume(): void {
    if (!this.paused) return;
    this.paused = false;
    this.log.info("chat: queue plays again");
    this.next();
  }

  /**
   * Into the running turn (a turn editing an earlier prompt cannot join
   * one); else sent, ahead of the waiting ones.
   */
  private now(message: Queued): void {
    if (this.busy && this.session.sessionId && message.edit === undefined) {
      this.steering.push(message);
      this.steerAll();
      this.changed();
      return;
    }
    if (!this.busy && this.session.sessionId) {
      void this.start(message);
      return;
    }
    this.queue.unshift(message);
    this.log.info(`chat: message ${message.id} first in line`);
    this.next();
  }

  /** The agent's id for the turn running, once it said it started. */
  private get liveTurn(): number | undefined {
    for (const [id, turn] of this.turns) if (turn === this.live && turn.state === "running") return id;
    return undefined;
  }

  /** What is for the running turn goes to the agent, once it said the turn started. */
  private steerAll(): void {
    const turn = this.liveTurn;
    if (turn === undefined) return;
    for (const message of this.steering) {
      if (this.steered.has(message)) continue;
      this.steered.add(message);
      void this.steer(message, turn);
    }
  }

  /** Into running turn `turn`: the agent reads it before its next step. One it refuses comes first in line. */
  private async steer(message: Queued, turn: number): Promise<void> {
    const sessionId = this.session.sessionId;
    if (!sessionId) return;
    const run = this.run;
    try {
      await this.agent.request(STEER, { sessionId, prompt: this.prompt(message), turn });
      if (run !== this.run) return;
      const { id, text, images } = message;
      this.log.info(`chat: message ${id} sent into the running turn (${text.length} chars, ${count(images.length, "image")})`);
    } catch (error) {
      if (run !== this.run) return;
      this.log.info(`chat: message ${message.id} not sent into the turn: ${errorText(error)}`);
      const at = this.steering.indexOf(message);
      if (at < 0) return;
      this.steering.splice(at, 1);
      this.steered.delete(message);
      this.queue.unshift(message);
      this.next();
    }
  }

  /** Stopped: the waiting messages, and those sent into the turn it does not take, wait for play. */
  private pause(): void {
    // A goal command the buttons sent is not the user's to send again.
    this.queue = this.queue.filter((message) => !message.quiet);
    const waiting = this.queue.length + this.steering.length;
    if (waiting === 0 || this.paused) return;
    this.paused = true;
    this.log.info(`chat: queue paused (${waiting} waiting)`);
    this.changed();
  }

  /** What was sent into the turn and not taken: first in line. */
  private requeue(): void {
    if (this.steering.length === 0) return;
    this.log.info(`chat: ${count(this.steering.length, "message")} not taken by the turn, first in line`);
    this.queue.unshift(...this.steering);
    this.steering = [];
    this.steered.clear();
  }

  /**
   * Empty, before the agent restarts: on a new conversation, or on the saved
   * one (`loading`), which comes back here as the agent replays it.
   */
  reset(loading: boolean): void {
    this.requeue();
    this.pause();
    // Nothing the stopping process still sends lands in the next conversation.
    this.run++;
    this.turns.clear();
    this.images.clear();
    this.streaming = this.thinking = this.compacting = undefined;
    this.entries = [];
    this.sent = this.live = undefined;
    this.usage = null;
    this.goal = undefined;
    this.showGoal(null);
    this.loading = loading;
    this.restored = undefined;
    this.post({ type: "chat", chat: this.current });
  }

  /** The saved conversation is back (or the agent could not load it). */
  private loaded(): void {
    this.loading = false;
    if (this.session.current?.kind === "open") {
      const turns = this.entries.filter((entry) => entry.kind === "turn") as Turn[];
      const stopped = turns.filter((turn) => turn.state === "cancelled").length;
      const stops = stopped > 0 ? `; ${stopped} stopped` : "";
      this.log.info(`chat: loaded ${count(turns.length, "turn")}; ${summary(turns.flatMap((turn) => turn.items))}${stops}`);
      if (this.restored !== undefined) this.kernelLine(this.restored);
      // yusAgent (ticket 28, D4) : un tour que l'agent faisait encore au chargement (C9, gardé ouvert
      // par le pont) reste en cours : il occupe le chat (messages en attente, « Stop ») jusqu'à sa
      // frontière, sans durée affichée (son début n'est pas connu ici).
      const running = [...this.turns.values()].find((turn) => turn.state === "running");
      if (running) {
        this.live = running;
        this.log.info(`chat: turn ${running.id} still running in the agent`);
      }
    }
    this.restored = undefined;
    this.post({ type: "chat", chat: this.current });
  }

  /** The kernel came back with the conversation's variables, or could not. */
  private onRestored(params: unknown): void {
    const { sessionId, restored } = params as { sessionId: string; restored: boolean };
    if (sessionId !== this.session.updatesFrom) return;
    if (this.loading) {
      this.restored = restored;
      return;
    }
    const line = this.kernelLine(restored);
    this.dirtyHeads.add(line.id);
    this.changed();
  }

  private kernelLine(restored: boolean): ChatEntry {
    const line: ChatEntry = { kind: "kernel", id: this.nextId++, restored, at: Date.now() };
    this.entries.push(line);
    this.log.info(`chat: kernel ${restored ? "restored" : "not restored"}`);
    return line;
  }

  private get busy(): boolean {
    return this.sent !== undefined || this.live !== undefined;
  }

  private async start({ id, text, images, edit, quiet }: Queued): Promise<void> {
    const sessionId = this.session.sessionId;
    if (!sessionId) return;
    // yusAgent (PR 3, C11) : `[user-reverted] <chemin>` à la fin du message suivant un « Rejeter »,
    // après une ligne vide, dans le même bloc texte (jamais au préfixe) ; pas sur une commande `/…`.
    const reverted = quiet || text.trimStart().startsWith("/") ? [] : this.yaReverts.takeNotices();
    if (reverted.length > 0) {
      text = `${text}\n\n${reverted.join("\n")}`;
      this.log.info(`chat: turn ${id} tells the agent: ${reverted.join(", ")}`);
    }
    const run = this.run;
    const turn: Turn = {
      kind: "turn",
      id,
      prompt: quiet ? undefined : { text, images: images.map(thumbOf) },
      state: "running",
      items: [],
    };
    this.images.set(id, images);
    this.sent = turn;
    this.began.set(turn, Date.now());
    if (edit !== undefined) {
      const error = await this.rewind(sessionId, edit);
      if (run !== this.run) return;
      if (error) {
        this.entries.push(turn);
        this.end(turn, "failed", `It could not go back to the message: ${error}`);
        return;
      }
      // The edited turn and what came after leave; the panel gets the rest whole.
      const at = this.entries.findIndex((entry) => entry.id === edit);
      if (at >= 0) this.entries.splice(at);
      this.entries.push(turn);
      this.post({ type: "chat", chat: this.current });
    } else {
      this.entries.push(turn);
      this.dirtyHeads.add(id);
      this.changed();
    }
    this.log.info(`chat: turn ${id} sent (${text.length} chars, ${count(images.length, "image")})`);
    const prompt = this.prompt({ id, text, images });
    try {
      const { stopReason } = await this.agent.request<{ stopReason: string }>("session/prompt", { sessionId, prompt });
      if (run !== this.run) return;
      // The answer ends the last turn of the request: a message that joined it took it on.
      const last = this.sent ?? turn;
      if (stopReason === "max_tokens" || stopReason === "max_turn_requests") {
        this.push(last, { kind: "notice", tone: "warning", text: "The autonomous run reached its limit." });
      }
      this.end(last, stopReason === "cancelled" ? "cancelled" : "done", undefined, stopReason);
    } catch (error) {
      if (run !== this.run) return;
      // yusAgent : échec d'authentification, message fixe avec le chemin du binaire.
      this.end(this.sent ?? turn, "failed", isAuthFailure(error) ? authText(this.agent.binaryPath) : errorText(error));
    }
  }

  /** What goes to the agent: the text (a skill by the agent's name), its `@` links, its images. */
  private prompt({ id, text, images }: Pick<Queued, "id" | "text" | "images">): unknown[] {
    // yusAgent : blocs texte seulement (§5.4) tant que `features().attachments` est faux.
    const links = features().attachments ? this.files.links(text) : [];
    if (links.length > 0) this.log.info(`chat: turn ${id} links ${links.map((link) => link.name).join(", ")}`);
    const skill = this.skillCommand(text);
    if (skill !== text) this.log.info(`chat: turn ${id} runs ${skill.split(/\s/, 1)[0]}`);
    return [
      { type: "text", text: skill },
      ...links,
      ...(features().attachments ? images.map(({ mimeType, data }) => ({ type: "image", mimeType, data })) : []),
    ];
  }

  /** The agent goes back to before `edited`'s prompt; why not, when it cannot. */
  private async rewind(sessionId: string, edited: number): Promise<string | undefined> {
    const turn = this.entries.find((entry) => entry.id === edited);
    if (turn?.kind !== "turn" || turn.rewindTo === undefined) {
      return "it is no longer in the agent's conversation.";
    }
    try {
      await this.agent.request("_unharnessed/session/rewind", { sessionId, to: turn.rewindTo });
    } catch (error) {
      this.log.warn(`chat: going back to before turn ${edited} failed: ${errorText(error)}`);
      return errorText(error);
    }
    this.log.info(`chat: back to before turn ${edited}`);
    return undefined;
  }

  private end(turn: Turn, state: Turn["state"], error?: string, reason?: string): void {
    if (this.thinking?.turn === turn) this.settle();
    // Its compaction never said how it ended.
    if (this.compacting?.turn === turn) this.unmark();
    if (turn.state === "running") {
      turn.state = state;
      if (state === "done") this.worked(turn);
      if (error) turn.error = error;
      this.dirtyHeads.add(turn.id);
      const why = reason ? ` (${reason})` : "";
      // A replayed turn: the load says it once for all.
      if (!this.loading) {
        this.log.info(`chat: turn ${turn.id} ${state}${why}${error ? `: ${error}` : ""}; ${summary(turn.items)}`);
      }
    }
    if (this.sent === turn) this.sent = undefined;
    if (this.live === turn) this.live = undefined;
    this.streaming = undefined;
    // The turn is over: the agent let go of what it did not take.
    if (!this.busy) this.requeue();
    this.next();
    this.changed();
  }

  /** How long a turn that began here worked, once done. */
  private worked(turn: Turn): void {
    const began = this.began.get(turn);
    if (began !== undefined) turn.ms = Date.now() - began;
  }

  /**
   * The next waiting message, once nothing runs: a goal command from the
   * buttons first, paused or not; then the first in line, unless the queue
   * is paused or waits at a message being changed.
   */
  private next(): void {
    if (this.paused && this.queue.length === 0 && this.steering.length === 0) this.paused = false;
    this.changed();
    if (this.busy || !this.session.sessionId) return;
    const quiet = this.queue.findIndex((message) => message.quiet);
    const at = quiet >= 0 ? quiet : this.paused || this.queue[0]?.held ? -1 : 0;
    const [message] = at >= 0 ? this.queue.splice(at, 1) : [];
    if (message) void this.start(message);
  }

  private onUpdate(params: unknown): void {
    const { sessionId, update } = params as { sessionId: string; update: Update };
    if (sessionId !== this.session.updatesFrom) return;
    // yusAgent : notre `_meta` (espace `ai.primeintellect.prime-agent`), noms d'Unharnessed.
    const meta = yaMeta<Meta>(update);
    if (update.sessionUpdate === "available_commands_update") {
      this.setCommands(update.availableCommands ?? []);
      return;
    }
    if (meta.usage) {
      if (meta.usage.compacting && !this.usage?.compacting) this.mark();
      this.usage = meta.usage;
      this.usageChanged = true;
      this.log.info(`chat: ${usageText(meta.usage)}`);
      this.changed();
      return;
    }
    if (meta.goal) {
      const { status, objective, createdAt, updatedAt, lastReason } = meta.goal;
      this.showGoal(status === "idle" || !objective ? null : { status: status as Goal["status"], objective, createdAt, updatedAt, lastReason });
    }
    if (meta.turnStart) {
      this.begin(meta.promptTurnId ?? 0, meta.turnStart);
      return;
    }
    const turn = this.turns.get(meta.promptTurnId ?? 0) ?? this.live;
    if (!turn) return;
    switch (update.sessionUpdate) {
      case "user_message_chunk":
        this.prompted(turn, update.content);
        break;
      case "agent_thought_chunk":
      case "agent_message_chunk": {
        const text = !Array.isArray(update.content) ? (update.content?.text ?? "") : "";
        const kind = update.sessionUpdate === "agent_thought_chunk" ? "thought" : "text";
        this.stream(turn, kind, `${kind}:${update.messageId}`, text);
        break;
      }
      case "tool_call": {
        const tool: Tool = {
          kind: "tool",
          id: update.toolCallId ?? "",
          title: update.title ?? "Tool",
          // yusAgent (PR 3, défaut 3) : en manuel, le pont l'annonce `pending` jusqu'à l'accord.
          status: update.status === "pending" ? "pending" : "running",
          input: toolInput(update.rawInput),
        };
        if (tool.title === "Python cell") {
          tool.cell = readCell(tool.input);
          for (const path of tool.cell.files) {
            const dir = awayDir(path, this.files.cwd, homedir());
            if (dir !== undefined) tool.cell.away = { ...tool.cell.away, [path]: dir };
          }
          if (!this.loading) this.log.info(`chat: turn ${turn.id} cell: ${cellText(tool.cell)}`);
        }
        this.push(turn, tool);
        break;
      }
      case "tool_call_update":
        this.toolDone(turn, update, meta);
        break;
      case "session_info_update": {
        // yusAgent (ticket 31) : la notice d'une annonce, une ligne du tour au-dessus de son message.
        const lines = noticeLines(update._meta);
        if (lines) {
          turn.notices = [...(turn.notices ?? []), ...lines];
          this.dirtyHeads.add(turn.id);
          if (!this.loading) this.log.info(`chat: turn ${turn.id} notice: ${lines.join(" ")}`);
          this.changed();
          break;
        }
        this.info(turn, meta);
        break;
      }
    }
  }

  /** What started a turn comes first in it: our prompt, or a wake. */
  private begin(agentTurn: number, start: NonNullable<Meta["turnStart"]>): void {
    // A turn of the conversation the agent replays: its prompt follows.
    if (this.loading) {
      const turn: Turn = { kind: "turn", id: this.nextId++, wake: start.wake, state: "running", items: [] };
      if (start.rewindTo !== undefined) turn.rewindTo = start.rewindTo;
      this.turns.set(agentTurn, turn);
      this.entries.push(turn);
      return;
    }
    if (start.origin === "steer") {
      this.joined(agentTurn, start);
      return;
    }
    if (start.origin === "prompt" && this.sent) {
      this.turns.set(agentTurn, this.sent);
      this.live = this.sent;
      // It may have waited for a turn the agent started.
      this.began.set(this.sent, Date.now());
      if (start.rewindTo !== undefined) {
        this.sent.rewindTo = start.rewindTo;
        this.dirtyHeads.add(this.sent.id);
        this.changed();
      }
      this.steerAll();
      return;
    }
    const turn: Turn = { kind: "turn", id: this.nextId++, wake: start.wake, state: "running", items: [] };
    this.turns.set(agentTurn, turn);
    this.live = turn;
    this.began.set(turn, Date.now());
    // Our prompt waits for it: the agent's turn shows above it.
    const at = this.sent ? this.entries.indexOf(this.sent) : -1;
    if (at >= 0) {
      this.entries.splice(at, 0, turn);
      this.post({ type: "chat", chat: this.current });
    } else {
      this.entries.push(turn);
      this.dirtyHeads.add(turn.id);
    }
    this.log.info(`chat: turn ${turn.id} started by the agent (${start.wake ? WAKES[start.wake] : start.origin})`);
    this.steerAll();
    this.changed();
  }

  /** A message sent into the running turn joined it: what follows is its own turn, under it. */
  private joined(agentTurn: number, start: NonNullable<Meta["turnStart"]>): void {
    const message = this.steering.shift();
    if (message) this.steered.delete(message);
    const before = this.turns.get(agentTurn) ?? this.live;
    const turn: Turn = { kind: "turn", id: message?.id ?? this.nextId++, state: "running", items: [] };
    if (message) {
      turn.prompt = { text: message.text, images: message.images.map(thumbOf) };
      this.images.set(turn.id, message.images);
    }
    if (start.rewindTo !== undefined) turn.rewindTo = start.rewindTo;
    if (before?.state === "running") {
      if (this.thinking?.turn === before) this.settle();
      before.state = "done";
      this.worked(before);
      this.dirtyHeads.add(before.id);
      this.log.info(`chat: turn ${before.id} done (joined); ${summary(before.items)}`);
    }
    if (this.sent === before) this.sent = turn;
    this.turns.set(agentTurn, turn);
    this.live = turn;
    this.began.set(turn, Date.now());
    this.streaming = undefined;
    this.entries.push(turn);
    this.dirtyHeads.add(turn.id);
    this.log.info(`chat: turn ${turn.id} joined the running turn`);
    this.changed();
  }

  /** A block of a replayed prompt: its text, or one of its images. */
  private prompted(turn: Turn, block: Update["content"]): void {
    if (!block || Array.isArray(block)) return;
    const prompt = (turn.prompt ??= { text: "", images: [] });
    if (block.type === "image" && block.data && block.mimeType) {
      const image = { data: block.data, mimeType: block.mimeType, thumb: `data:${block.mimeType};base64,${block.data}`, name: "image" };
      prompt.images.push(thumbOf(image));
      this.images.set(turn.id, [...(this.images.get(turn.id) ?? []), image]);
    } else if (block.text) {
      const text = withoutLinks(block.text);
      const links = block.text.split("\n").length - text.split("\n").length;
      if (links > 0) this.log.info(`chat: turn ${turn.id} replayed without its ${count(links, "link")}`);
      prompt.text = prompt.text ? `${prompt.text}\n${text}` : text;
    }
    this.dirtyHeads.add(turn.id);
  }

  private stream(turn: Turn, kind: "thought" | "text", key: string, text: string): void {
    if (!text) return;
    const last = turn.items.at(-1);
    if (last && this.streaming?.item === last && this.streaming.key === key && last.kind === kind) {
      last.text += text;
      this.dirtyItem(turn, turn.items.length - 1);
    } else {
      // Reasoning the agent did not say opened: timed from its first words.
      const item: TurnItem = kind === "thought" ? { kind, text, started: Date.now() } : { kind, text };
      this.push(turn, item);
      this.streaming = { item, key };
      // A replayed reasoning is not timed: how long it took was not kept.
      if (kind === "thought" && !this.loading) this.thinking = { turn, at: turn.items.length - 1 };
    }
    this.changed();
  }

  /** A reasoning block opens: it shows at once, before its words (if they ever come). */
  private think(turn: Turn, messageId: string): void {
    const item: TurnItem = { kind: "thought", text: "", started: Date.now() };
    this.push(turn, item);
    this.streaming = { item, key: `thought:${messageId}` };
    this.thinking = { turn, at: turn.items.length - 1 };
  }

  /** The reasoning under way is over: how long it took. */
  private settle(): void {
    const thinking = this.thinking;
    this.thinking = undefined;
    const item = thinking?.turn.items[thinking.at];
    if (!thinking || item?.kind !== "thought" || item.ms !== undefined) return;
    item.ms = Date.now() - item.started;
    this.dirtyItem(thinking.turn, thinking.at);
    const took = (item.ms / 1000).toFixed(1);
    this.log.info(`chat: turn ${thinking.turn.id} thought for ${took} s (${item.text.length} chars)`);
    this.changed();
  }

  private push(turn: Turn, item: TurnItem): void {
    // Whatever comes next, the reasoning before it is over.
    this.settle();
    turn.items.push(item);
    this.streaming = undefined;
    this.dirtyItem(turn, turn.items.length - 1);
    this.changed();
  }

  /** Said once per turn: a `/goal` command's answer and the goal's change say the same. */
  private notice(turn: Turn, tone: "info" | "warning" | "error", text: string): void {
    if (turn.items.some((item) => item.kind === "notice" && item.text === text)) return;
    this.push(turn, { kind: "notice", tone, text });
  }

  private toolDone(turn: Turn, update: Update, meta: Meta): void {
    const at = turn.items.findIndex((item) => item.kind === "tool" && item.id === update.toolCallId);
    if (at < 0) return;
    const tool = turn.items[at] as Tool;
    // yusAgent (PR 3, défaut 3) : l'accord donné, la cellule tourne.
    if (update.status === "in_progress" && tool.status === "pending") tool.status = "running";
    if (update.status === "completed") tool.status = "done";
    if (update.status === "failed") tool.status = "failed";
    if (Array.isArray(update.content)) {
      // yusAgent (PR 3) : les contenus `diff` (un par `edit()`) ne sont pas de la sortie.
      const text = update.content
        .filter((block) => (block as { type?: string }).type !== "diff")
        .map((block) => block.content?.text ?? "")
        .join("\n");
      const cut = text.length - OUTPUT_LIMIT;
      tool.output = cut > 0 ? `${text.slice(0, OUTPUT_LIMIT)}\n… ${cut} more characters` : text;
      if (cut > 0) this.wholeOutput.set(tool, text);
    }
    if (meta.ipython?.attachments?.length) tool.images = meta.ipython.attachments.length;
    // A cell that raised ran to its end for the agent (its traceback is its result); here it failed.
    if (meta.ipython?.errorEname) {
      tool.error = meta.ipython.errorEname;
      tool.status = "failed";
    }
    this.yaCellData(tool, update);
    if (tool.cell && !this.loading) this.log.info(`chat: turn ${turn.id} cell ${tool.status}${tool.error ? `: ${tool.error}` : ""}`);
    this.dirtyItem(turn, at);
    this.changed();
  }

  /**
   * yusAgent (PR 3, C5 à C7) : `bash()` réels, vrais diffs et refus, lus des données du pont
   * (`src/ya/cells.ts`) ; la carte les montre à la place de ce que le code laissait deviner.
   */
  private yaCellData(tool: Tool, update: Update): void {
    const runs = bashRuns(update._meta);
    if (runs) {
      tool.runs = runs;
      if (tool.title === "Python cell") tool.cell = cellOfRuns(tool.cell, runs);
    }
    const all = fragments(update.content);
    const edits = fileEdits(all, this.files.cwd);
    if (edits.length > 0) {
      tool.edits = edits;
      if (tool.title === "Python cell") tool.cell = cellOfEdits(tool.cell, edits);
      if (!this.loading) void this.yaRecordReverts(tool, all);
    }
    if (tool.status === "failed" && isDeclined(tool.output)) tool.declined = true;
  }

  /** yusAgent (PR 3, étape 4) : l'après et l'avant de chaque fichier de la cellule, pour « Rejeter ». */
  private async yaRecordReverts(tool: Tool, all: Fragment[]): Promise<void> {
    const edits = tool.edits ?? [];
    for (const edit of edits) edit.revert = { state: "checking" };
    await this.yaReverts.record(tool.id, this.session.sessionId, edits, all);
    for (const edit of edits) {
      const status = this.yaReverts.status(tool.id, edit.path);
      if (status?.state === "blocked") this.log.info(`chat: revert of ${edit.shown} unavailable: ${status.reason}`);
    }
    this.yaRefreshTool(tool);
  }

  /** yusAgent (PR 3, étape 4) : « Rejeter » la modification de `path` par la cellule `toolId`. */
  async yaRevert(toolId: string, path: string): Promise<void> {
    const status = await this.yaReverts.revert(toolId, path, { busy: this.busy, session: this.session.sessionId });
    const why = status?.state === "ready" ? status.refused : status?.state === "blocked" ? status.reason : undefined;
    this.log.info(`chat: revert ${path} (${toolId}): ${status?.state ?? "unknown"}${why ? `: ${why}` : ""}`);
    const tool = this.yaFindTool(toolId);
    if (tool) this.yaRefreshTool(tool);
  }

  /** yusAgent (PR 3, étape 4) : l'avant et l'après de la cellule dans `vscode.diff` (C9, C10). */
  async yaOpenDiff(toolId: string, path: string): Promise<void> {
    if (!(await this.yaReverts.openDiff(toolId, path))) this.log.info(`chat: no diff to open for ${path} (${toolId})`);
  }

  private yaFindTool(toolId: string): Tool | undefined {
    for (const entry of this.entries) {
      if (entry.kind !== "turn") continue;
      const tool = entry.items.find((item): item is Tool => item.kind === "tool" && item.id === toolId && item.edits !== undefined);
      if (tool) return tool;
    }
    return undefined;
  }

  /** Le statut de « Rejeter » de chaque fichier de la cellule, vers le panneau. */
  private yaRefreshTool(tool: Tool): void {
    for (const edit of tool.edits ?? []) edit.revert = this.yaReverts.status(tool.id, edit.path) ?? edit.revert;
    for (const entry of this.entries) {
      if (entry.kind !== "turn") continue;
      const at = entry.items.indexOf(tool);
      if (at < 0) continue;
      this.dirtyItem(entry as Turn, at);
      this.changed();
      return;
    }
  }

  private yaRefreshAllTools(): void {
    for (const entry of this.entries) {
      if (entry.kind !== "turn") continue;
      for (const item of entry.items) if (item.kind === "tool" && item.edits) this.yaRefreshTool(item);
    }
  }

  /** Reasoning opening and closing, a command's answer, a cut answer, a compaction, a goal's change; the end of a turn the agent started or replayed. */
  private info(turn: Turn, meta: Meta): void {
    if (meta.turnEnd) {
      this.end(turn, meta.turnEnd.state, meta.turnEnd.error);
      return;
    }
    if (meta.thinking?.state === "start") this.think(turn, meta.thinking.messageId);
    if (meta.thinking?.state === "end") this.settle();
    if (meta.outputLimit) {
      this.push(turn, { kind: "notice", tone: "info", text: "The answer hit the output limit; the agent went on." });
      if (!this.loading) this.log.info(`chat: turn ${turn.id} cut at the output limit`);
    }
    if (meta.command) {
      const { severity, text } = meta.command;
      const tone = severity === "error" || severity === "warning" ? severity : "info";
      this.notice(turn, tone, text);
    }
    if (meta.compaction) this.compacted(turn, meta.compaction);
    if (meta.goal && meta.goal.status !== this.goal) {
      this.goal = meta.goal.status;
      const objective = meta.goal.objective ? `: ${meta.goal.objective}` : "";
      // A cleared goal: `/goal clear` says it.
      if (meta.goal.status !== "idle") this.notice(turn, "info", `Goal ${meta.goal.status}${objective}`);
    }
    // Our prompt ends with its answer; a turn the agent started, here.
    if (meta.phase === "responseBoundary" && turn !== this.sent) {
      if (meta.outcome === "error") this.end(turn, "failed", "The turn failed.");
      else this.end(turn, "done");
    }
  }

  /** A compaction starts: it shows in the running turn, at its end. */
  private mark(): void {
    const turn = this.live ?? this.sent;
    if (!turn || this.compacting) return;
    const auto = !isCompact(turn);
    this.push(turn, { kind: "compaction", state: "running", auto });
    this.compacting = { turn, at: turn.items.length - 1 };
    this.log.info(`chat: turn ${turn.id} compacting${auto ? " automatically" : ""}`);
  }

  /**
   * A compaction ended: one that ran tells how much it summarized, and
   * holds the summary; one that did not (skipped, failed, stopped) leaves.
   * A replayed one comes done.
   */
  private compacted(turn: Turn, { tokensBefore, summary }: NonNullable<Meta["compaction"]>): void {
    if (tokensBefore === undefined) {
      this.unmark();
      return;
    }
    const mark = this.compacting;
    this.compacting = undefined;
    const running = mark?.turn.items[mark.at];
    const auto = running?.kind === "compaction" ? running.auto : !isCompact(turn);
    const item: Compaction = { kind: "compaction", state: "done", auto, tokensBefore };
    if (summary) item.summary = summary;
    if (mark && running?.kind === "compaction") {
      mark.turn.items[mark.at] = item;
      this.dirtyItem(mark.turn, mark.at);
      this.changed();
    } else {
      this.push(turn, item);
    }
    if (this.loading) return;
    const where = mark?.turn ?? turn;
    const kept = summary ? `, a summary of ${count(summary.length, "char")}` : ", no summary";
    this.log.info(`chat: turn ${where.id} compacted (${tokensBefore.toLocaleString("en-US")} tokens summarized${kept})`);
  }

  /** The running compaction's mark leaves: it did not run. */
  private unmark(): void {
    const mark = this.compacting;
    this.compacting = undefined;
    if (mark?.turn.items[mark.at]?.kind !== "compaction") return;
    mark.turn.items.splice(mark.at, 1);
    this.streaming = undefined;
    this.log.info(`chat: turn ${mark.turn.id} compaction did not run`);
    // What followed it moved up: the panel gets the conversation whole.
    this.post({ type: "chat", chat: this.current });
  }

  /** The agent's commands, prompts and skills; its built-in skills are its own business. */
  private setCommands(commands: NonNullable<Update["availableCommands"]>): void {
    const kinds = ["command", "prompt", "skill"] as const;
    const builtin = commands.filter(({ _meta }) => _meta?.unharnessed?.source === "builtin");
    const agent = commands
      .filter((command) => !builtin.includes(command))
      .map(({ name, description, input, _meta }): ChatCommand => {
        const kind = kinds.find((k) => k === _meta?.unharnessed?.kind) ?? "command";
        return { name, description, hint: input?.hint || undefined, kind };
      });
    this.commands = [...PANEL_COMMANDS, ...agent.filter((command) => command.name !== "new")];
    this.commandsChanged = true;
    const left = builtin.length > 0 ? `; ${count(builtin.length, "built-in skill")} left out` : "";
    this.log.info(`chat: ${agent.length} commands: ${agent.map((command) => command.name).join(", ")}${left}`);
    this.changed();
  }

  private dirtyItem(turn: Turn, at: number): void {
    const items = this.dirtyItems.get(turn.id) ?? new Set();
    this.dirtyItems.set(turn.id, items.add(at));
  }

  /** What the panel shows waiting: what was sent into the turn first, then the queue. */
  private queued(): QueuedMessage[] {
    const shown = ({ id, text, images, held }: Queued, steering: boolean): QueuedMessage => ({
      id,
      text,
      images: images.map(thumbOf),
      state: steering ? "steering" : held ? "held" : "waiting",
    });
    return [
      ...this.steering.map((message) => shown(message, true)),
      ...this.queue.filter((message) => !message.quiet).map((message) => shown(message, false)),
    ];
  }

  private showGoal(goal: Goal | null): void {
    if (goal?.status !== this.goalState?.status) this.log.info(`chat: goal ${goalText(goal)}`);
    this.goalState = goal;
    this.goalChanged = true;
    this.changed();
  }

  /** What changed goes to the panel together, a moment later; a conversation coming back, once whole. */
  private changed(): void {
    if (this.loading) return;
    this.timer ??= setTimeout(() => this.flush(), FLUSH_MS);
  }

  private flush(): void {
    this.timer = undefined;
    const byId = new Map(this.entries.map((entry) => [entry.id, entry]));
    const heads = [...this.dirtyHeads].flatMap((id): EntryHead[] => {
      const entry = byId.get(id);
      if (!entry) return [];
      if (entry.kind !== "turn") return [entry];
      const { items: _, ...head } = entry;
      return [head];
    });
    const items: [number, number, TurnItem][] = [];
    for (const [id, indexes] of this.dirtyItems) {
      const entry = byId.get(id);
      if (entry?.kind !== "turn") continue;
      for (const at of indexes) items.push([id, at, entry.items[at]]);
    }
    const patch: ChatPatch = { heads, items, busy: this.busy, queue: this.queued(), paused: this.paused };
    if (this.commandsChanged) patch.commands = this.commands;
    if (this.usageChanged) patch.usage = this.usage;
    if (this.goalChanged) patch.goal = this.goalState;
    this.post({ type: "chatPatch", patch });
    this.dirtyHeads.clear();
    this.dirtyItems.clear();
    this.commandsChanged = false;
    this.usageChanged = false;
    this.goalChanged = false;
  }

  private post(message: ChatMessage): void {
    // A full post carries what the next patch would.
    if (message.type === "chat") {
      this.dirtyHeads.clear();
      this.dirtyItems.clear();
      this.commandsChanged = false;
      this.usageChanged = false;
      this.goalChanged = false;
    }
    for (const listener of this.listeners) listener(message);
  }
}

/** For the log: `active: <objective>`, `paused, 12 s in`, `gone`. */
function goalText(goal: Goal | null): string {
  if (!goal) return "gone";
  if (goal.status === "active") return `active: ${goal.objective}`;
  const { createdAt, updatedAt } = goal;
  return createdAt && updatedAt ? `${goal.status}, ${Math.round((updatedAt - createdAt) / 1000)} s in` : goal.status;
}

function thumbOf({ thumb, name }: Attachment): Thumb {
  return { thumb, name };
}

/** The turn is `/compact`. */
function isCompact(turn: Turn): boolean {
  return /^\/compact(?=\s|$)/.test(turn.prompt?.text.trim() ?? "");
}

/** A Python cell's code; another tool's input as JSON. */
function toolInput(raw: unknown): string {
  if (raw && typeof raw === "object" && "code" in raw && typeof raw.code === "string") return raw.code;
  return raw === undefined ? "" : JSON.stringify(raw, null, 2);
}

/** For the log: `edit src/a.ts +2 -1`, `read /etc/hosts (in /etc/)`, `shell \`npm test\``. */
function cellText({ kind, files, away, command, diff }: Cell): string {
  const changed = !diff ? "" : kind === "write" ? `+${diff.added}` : `+${diff.added} -${diff.removed}`;
  const named = files.map((path) => (away?.[path] ? `${path} (in ${away[path]})` : path));
  return [kind, ...named, command === undefined ? "" : `\`${command}\``, changed].filter(Boolean).join(" ");
}

/** `1 image`, `2 images`. */
function count(n: number, what: string): string {
  return `${n} ${what}${n === 1 ? "" : "s"}`;
}

/** For the log: `usage 12,345 of 200,000 tokens (compacts at 160,000), $0.0123, cache 96% saved $0.0456`. */
function usageText({ tokens, window, compactAt, cost, cacheReusable, cacheReused, cacheSaved, compacting }: Usage): string {
  const n = (count: number) => count.toLocaleString("en-US");
  const context = tokens === undefined ? "not measured" : `${n(tokens)}`;
  const compacts = compactAt === undefined ? "" : ` (compacts at ${n(compactAt)})`;
  const cache = cacheReusable > 0 ? `, cache ${Math.round((cacheReused / cacheReusable) * 100)}% saved $${cacheSaved.toFixed(4)}` : "";
  return `usage ${context} of ${n(window)} tokens${compacts}, $${cost.toFixed(4)}${cache}${compacting ? ", compacting" : ""}`;
}

/** For the log: `2 texts, 1 thought, 1 tool`. */
function summary(items: TurnItem[]): string {
  const counts = new Map<string, number>();
  for (const item of items) counts.set(item.kind, (counts.get(item.kind) ?? 0) + 1);
  const parts = [...counts].map(([kind, n]) => count(n, kind));
  return parts.length ? parts.join(", ") : "nothing streamed";
}

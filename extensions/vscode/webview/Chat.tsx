import {
  Fragment,
  memo,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import type { Cell, ChatEntry, ChatPatch, ChatState, Thumb, TurnItem } from "../src/protocol";
import { FileIcon } from "./FileIcon";
import {
  IconArrowDown,
  IconChevron,
  IconCompact,
  IconExternal,
  IconEye,
  IconFilePlus,
  IconFlag,
  IconImage,
  IconPencil,
  IconPython,
  IconRestart,
  IconSearch,
  IconTerminal,
  IconVariables,
} from "./icons";
import { ThinkingOrb } from "./ya-orb"; // yusAgent : bouchon du paquet thinking-orbs
import { BashRuns, exitMeta, FileEdits } from "./ya-cells"; // yusAgent (PR 3)
import { Markdown } from "./Markdown";
import { MentionText } from "./mentions";
import { count } from "./Usage";
import { post } from "./vscode";

type Turn = Extract<ChatEntry, { kind: "turn" }>;
type Tool = Extract<TurnItem, { kind: "tool" }>;
type Compaction = Extract<TurnItem, { kind: "compaction" }>;

/** How close to the end the view must be to follow what streams in. */
const FOLLOW_PX = 48;
/** The reasoning's window: it grows to this, then scrolls; its edges fade over this (two lines). */
const THOUGHT_MAX_PX = 80;
const THOUGHT_FADE_PX = 40;
/** How long reasoning that just ended stays open before it folds. */
const FOLD_BEAT_MS = 360;
/** How long the answer may pause before the turn says again that it works. */
const QUIET_MS = 900;
/** How long an orb takes to leave once its state is over (styles.css, .orb). */
const ORB_OUT_MS = 320;
/** How long a compaction takes to land: its letters light up, its lines grow (styles.css, .compaction). */
const LANDING_MS = 1800;
/** How soon after the kernel came back its line still lands as it shows: not when the panel opens again later. */
const KERNEL_LANDS_MS = 1500;

const WAKES: Record<NonNullable<Turn["wake"]>, string> = {
  bash: "a background command ended",
  quota: "the usage limit reset",
  goal: "the goal's pause ended",
  // yusAgent (ticket 25, E2) : réveil planifié du démon.
  heartbeat: "a scheduled heartbeat came",
};

export const EMPTY_CHAT: ChatState = {
  entries: [],
  busy: false,
  queue: [],
  paused: false,
  commands: [],
  usage: null,
  goal: null,
};

/** What changed, onto the conversation; what did not change keeps its identity (and its render). */
export function applyPatch(chat: ChatState, patch: ChatPatch): ChatState {
  let entries = chat.entries;
  if (patch.heads.length > 0 || patch.items.length > 0) {
    entries = [...entries];
    const index = new Map(entries.map((entry, at) => [entry.id, at]));
    for (const head of patch.heads) {
      const at = index.get(head.id);
      const before = at === undefined ? undefined : entries[at];
      const entry: ChatEntry =
        head.kind === "turn" ? { ...head, items: before?.kind === "turn" ? before.items : [] } : head;
      if (at === undefined) index.set(head.id, entries.push(entry) - 1);
      else entries[at] = entry;
    }
    const byEntry = new Map<number, [number, TurnItem][]>();
    for (const [id, at, item] of patch.items) byEntry.set(id, [...(byEntry.get(id) ?? []), [at, item]]);
    for (const [id, changes] of byEntry) {
      const at = index.get(id);
      const entry = at === undefined ? undefined : entries[at];
      if (at === undefined || entry?.kind !== "turn") continue;
      const items = [...entry.items];
      for (const [itemAt, item] of changes) items[itemAt] = item;
      entries[at] = { ...entry, items };
    }
  }
  return {
    entries,
    busy: patch.busy,
    queue: patch.queue,
    paused: patch.paused,
    commands: patch.commands ?? chat.commands,
    usage: patch.usage === undefined ? chat.usage : patch.usage,
    goal: patch.goal === undefined ? chat.goal : patch.goal,
  };
}

/** A sent prompt goes back to the message box, to be changed and sent again. */
const edit = (id: number) => post({ type: "edit", id });

/**
 * The conversation; it follows what streams in while it is scrolled to the
 * end. While turn `editing`'s prompt is being changed, what sending it
 * would drop fades: its answer and the turns after it.
 */
export function Transcript({
  chat,
  editing,
  children,
}: {
  chat: ChatState;
  editing: number | null;
  children?: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const follow = useRef(true);
  const [away, setAway] = useState(false);
  const last = chat.entries.at(-1);

  // A message the user just sent brings the view back to the end.
  const sent = last?.kind === "turn" && last.prompt ? last.id : undefined;
  useLayoutEffect(() => {
    if (sent !== undefined) follow.current = true;
  }, [sent]);

  // What grows between two patches (the reasoning's window opening, a fold
  // closing) is followed too: each entry is watched for its size.
  const sizes = useRef<ResizeObserver>(undefined);
  useLayoutEffect(() => {
    const view = ref.current;
    if (!view) return;
    if (follow.current) view.scrollTop = view.scrollHeight;
    sizes.current ??= new ResizeObserver(() => {
      if (follow.current && ref.current) ref.current.scrollTop = ref.current.scrollHeight;
    });
    for (const entry of view.children) sizes.current.observe(entry);
    // Its own room at the end changing (the kernel's pills coming over it) too.
    sizes.current.observe(view);
  });
  useEffect(() => () => sizes.current?.disconnect(), []);

  const onScroll = () => {
    const view = ref.current;
    if (!view) return;
    follow.current = view.scrollHeight - view.scrollTop - view.clientHeight < FOLLOW_PX;
    setAway(!follow.current);
  };

  const toEnd = () => {
    follow.current = true;
    setAway(false);
    if (ref.current) ref.current.scrollTop = ref.current.scrollHeight;
  };

  const cut = editing === null ? -1 : chat.entries.findIndex((entry) => entry.id === editing);
  const dim = (at: number) => (cut < 0 || at < cut ? undefined : at === cut ? "answer" : "all");
  return (
    <div className="transcript-shell">
      <div className="transcript" ref={ref} onScroll={onScroll}>
        {children}
        {chat.entries.map((entry, at) =>
          entry.kind === "restart" ? (
            <Restart key={entry.id} dimmed={dim(at) !== undefined} />
          ) : entry.kind === "kernel" ? (
            <KernelLine key={entry.id} restored={entry.restored} at={entry.at} dimmed={dim(at) !== undefined} />
          ) : (
            <TurnView
              key={entry.id}
              turn={entry}
              editable={!chat.busy && entry.rewindTo !== undefined && at !== cut}
              dim={dim(at)}
            />
          ),
        )}
      </div>
      {away && (
        <button className="icon-button to-end" type="button" aria-label="Latest" data-tip="Latest" onClick={toEnd}>
          <IconArrowDown />
        </button>
      )}
    </div>
  );
}

function Restart({ dimmed }: { dimmed: boolean }) {
  return (
    <div className={dimmed ? "restart-line is-dimmed" : "restart-line"}>
      <IconRestart />
      The agent restarted: it no longer remembers the messages above.
    </div>
  );
}

/**
 * Where the kernel came back with the conversation's variables, or could
 * not. Landing here, a light comes on (styles.css, .kernel-line): the
 * braces draw themselves, their dot lights up, then the words.
 */
function KernelLine({ restored, at, dimmed }: { restored: boolean; at: number; dimmed: boolean }) {
  const [landing] = useState(() => Date.now() - at < KERNEL_LANDS_MS);
  const words = restored ? "Python kernel restored" : "The Python kernel could not be restored";
  const classes = ["restart-line kernel-line", !restored && "is-failed", landing && "is-landing", dimmed && "is-dimmed"];
  return (
    <div className={classes.filter(Boolean).join(" ")}>
      <IconVariables className="kernel-line-logo" />
      <span className="kernel-words">
        {[...words].map((letter, i) => (
          <span key={i} className="letter" style={{ "--i": i } as CSSProperties}>
            {letter}
          </span>
        ))}
      </span>
    </div>
  );
}

/** `dim`: the turn fades while an edit would drop it, all of it or its answer only. */
const TurnView = memo(function TurnView({
  turn,
  editable,
  dim,
}: {
  turn: Turn;
  editable: boolean;
  dim?: "answer" | "all";
}) {
  const running = turn.state === "running";
  // Reasoning, a running cell, a compaction and the answer as it streams show
  // their own life; otherwise (a pause: the model writing a cell, say) the turn says it works.
  const last = turn.items.at(-1);
  const writing = useWriting(running && last?.kind === "text" ? last.text : undefined);
  const busy =
    writing ||
    (last?.kind === "thought" && last.ms === undefined) ||
    (last?.kind === "tool" && last.status === "running") ||
    // yusAgent (PR 3, défaut 3) : une cellule en attente d'accord attend l'utilisateur, pas l'agent.
    (last?.kind === "tool" && last.status === "pending") ||
    (last?.kind === "compaction" && last.state === "running");
  const working = running && !busy;
  // What it did before its answer, then the answer, out of the fold.
  const answer = answerAt(turn);
  const split = answer ?? turn.items.length;
  const all = pieces(turn.items);
  const show = (piece: Piece) =>
    piece.kind === "item" ? (
      <Item
        key={piece.at}
        item={turn.items[piece.at]}
        running={running}
        streaming={running && piece.at === turn.items.length - 1}
      />
    ) : (
      <CallRun key={`run ${piece.start}`} items={turn.items} start={piece.start} end={piece.end} running={running} />
    );
  return (
    <section className={dim ? `turn is-dimmed-${dim}` : "turn"}>
      {/* yusAgent (ticket 31) : les annonces parties avec ce tour, au-dessus de son message. */}
      {turn.notices?.map((text, at) => (
        <p key={`notice ${at}`} className="ya-turn-notice" role="note">
          {text}
        </p>
      ))}
      {turn.prompt && (
        <UserMessage
          text={turn.prompt.text}
          images={turn.prompt.images}
          onEdit={editable ? () => edit(turn.id) : undefined}
        />
      )}
      {!turn.prompt && turn.wake && <p className="turn-wake">The agent went on by itself: {WAKES[turn.wake]}.</p>}
      {[
        split > 0 && (
          <WorkRun key="work" items={turn.items.slice(0, split)} folded={answer !== undefined} ms={turn.ms}>
            {all.filter((piece) => first(piece) < split).map(show)}
          </WorkRun>
        ),
        ...all.filter((piece) => first(piece) >= split).map(show),
      ]}
      {working && (
        <p className="working-line">
          <Orb on state="breathing" />
          <span className="working">Working</span>
        </p>
      )}
      {turn.state === "cancelled" && <p className="turn-end">Stopped</p>}
      {turn.state === "failed" && turn.error && <p className="error turn-error">{turn.error}</p>}
    </section>
  );
});

/** The user's message; `onEdit`: a click on it brings it back to the message box, unless the click ended a selection. */
function UserMessage({ text, images, onEdit }: { text: string; images: Thumb[]; onEdit?: () => void }) {
  // `/goal` reads as the goal's flag.
  const goal = /^\/goal(?=\s|$)/.exec(text);
  const editable = onEdit && {
    role: "button",
    tabIndex: 0,
    "aria-label": "Edit this message",
    onClick: () => window.getSelection()?.isCollapsed !== false && onEdit(),
    onKeyDown: (event: KeyboardEvent) => {
      if (event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault();
      onEdit();
    },
  };
  return (
    <div className={editable ? "user-message is-editable" : "user-message"} {...editable}>
      {images.length > 0 && (
        <div className="user-images">
          {images.map((image, at) => (
            <img key={at} src={image.thumb} alt={image.name} title={image.name} />
          ))}
        </div>
      )}
      {text && (
        <p className="user-text">
          {goal && (
            <span className="goal-word">
              <IconFlag />
              goal
            </span>
          )}
          <MentionText text={goal ? text.slice(goal[0].length) : text} />
        </p>
      )}
    </div>
  );
}

/** Whether `text` (the answer under way) changed within the last QUIET_MS. */
function useWriting(text: string | undefined): boolean {
  const [quiet, setQuiet] = useState(true);
  // Before the paint, so "Working" never flashes as a chunk lands.
  useLayoutEffect(() => {
    if (text === undefined) return;
    setQuiet(false);
    const timer = setTimeout(() => setQuiet(true), QUIET_MS);
    return () => clearTimeout(timer);
  }, [text]);
  return text !== undefined && !quiet;
}

const Item = memo(function Item({ item, running, streaming }: { item: TurnItem; running: boolean; streaming: boolean }) {
  switch (item.kind) {
    case "text":
      return <Markdown text={item.text} streaming={streaming} />;
    case "thought":
      return <Thought thought={item} live={running && item.ms === undefined} />;
    case "tool":
      return <ToolCall tool={item} live={running && item.status === "running"} waiting={running && item.status === "pending"} />;
    case "compaction":
      return <CompactionMark compaction={item} />;
    case "notice":
      return <p className={`notice is-${item.tone}`}>{item.text}</p>;
  }
});

/**
 * The model's reasoning. While it thinks, its words stream open in a window
 * that grows to THOUGHT_MAX_PX, then keeps the newest lines in view under a
 * fade; once it is over it folds into "Thought for 6s", which opens again
 * as a window to scroll.
 */
function Thought({ thought, live }: { thought: Extract<TurnItem, { kind: "thought" }>; live: boolean }) {
  const text = thought.text.trim();
  const [open, setOpen] = useState(false);
  // The reasoning that just ended stays open a beat before it folds.
  const [held, setHeld] = useState(false);
  const wasLive = useRef(live);
  useLayoutEffect(() => {
    const ended = wasLive.current && !live;
    wasLive.current = live;
    if (!ended) return;
    setHeld(true);
    const timer = setTimeout(() => setHeld(false), FOLD_BEAT_MS);
    return () => clearTimeout(timer);
  }, [live]);

  // The words' height, to size the window and keep the newest in view.
  const stream = useRef<HTMLDivElement>(null);
  const view = useRef<HTMLDivElement>(null);
  const [height, setHeight] = useState(0);
  const hasText = text !== "";
  useLayoutEffect(() => {
    const element = stream.current;
    if (!element) return;
    const measure = () => setHeight(element.offsetHeight);
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    measure();
    return () => observer.disconnect();
  }, [hasText]);
  const [fade, setFade] = useState({ top: false, bottom: true });

  const thinking = live || held;
  const expanded = hasText && (thinking || open);
  const scrolls = !thinking && open;
  const capped = height > THOUGHT_MAX_PX;
  // While it streams the newest words sit at the bottom, the oldest fading out at the top.
  const shift = !scrolls && capped ? THOUGHT_MAX_PX - height : 0;
  const top = scrolls ? fade.top : capped;
  const bottom = scrolls && fade.bottom;
  const mask = capped
    ? `linear-gradient(to bottom, transparent 0, #000 ${top ? THOUGHT_FADE_PX : 0}px, #000 calc(100% - ${bottom ? THOUGHT_FADE_PX : 0}px), transparent 100%)`
    : undefined;

  const toggle = () => {
    if (!open) {
      setFade({ top: false, bottom: true });
      if (view.current) view.current.scrollTop = 0;
    }
    setOpen(!open);
  };
  const onScroll = () => {
    const element = view.current;
    if (!element) return;
    setFade({
      top: element.scrollTop > 1,
      bottom: element.scrollTop + element.clientHeight < element.scrollHeight - 1,
    });
  };

  const opens = !thinking && hasText;
  return (
    <div className="thought">
      <button
        className={opens ? "thought-head" : "thought-head is-still"}
        type="button"
        data-no-press
        aria-expanded={expanded}
        onClick={opens ? toggle : undefined}
      >
        <span className="thought-slot">
          <Orb on={thinking} state="composing" />
          {!thinking && <IconChevron className="thought-chevron" />}
        </span>
        {thinking ? (
          <span className="thought-label">
            <span className="thought-verb is-live">Thinking</span>
            {live ? <Seconds since={thought.started} /> : thought.ms !== undefined && ` ${took(thought.ms)}`}
          </span>
        ) : (
          <span className="thought-label">
            <span className="thought-verb">Thought</span>
            {thought.ms !== undefined && ` for ${took(thought.ms)}`}
          </span>
        )}
      </button>
      {hasText && (
        <div className={expanded ? "thought-fold" : "thought-fold is-folded"}>
          <div className="thought-inner">
            <div
              ref={view}
              className={scrolls ? "thought-view is-scroll" : "thought-view"}
              style={{ height: Math.min(height, THOUGHT_MAX_PX), WebkitMaskImage: mask, maskImage: mask }}
              onScroll={scrolls ? onScroll : undefined}
            >
              <div ref={stream} className="thought-stream" style={{ transform: `translateY(${shift}px)` }}>
                <Markdown text={text} streaming={live} />
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

/**
 * The orb beside "Thinking" and "Working" (thinking-orbs), in the cyan of
 * live things, glowing (styles.css, .orb): in while it is `on`, out softly
 * once it is not.
 */
function Orb({ on, state }: { on: boolean; state: "composing" | "breathing" }) {
  const [shown, setShown] = useState(on);
  const ink = useInk();
  useEffect(() => {
    if (on) {
      setShown(true);
      return;
    }
    const timer = setTimeout(() => setShown(false), ORB_OUT_MS);
    return () => clearTimeout(timer);
  }, [on]);
  if (!shown) return null;
  return (
    <span className={on ? "orb" : "orb is-leaving"} aria-hidden="true">
      <ThinkingOrb state={state} size={20} theme={ink.light ? "light" : "dark"} color={ink.accent} />
    </span>
  );
}

/** The panel's live colour (the cyan) and whether VS Code's theme is light, following theme changes. */
function useInk(): { accent: string; light: boolean } {
  const [ink, setInk] = useState(readInk);
  useEffect(() => {
    const observer = new MutationObserver(() => setInk(readInk()));
    observer.observe(document.body, { attributes: true, attributeFilter: ["class"] });
    return () => observer.disconnect();
  }, []);
  return ink;
}

function readInk(): { accent: string; light: boolean } {
  const rgb = getComputedStyle(document.body).getPropertyValue("--uh-accent-rgb").trim().split(/\s+/);
  const classes = document.body.classList;
  return {
    accent: `rgb(${rgb.join(", ")})`,
    light: classes.contains("vscode-light") || classes.contains("vscode-high-contrast-light"),
  };
}

/** ` 3s` after `since`, counting each second; nothing in the first. */
function Seconds({ since }: { since: number }) {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    // The next whole second.
    const timer = setTimeout(() => setNow(Date.now()), 1000 - ((Date.now() - since) % 1000));
    return () => clearTimeout(timer);
  }, [now, since]);
  return now - since < 1000 ? null : ` ${took(now - since)}`;
}

/** `6s`, `1m 12s`; whole seconds, as the count while it runs showed them. */
function took(ms: number): string {
  const seconds = Math.max(1, Math.floor(ms / 1000));
  if (seconds < 60) return `${seconds}s`;
  const rest = seconds % 60;
  return `${Math.floor(seconds / 60)}m${rest ? ` ${rest}s` : ""}`;
}

/**
 * Where the conversation was compacted: what is above it now lives as a
 * summary. A pill, its words shining while it runs; once done, a grey line
 * on each side across the conversation. Landing here (not in a conversation
 * coming back), its words light up one after another and the lines grow out
 * of it. It opens on the summary.
 */
function CompactionMark({ compaction }: { compaction: Compaction }) {
  const { state, auto, tokensBefore, summary } = compaction;
  const done = state === "done";
  const [open, setOpen] = useState(false);
  // It lands once, done after it ran here; it comes done in a conversation coming back.
  const ran = useRef(!done);
  const [landing, setLanding] = useState(false);
  useLayoutEffect(() => {
    if (!done || !ran.current) return;
    ran.current = false;
    setLanding(true);
    const timer = setTimeout(() => setLanding(false), LANDING_MS);
    return () => clearTimeout(timer);
  }, [done]);

  if (!done) {
    return (
      <div className="compaction is-running">
        <div className="compaction-row">
          <span className="compaction-pill">
            <svg className="mini-orbit" viewBox="0 0 14 14" aria-hidden="true">
              <circle className="track" cx="7" cy="7" r="5.5" />
              <circle className="arc" cx="7" cy="7" r="5.5" strokeDasharray="9 35" />
            </svg>
            <span className="working">Compacting the conversation</span>
          </span>
        </div>
      </div>
    );
  }
  const label = auto ? "Compacted automatically" : "Compacted";
  const classes = ["compaction", "is-done", landing && "is-landing", open && "is-open"].filter(Boolean).join(" ");
  return (
    <div className={classes}>
      <div className="compaction-row">
        <span className="seam is-left" />
        <button
          className={summary ? "compaction-pill" : "compaction-pill is-still"}
          type="button"
          data-no-press
          aria-expanded={summary ? open : undefined}
          onClick={summary ? () => setOpen(!open) : undefined}
        >
          <IconCompact />
          <span className="compaction-label">
            {[...label].map((letter, at) => (
              <span key={at} className="letter" style={{ "--i": at } as CSSProperties}>
                {letter}
              </span>
            ))}
          </span>
          {tokensBefore !== undefined && (
            <span className="compaction-detail">· {count(tokensBefore)} tokens summarized</span>
          )}
          {summary && <IconChevron className="compaction-chevron" />}
        </button>
        <span className="seam is-right" />
      </div>
      {summary && (
        <div className="compaction-fold">
          <div>
            <div className="compaction-summary">
              <Markdown text={summary} />
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

/** A turn's items, each run of cells in a row gathered: anything else between two cells, a thought too, ends a run. */
type Piece = { kind: "item"; at: number } | { kind: "run"; start: number; end: number };

function first(piece: Piece): number {
  return piece.kind === "item" ? piece.at : piece.start;
}

/**
 * Where a done turn's answer begins: the text (and notices) it ended on.
 * What came before it folds, if a cell ran there; else nothing does.
 */
function answerAt(turn: Turn): number | undefined {
  if (turn.state !== "done") return undefined;
  let at = turn.items.length;
  while (at > 0 && (turn.items[at - 1].kind === "text" || turn.items[at - 1].kind === "notice")) at--;
  return turn.items.slice(0, at).some((item) => item.kind === "tool") ? at : undefined;
}

/**
 * What a turn did before its answer. While it runs it lies flat, as it
 * always did; once done it folds in one move into "Worked for 2m 14s ·
 * 6 cells +52 −11", which opens again. A conversation coming back shows it
 * folded already, without its time.
 */
function WorkRun({ items, folded, ms, children }: { items: TurnItem[]; folded: boolean; ms?: number; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const tools = items.filter((item): item is Tool => item.kind === "tool");
  const classes = ["work-group", folded ? open && "is-open" : "is-open is-flat"].filter(Boolean).join(" ");
  return (
    <div className={classes}>
      <div className="work-head-wrap" inert={!folded}>
        <div>
          <button className="work-head" type="button" data-no-press aria-expanded={open} onClick={() => setOpen(!open)}>
            <span className="work-verb">Worked</span>
            {ms !== undefined && <span>for {took(ms)}</span>}
            <span className="call-meta">
              <span className="sep">·</span>
              {tools.length} cell{tools.length === 1 ? "" : "s"}
              {tools.some(changes) && (
                <>
                  <span className="sep">·</span>
                  <Changed tools={tools} />
                </>
              )}
            </span>
            <IconChevron className="work-chevron" />
          </button>
          <span className="work-rule" />
        </div>
      </div>
      <div className="work-fold" inert={folded && !open}>
        <div>
          <div className="work-list">{children}</div>
        </div>
      </div>
    </div>
  );
}

function pieces(items: TurnItem[]): Piece[] {
  const out: Piece[] = [];
  items.forEach((item, at) => {
    const last = out.at(-1);
    if (last?.kind === "run" && item.kind === "tool") last.end = at + 1;
    else if (item.kind === "tool") out.push({ kind: "run", start: at, end: at + 1 });
    else out.push({ kind: "item", at });
  });
  return out;
}

/**
 * Cells in a row, `items[start..end]`. While they run, each shows on its
 * own; once the agent thinks or writes again (or the turn ends), two or
 * more fold into one line, "Ran 5 Python cells", in one move: the line
 * grows in as they fold away under it and slide to its indent. A cell
 * alone stays as it is. A conversation coming back shows them folded
 * already.
 */
function CallRun({ items, start, end, running }: { items: TurnItem[]; start: number; end: number; running: boolean }) {
  const [open, setOpen] = useState(false);
  const run = items.slice(start, end);
  const tools = run.filter((item): item is Tool => item.kind === "tool");
  const grouped = tools.length > 1 && (end < items.length || !running);
  const classes = ["call-group", grouped ? open && "is-open" : "is-open is-flat"].filter(Boolean).join(" ");
  return (
    <div className={classes}>
      <div className="call-head-wrap" inert={!grouped}>
        <div>
          <button className="call-head" type="button" data-no-press aria-expanded={open} onClick={() => setOpen(!open)}>
            <span className="call-lead">
              <IconPython className="call-icon" />
              <span className="call-verb">Ran</span>
            </span>
            <span className="call-target">
              {tools.length} Python cell{tools.length === 1 ? "" : "s"}
            </span>
            {tools.some(changes) && (
              <span className="call-meta">
                <Changed tools={tools} />
              </span>
            )}
            <IconChevron className="call-chevron" />
          </button>
        </div>
      </div>
      <div className="call-fold" inert={grouped && !open}>
        <div>
          <div className="call-list">
            {run.map((item, at) => (
              <Item
                key={start + at}
                item={item}
                running={running}
                streaming={running && start + at === items.length - 1}
              />
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}

/** What each kind of cell shows: its icon, then its verb while it runs, once it ran, when it failed. */
const KINDS: Record<Cell["kind"], { Icon: typeof IconPython; live: string; done: string; failed: string }> = {
  edit: { Icon: IconPencil, live: "Editing", done: "Edited", failed: "Edit failed" },
  write: { Icon: IconFilePlus, live: "Writing", done: "Wrote", failed: "Write failed" },
  view: { Icon: IconImage, live: "Viewing", done: "Viewed", failed: "View failed" },
  read: { Icon: IconEye, live: "Reading", done: "Read", failed: "Read failed" },
  search: { Icon: IconSearch, live: "Searching", done: "Searched", failed: "Search failed" },
  shell: { Icon: IconTerminal, live: "Running", done: "Ran", failed: "Failed" },
  python: { Icon: IconPython, live: "Running", done: "Ran", failed: "Failed" },
};

/** yusAgent (PR 3, défaut 3) : une cellule en attente d'accord, et une qui n'a jamais tourné. */
const YA_VERBS = { waiting: "Awaiting permission", unrun: "Not run", declined: "Declined" };

/**
 * A cell the agent runs, as one line on the page: what it does, the files
 * it touches or the command it runs (else its telling line of code), and
 * once it ran, the lines an edit changed or the exception it raised. A
 * click opens its code and what came out of it.
 */
function ToolCall({ tool, live, waiting }: { tool: Tool; live: boolean; waiting: boolean }) {
  const [open, setOpen] = useState(false);
  // Its insides are drawn on the first open, then kept for the fold to close over.
  const [drawn, setDrawn] = useState(false);
  const failed = tool.status === "failed";
  // yusAgent (PR 3, défaut 3) : en attente d'accord, sans état en cours ; jamais lancée si le tour
  // a fini sans que son appel sorte de `pending`.
  // yusAgent (PR 3, C5) : refusée, distincte d'une cellule en erreur.
  const state = waiting ? "waiting" : live ? "live" : tool.status === "pending" ? "unrun" : tool.declined ? "declined" : failed ? "failed" : "done";
  const { Icon, ...kindVerbs } = KINDS[tool.cell?.kind ?? "python"];
  const verbs = { ...kindVerbs, ...YA_VERBS };
  const files = tool.cell?.files ?? [];
  // yusAgent (PR 3, C7) : les vrais diffs se montrent même si la cellule a échoué après ses `edit()`.
  const diff = failed && !tool.edits ? undefined : tool.cell?.diff;
  const meta: ReactNode[] = [];
  if (state === "done" || state === "failed") {
    if (diff) meta.push(<Changed tools={[tool]} />);
    if (failed && tool.error) meta.push(tool.error);
    const exit = exitMeta(tool.runs); // yusAgent (PR 3, C6)
    if (exit) meta.push(exit);
    if (tool.images) meta.push(`${tool.images} image${tool.images > 1 ? "s" : ""} loaded`);
  }
  return (
    <div className={`call is-${state}${open ? " is-open" : ""}`}>
      <button
        className="call-head"
        type="button"
        data-no-press
        aria-expanded={open}
        onClick={() => {
          setDrawn(true);
          setOpen(!open);
        }}
      >
        <span className="call-lead">
          <Icon className="call-icon" />
          <span className="call-verb">{verbs[state]}</span>
        </span>
        {!tool.cell ? (
          <span className="call-target">{tool.title}</span>
        ) : files.length > 0 ? (
          <span className="call-target is-file" title={files.join("\n")}>
            {tool.cell.kind === "read" && <FileIcon path={files[0]} />}
            {files.map((path, at) => (
              <Fragment key={path}>
                {tool.cell?.away?.[path] && <span className="call-dir">{tool.cell.away[path]}</span>}
                <span className="call-file">
                  {base(path)}
                  {at < files.length - 1 && ", "}
                </span>
              </Fragment>
            ))}
          </span>
        ) : (
          <span className="call-target is-mono">{tool.cell.command ?? gist(tool.input)}</span>
        )}
        {meta.length > 0 && (
          <span className="call-meta">
            {meta.map((part, at) => (
              <Fragment key={at}>
                {at > 0 && <span className="sep">·</span>}
                {part}
              </Fragment>
            ))}
          </span>
        )}
        <IconChevron className="call-chevron" />
      </button>
      <div className="call-fold" inert={!open}>
        <div>{drawn && <CallSheet tool={tool} files={files} diff={diff} failed={failed} />}</div>
      </div>
    </div>
  );
}

/**
 * What a cell opens on, one sheet without labels: its Python, then a tone
 * deeper what came out of it, an edit's changes (a write's lines) under the
 * file's icon and name, and what it printed. The code and the print each
 * open, whole, in an editor tab.
 */
function CallSheet({ tool, files, diff, failed }: { tool: Tool; files: string[]; diff: Cell["diff"]; failed: boolean }) {
  const output = tool.output?.replace(/\n+$/, "");
  return (
    <div className="call-open">
      <div className="call-sheet">
        <div className="call-part">
          <pre className="call-code">{highlight(tool.input)}</pre>
          <OpenTab id={tool.id} part="input" />
        </div>
        {/* yusAgent (PR 3, C6, C7) : `bash()` réels, puis vrais diffs par fichier à la place du diff deviné. */}
        {tool.runs && <BashRuns runs={tool.runs} />}
        {tool.edits && <FileEdits toolId={tool.id} edits={tool.edits} />}
        {diff && !tool.edits && (
          <div className="is-sunk">
            <div className="call-diff-head">
              {files.map((path) => (
                <Fragment key={path}>
                  <FileIcon path={path} />
                  <span title={path}>{base(path)}</span>
                </Fragment>
              ))}
              <span className="call-meta">
                <Changed tools={[tool]} />
              </span>
            </div>
            <div className="call-lines">
              {diff.lines.map(([sign, text], at) => (
                <div key={at} className={sign === "+" ? "call-line is-add" : sign === "-" ? "call-line is-del" : "call-line"}>
                  <span className="gutter">{sign === "+" ? "+" : sign === "-" ? "−" : ""}</span>
                  {sign === "…" ? "…" : text || " "}
                </div>
              ))}
            </div>
          </div>
        )}
        {output && (
          <div className="call-part is-sunk">
            <div className={failed ? "call-lines is-error" : "call-lines is-output"}>
              {output.split("\n").map((line, at) => (
                <div key={at} className="call-line">
                  {line || " "}
                </div>
              ))}
            </div>
            <OpenTab id={tool.id} part="output" />
          </div>
        )}
      </div>
    </div>
  );
}

/** Opens a cell's code, or all it printed (the panel's is cut), in an editor tab. */
function OpenTab({ id, part }: { id: string; part: "input" | "output" }) {
  const tip = part === "input" ? "Open the code" : "Open the output";
  return (
    <button
      className="icon-button call-tab"
      type="button"
      aria-label={tip}
      data-tip={tip}
      data-tip-side="left"
      onClick={() => post({ type: "openCell", id, part })}
    >
      <IconExternal />
    </button>
  );
}

/**
 * The lines cells' edits and writes add and remove: `+2 −1`; writes alone
 * (whatever a file held before, their code does not say), `+42`.
 */
function Changed({ tools }: { tools: Tool[] }) {
  const changed = tools.filter(changes);
  const edits = changed.filter((tool) => tool.cell?.kind !== "write");
  const sum = (of: Tool[], key: "added" | "removed") => of.reduce((n, tool) => n + (tool.cell?.diff?.[key] ?? 0), 0);
  return (
    <>
      <span className="add">+{sum(changed, "added")}</span>
      {edits.length > 0 && (
        <>
          {" "}
          <span className="del">−{sum(edits, "removed")}</span>
        </>
      )}
    </>
  );
}

/** Whether a cell shows the lines it changed: it did not fail, and its code says which. */
function changes(tool: Tool): boolean {
  // yusAgent (PR 3, C7) : une modification faite (vrai diff) compte, même si la cellule a échoué ensuite.
  if (tool.edits?.length) return true;
  return tool.status !== "failed" && tool.cell?.diff !== undefined;
}

/** `a.ts` of `src/a.ts`. */
function base(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

/** A cell's telling line: its first that is not an import or a comment. */
function gist(code: string): string {
  const lines = code
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  return lines.find((line) => !/^(?:#|import\s|from\s+\S+\s+import\s)/.test(line)) ?? lines[0] ?? "";
}

/** Python's strings, comments and keywords. */
const PY =
  /([rRbBfFuU]{0,2}(?:'''[\s\S]*?(?:'''|$)|"""[\s\S]*?(?:"""|$)|'(?:\\.|[^'\\\n])*'?|"(?:\\.|[^"\\\n])*"?))|(#[^\n]*)|\b(False|None|True|and|as|assert|async|await|break|class|continue|def|del|elif|else|except|finally|for|from|global|if|import|in|is|lambda|nonlocal|not|or|pass|raise|return|try|while|with|yield)\b/g;

/** A cell's code, quietly coloured: its keywords in the cyan, its strings a step back, its comments faint. */
function highlight(code: string): ReactNode[] {
  const out: ReactNode[] = [];
  let last = 0;
  for (const match of code.matchAll(PY)) {
    const kind = match[1] !== undefined ? "py-s" : match[2] !== undefined ? "py-c" : "py-k";
    out.push(
      code.slice(last, match.index),
      <span key={match.index} className={kind}>
        {match[0]}
      </span>,
    );
    last = match.index + match[0].length;
  }
  out.push(code.slice(last));
  return out;
}

import {
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ClipboardEvent,
  type DragEvent,
  type KeyboardEvent,
} from "react";
import type {
  Attachment,
  ChatCommand,
  ChatState,
  ConfigChange,
  Draft,
  KernelState,
  Provider,
  QueuedMessage,
  SessionConfig,
  SessionState,
} from "../src/protocol";
import { mentions, mentionText } from "../src/mentions";
import { attach, IMAGE_TYPES } from "./attachments";
import { EffortPicker, pickable } from "./Effort";
import { FileIcon } from "./FileIcon";
import { useFit } from "./fit";
import { GoalHead, goalPhase, GoalPill, useGoalLight } from "./Goal";
import {
  IconBolt,
  IconBook,
  IconChevronDown,
  IconCompact,
  IconCross,
  IconFlag,
  IconGrip,
  IconPencil,
  IconPlay,
  IconPlug,
  IconPlus,
  IconSend,
  IconStop,
  IconText,
} from "./icons";
import { ProviderLogo } from "./logos";
import { findPaths, knows, MentionText, nameOf, PathsContext, type Paths } from "./mentions";
import { KernelTray } from "./Kernel";
import { isLocal, ModelList, providerName } from "./models";
import { PickMenu } from "./PickMenu";
import { Popover } from "./Popover";
import { UsageRing } from "./Usage";
import { onHostMessage, post } from "./vscode";

const PLACEHOLDER = "Ask Unharnessed";

/** The keys that send a message into the running turn. */
const NOW_KEYS = /Mac/.test(navigator.userAgent) ? "⌘⏎" : "Ctrl+Enter";

const set = (change: ConfigChange) => post({ type: "setConfig", ...change });

type Menu = "model" | "effort" | "slash";

/** What is being written: kept above the box, so it survives a visit to the settings. */
export type { Draft };

/** The image settings, as the agent applies them. */
export type ImageSettings = { block: boolean; resize: boolean; imageModel: string | null };

/** Images a message carries at most. */
const MAX_IMAGES = 10;

const GROUPS: [string, ChatCommand["kind"][]][] = [
  ["Commands", ["panel", "command"]],
  ["Prompts", ["prompt"]],
  ["Skills", ["skill"]],
];

/** The commands the `/` menu offers, with the prompts and skills; the others (`/new`, `/autonomous`) run typed. */
const OFFERED = ["compact", "goal"];

/**
 * What the bar lets go of, short of room, first to last (styles.css, `data-short`): the effort's name (its gauge
 * stays), the model's (its mark stays), the goal's time (its pause stays), then a control at a time, down to the
 * model and send.
 */
const BAR_STEPS = ["effort-label", "model-name", "goal-time", "usage", "fast", "effort", "attach"];

/** What is written after a command or a skill, in words: the agent's own hints are syntax (`[instructions]`). */
const ARGUMENT = "instructions";

/** What a command does, said plainly: the agent's own descriptions are terse. */
const ABOUT: Record<string, string> = {
  compact:
    "Summarizes the conversation so far, to free room in the context. Instructions, if you write some, say what the summary should keep.",
  goal: "Gives the agent a goal it keeps working on, turn after turn, until it is reached. Pause, play or end it from the message box.",
};

/** What an entry is, when it says nothing itself. */
const KINDS: Record<ChatCommand["kind"], string> = {
  panel: "A command of the panel.",
  command: "A command of the agent.",
  prompt: "A prompt template: its text goes as your message.",
  skill: "A skill: instructions the agent follows for this kind of task.",
};

/**
 * The message box at the bottom of home: what is being written and the
 * images it carries, the messages waiting for the running turn on top, and
 * the session's pickers (the model, its reasoning effort, its fast mode).
 * Over it, the agent's background commands and REPL variables (Kernel.tsx).
 * Enter sends; while a turn runs the message waits, ⌘/Ctrl+Enter sends it
 * into the turn, and stop ends the turn.
 * While there is a goal, its line tops the box, and its timer with pause and
 * play stands in for stop.
 * A `/` at the start opens the commands, prompts and skills. While turn
 * `editing`'s prompt is in it, sending it goes in place of that prompt (a
 * waiting message's, in its place in the queue); `onEditEnd` ends the edit,
 * sent or given up. While no provider is connected (`unconnected`) or the
 * session has no model, a button to the providers screen (`onProviders`)
 * stands in for the pickers.
 */
export function Composer({
  session,
  providers,
  unconnected,
  chat,
  kernel,
  images,
  draft,
  onDraft,
  editing,
  onEditEnd,
  onProviders,
}: {
  session: SessionState | null;
  providers: Provider[];
  unconnected: boolean;
  chat: ChatState;
  kernel: KernelState;
  images: ImageSettings;
  draft: Draft;
  onDraft: (update: (draft: Draft) => Draft) => void;
  editing: number | null;
  onEditEnd: () => void;
  onProviders: () => void;
}) {
  const [menu, setMenu] = useState<Menu | null>(null);
  /** Bumped when the empty box takes focus: the placeholder streams in again. */
  const [stream, setStream] = useState(0);
  const [notes, setNotes] = useState<string[]>([]);
  const [dragging, setDragging] = useState(false);
  /** The text the `/` or `@` menu was dismissed on: it stays shut until the text changes. */
  const [dismissed, setDismissed] = useState<string | null>(null);
  const [highlight, setHighlight] = useState(0);
  const [caret, setCaret] = useState(0);
  /** Where the caret goes once the text set with it is in the box. */
  const placeCaret = useRef<number | null>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const shell = useRef<HTMLDivElement>(null);
  const glow = useRef<HTMLDivElement>(null);
  const mirror = useRef<HTMLDivElement>(null);
  const picker = useRef<HTMLInputElement>(null);
  const bar = useRef<HTMLDivElement>(null);
  useFit(bar, BAR_STEPS);
  const paths = useContext(PathsContext);
  const config = session?.kind === "open" ? session.config : null;
  // The pickers shown: none while no provider is connected, whatever model the session opened on.
  const pickers = unconnected ? null : config;
  const toggle = (next: Menu) => setMenu((open) => (open === next ? null : next));
  const close = () => setMenu(null);
  const hasContent = draft.text.trim() !== "" || draft.images.length > 0;
  const editingQueued = editing !== null && chat.queue.some((message) => message.id === editing);
  const phase = goalPhase(chat.goal);
  const lit = useGoalLight(shell, glow, phase);

  // A menu whose picker left (a model switch drops the effort, or leaves it a single level) closes.
  useEffect(() => {
    if (menu === "model" && !pickers?.model) setMenu(null);
    if (menu === "effort" && !(pickers?.effort && pickable(pickers.effort))) setMenu(null);
  }, [menu, pickers]);

  const slash = slashCommands(draft.text, chat.commands);
  // While a command's text is written, an `@` in it opens the files.
  const word = slash === null || slash.arg !== null ? mentionQuery(draft.text, caret) : null;
  /** What a dismissal holds for: the text; while a command's text is written, the command. */
  const shutKey =
    word === null && slash?.arg != null ? draft.text.slice(0, draft.text.length - slash.arg.length) : draft.text;
  const shut = dismissed === shutKey || menu !== null;
  const slashOpen = slash !== null && word === null && !shut;
  const atOpen = word !== null && !shut;
  const found = useMemo(() => (atOpen && paths ? findPaths(paths, word.query) : []), [atOpen, paths, word?.query]);
  useEffect(() => setHighlight(0), [draft.text, word?.start]);
  // A `/` typed again opens the menu again.
  useEffect(() => setDismissed((was) => (was === shutKey ? was : null)), [shutKey]);

  // The folder's files: for the pills under the mentions, then fresh each time the `@` menu opens.
  useEffect(() => post({ type: "listFiles" }), []);
  useEffect(() => {
    if (atOpen) post({ type: "listFiles" });
  }, [atOpen]);

  useLayoutEffect(() => {
    if (placeCaret.current === null) return;
    input.current?.setSelectionRange(placeCaret.current, placeCaret.current);
    setCaret(placeCaret.current);
    placeCaret.current = null;
  }, [draft.text]);

  // A prompt to edit comes in: the caret waits at its end.
  useEffect(() => {
    const box = input.current;
    if (editing === null || !box) return;
    box.focus();
    box.setSelectionRange(box.value.length, box.value.length);
  }, [editing]);

  // VS Code takes a drag of files that reaches the panel for its editors, unless Shift is held: one the box
  // can take stays in the panel, refused outside the box.
  useEffect(() => {
    const keep = (event: globalThis.DragEvent) => {
      if (!droppable(event.dataTransfer)) return;
      event.stopPropagation();
      if (event.defaultPrevented) return;
      event.preventDefault();
      event.dataTransfer.dropEffect = "none";
    };
    document.addEventListener("dragenter", keep);
    document.addEventListener("dragover", keep);
    return () => {
      document.removeEventListener("dragenter", keep);
      document.removeEventListener("dragover", keep);
    };
  }, []);

  const setText = (text: string) => onDraft((current) => ({ ...current, text }));
  const edit = (text: string, caretAt: number) => {
    placeCaret.current = caretAt;
    setText(text);
  };

  const addFiles = (files: File[]) => {
    const found = files.filter((file) => IMAGE_TYPES.includes(file.type) || file.type.startsWith("image/"));
    if (found.length === 0) return;
    if (images.block) {
      setNotes(["Images are blocked: turn off “Block images” in the settings to send them."]);
      return;
    }
    const room = MAX_IMAGES - draft.images.length;
    setNotes(found.length > room ? [`A message carries ${MAX_IMAGES} images at most.`] : []);
    for (const file of found.slice(0, Math.max(0, room))) {
      attach(file, images.resize).then(
        (attachment) =>
          onDraft((current) =>
            current.images.length >= MAX_IMAGES ? current : { ...current, images: [...current.images, attachment] },
          ),
        (error: unknown) => setNotes((current) => [...current, error instanceof Error ? error.message : String(error)]),
      );
    }
  };

  // The images the extension read from a drop out of VS Code; subscribed anew each render, for the current draft.
  useEffect(() =>
    onHostMessage((message) => {
      if (message.type !== "droppedImages") return;
      addFiles(message.images.map(fileOf));
      if (message.errors.length > 0) setNotes((current) => [...current, ...message.errors]);
    }),
  );

  /** `now`: into the running turn. */
  const submit = (now = false) => {
    if (!hasContent) return;
    post({
      type: "send",
      text: draft.text.trim(),
      images: draft.images,
      edit: editing ?? undefined,
      now: now || undefined,
    });
    if (editing === null) onDraft(() => ({ text: "", images: [] }));
    else onEditEnd();
    setNotes([]);
  };

  const pick = (command: ChatCommand) => {
    setText(`/${slashName(command)} `);
    input.current?.focus();
  };

  /** The `@` word under the caret becomes the mention of `path`, two spaces after it: its pill covers the first. */
  const pickPath = (path: string) => {
    if (!word) return;
    const before = draft.text.slice(0, word.start);
    const inserted = `${mentionText(path)}  `;
    edit(before + inserted + draft.text.slice(word.end).replace(/^ +/, ""), before.length + inserted.length);
    input.current?.focus();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.nativeEvent.isComposing) return;
    // While a command's text is written, its row stays alone: Enter sends.
    const listing = slashOpen && slash.arg === null;
    const count = listing ? slash.commands.length : atOpen ? found.length : 0;
    if (count > 0) {
      const move = { ArrowDown: 1, ArrowUp: -1 }[event.key];
      if (move) {
        event.preventDefault();
        setHighlight((at) => (at + move + count) % count);
        return;
      }
      if (event.key === "Enter" || event.key === "Tab") {
        event.preventDefault();
        const chosen = Math.min(highlight, count - 1);
        if (listing) pick(slash.commands[chosen]);
        else pickPath(found[chosen]);
        return;
      }
    }
    if ((slashOpen || atOpen) && event.key === "Escape") {
      event.preventDefault();
      setDismissed(shutKey);
      return;
    }
    // A mention goes whole: one backspace right after it.
    if (event.key === "Backspace" && !event.altKey && !event.metaKey && !event.ctrlKey) {
      const { selectionStart, selectionEnd } = event.currentTarget;
      const mention =
        selectionStart === selectionEnd && mentions(draft.text, knows(paths)).find((m) => m.end === selectionStart);
      if (mention) {
        event.preventDefault();
        edit(draft.text.slice(0, mention.start) + draft.text.slice(mention.end), mention.start);
        return;
      }
    }
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      submit(event.metaKey || event.ctrlKey);
    }
    if (event.key === "Escape" && editing !== null) {
      event.preventDefault();
      onEditEnd();
    }
  };

  const onPaste = (event: ClipboardEvent) => {
    const files = [...event.clipboardData.files];
    if (files.some((file) => file.type.startsWith("image/"))) {
      event.preventDefault();
      addFiles(files);
    }
  };

  const onDrop = (event: DragEvent) => {
    setDragging(false);
    if (!droppable(event.dataTransfer)) return;
    event.preventDefault();
    if (event.dataTransfer.files.length > 0) {
      addFiles([...event.dataTransfer.files]);
      return;
    }
    // From VS Code (the explorer, a tab): files' URIs, which the extension reads.
    const list = event.dataTransfer.getData(CODE_URIS) || event.dataTransfer.getData("text/uri-list");
    const uris = list
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line !== "" && !line.startsWith("#"));
    if (uris.length > 0) post({ type: "readImages", uris });
  };

  const current = config?.model?.choices.find((choice) => choice.value === config.model?.current);
  // A model whose inputs are known, without images among them, and no image model to read them.
  const unreadable =
    draft.images.length > 0 && !images.imageModel && !!current?.input.length && !current.input.includes("image");

  return (
    // The dock: the box, the waiting messages on its top, the kernel's pills over it (laid over the conversation's end).
    <div className="dock">
      <KernelTray kernel={kernel} />
      <div
        ref={shell}
        className={dragging ? "composer-shell is-dragging" : "composer-shell"}
        data-goal={phase ?? undefined}
        onDragOver={(event) => {
          if (!droppable(event.dataTransfer)) return;
          event.preventDefault();
          event.dataTransfer.dropEffect = "copy";
          setDragging(true);
        }}
        onDragLeave={() => setDragging(false)}
        onDrop={onDrop}
      >
        <div className="composer-glow" ref={glow} aria-hidden="true" />
        <div className="composer">
          {menu === "model" && pickers?.model && (
            <ModelMenu model={pickers.model} providers={providers} onClose={close} />
          )}
          {slashOpen && (
            <SlashMenu
              slash={slash}
              highlight={highlight}
              onHighlight={setHighlight}
              onPick={(command) => (slash.arg === null ? pick(command) : input.current?.focus())}
              onClose={() => setDismissed(shutKey)}
            />
          )}
          {atOpen && (
            <FileMenu
              paths={paths}
              found={found}
              query={word.query}
              highlight={highlight}
              onHighlight={setHighlight}
              onPick={pickPath}
              onClose={() => setDismissed(shutKey)}
            />
          )}
          {chat.queue.length > 0 && <Queue queue={chat.queue} paused={chat.paused} busy={chat.busy} />}
          <GoalHead goal={chat.goal} lit={lit} />
          {editing !== null && (
            <div className="composer-editing">
              <IconPencil />
              <span>{editingQueued ? "Editing a waiting message" : "Editing an earlier message"}</span>
              <button
                className="icon-button"
                type="button"
                aria-label="Stop editing"
                data-tip="Stop editing"
                data-tip-align="end"
                onClick={onEditEnd}
              >
                <IconCross />
              </button>
            </div>
          )}
          {draft.images.length > 0 && (
            <Attachments
              images={draft.images}
              onRemove={(at) => onDraft((d) => ({ ...d, images: d.images.filter((_, i) => i !== at) }))}
            />
          )}
          <div className="composer-field">
            {/* Under the text, a pill under each mention; it scrolls with the box. */}
            <div className="composer-mirror" ref={mirror} aria-hidden="true">
              <MentionText text={draft.text.endsWith("\n") ? `${draft.text} ` : draft.text} />
            </div>
            <textarea
              ref={input}
              className="composer-input"
              rows={3}
              aria-label={PLACEHOLDER}
              spellCheck={false}
              value={draft.text}
              onChange={(event) => {
                setText(event.target.value);
                setCaret(event.target.selectionStart);
              }}
              onSelect={(event) => setCaret(event.currentTarget.selectionStart)}
              onScroll={(event) => {
                if (mirror.current) mirror.current.scrollTop = event.currentTarget.scrollTop;
              }}
              onFocus={() => draft.text === "" && setStream((n) => n + 1)}
              onKeyDown={onKeyDown}
              onPaste={onPaste}
            />
            {draft.text === "" && <Placeholder key={stream} streaming={stream > 0} />}
          </div>
          {notes.map((note, at) => (
            <p className="composer-note" key={at}>
              {note}
            </p>
          ))}
          {unreadable && (
            <p className="composer-note">
              {current.name} doesn't read images: pick an image model in the settings, or a model that reads them.
            </p>
          )}
          {session?.kind === "open" && session.error && <p className="composer-error">{session.error}</p>}
          <div className="composer-bar" ref={bar}>
            <input
              ref={picker}
              type="file"
              accept={IMAGE_TYPES.join(",")}
              multiple
              hidden
              onChange={(event) => {
                addFiles([...(event.target.files ?? [])]);
                event.target.value = "";
              }}
            />
            <button
              className="chip"
              type="button"
              aria-label="Attach images"
              data-tip={images.block ? "Images are blocked in the settings" : "Attach images, or drop them with Shift held"}
              data-tip-align="start"
              disabled={images.block}
              onClick={() => picker.current?.click()}
            >
              <IconPlus />
            </button>
            <ModelButton
              session={session}
              providers={providers}
              open={menu === "model"}
              onClick={() => toggle("model")}
              unconnected={unconnected}
              onProviders={onProviders}
            />
            {pickers?.effort && (
              <EffortPicker
                effort={pickers.effort}
                open={menu === "effort"}
                onToggle={() => toggle("effort")}
                onClose={close}
              />
            )}
            {pickers && pickers.fast !== null && <FastToggle on={pickers.fast} />}
            <span className="composer-spacer" />
            {config && chat.usage && <UsageRing usage={chat.usage} empty={chat.entries.length === 0} />}
            {chat.goal && phase && <GoalPill goal={chat.goal} phase={phase} lit={lit} />}
            {/* While the goal runs, its pause stops the turn. */}
            {chat.busy && phase !== "running" && (
              <button
                className="send is-stop"
                type="button"
                aria-label="Stop"
                data-tip="Stop"
                data-tip-align="end"
                onClick={() => post({ type: "cancel" })}
              >
                <IconStop />
              </button>
            )}
            {(!chat.busy || hasContent) && (
              <button
                className="send"
                type="button"
                aria-label="Send"
                data-tip={
                  editingQueued
                    ? "Keep it in its place"
                    : chat.busy
                      ? `Send when the agent is done; ${NOW_KEYS} sends it now`
                      : "Send"
                }
                data-tip-align="end"
                disabled={!hasContent}
                onClick={() => submit()}
              >
                <IconSend />
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

/**
 * What the `/` menu shows: the commands matching a `/` at the start of the
 * text, names starting with what is typed first; once a space follows a
 * command's name, that command alone and `arg`, what is written for it.
 */
type Slash = { commands: ChatCommand[]; arg: string | null };

/** The `/` menu for `text`; `null` when the text opens none. */
function slashCommands(text: string, commands: ChatCommand[]): Slash | null {
  const offered = (command: ChatCommand) =>
    command.kind === "prompt" || command.kind === "skill" || OFFERED.includes(command.name);
  const written = /^\/(\S+)\s([\s\S]*)$/.exec(text);
  if (written) {
    const name = written[1].toLowerCase();
    const command = commands.find((c) => offered(c) && slashName(c).toLowerCase() === name);
    return command ? { commands: [command], arg: written[2] } : null;
  }
  const match = /^\/(\S*)$/.exec(text);
  if (!match) return null;
  const query = match[1].toLowerCase();
  const found = commands.filter((command) => offered(command) && slashName(command).toLowerCase().includes(query));
  const starts = (command: ChatCommand) => (slashName(command).toLowerCase().startsWith(query) ? 0 : 1);
  const sorted = GROUPS.flatMap(([, kinds]) =>
    found.filter((command) => kinds.includes(command.kind)).sort((a, b) => starts(a) - starts(b)),
  );
  return { commands: sorted, arg: null };
}

/** What is typed for `command`: a skill goes by its name, the extension adds the agent's `skill:`. */
function slashName(command: ChatCommand): string {
  return command.kind === "skill" ? command.name.replace(/^skill:/, "") : command.name;
}

/**
 * The `@` word the caret is in, from its `@` to its end, and what is typed
 * of it up to the caret; `null` when the caret is in none.
 */
function mentionQuery(text: string, caret: number): { start: number; end: number; query: string } | null {
  const match = /(?:^|\s)@([^\s"]*)$/.exec(text.slice(0, caret));
  if (!match) return null;
  const rest = /^\S*/.exec(text.slice(caret))?.[0] ?? "";
  return { start: caret - match[1].length - 1, end: caret + rest.length, query: match[1] };
}

/**
 * The commands, prompts and skills a `/` opens; the highlighted one is
 * picked with Enter or Tab. Picked, it stays, what is written for it in its
 * quotes as it is typed.
 */
function SlashMenu({
  slash: { commands, arg },
  highlight,
  onHighlight,
  onPick,
  onClose,
}: {
  slash: Slash;
  highlight: number;
  onHighlight: (at: number) => void;
  onPick: (command: ChatCommand) => void;
  onClose: () => void;
}) {
  const written = arg?.replace(/\s+/g, " ").trim();
  const picks = commands.map((command) => {
    // A template says what it takes itself.
    const blank = command.kind === "prompt" ? command.hint : `"${ARGUMENT}"`;
    const hint = written ? (
      <span className="pick-hint">
        "<span className="pick-written">{written}</span>"
      </span>
    ) : (
      blank && <span className="pick-hint">{blank}</span>
    );
    return {
      key: command.name,
      group: GROUPS.find(([, kinds]) => kinds.includes(command.kind))?.[0],
      icon: iconOf(command),
      name: (
        <>
          {command.kind === "command" ? capitalized(command.name) : slashName(command)}
          {hint}
        </>
      ),
      detail: (
        <p className="pick-explain">
          {(command.kind === "command" && ABOUT[command.name]) || command.description || KINDS[command.kind]}
        </p>
      ),
    };
  });
  return (
    <PickMenu
      className="slash-menu"
      trigger="slash"
      label="Commands"
      picks={picks}
      empty="No command, prompt or skill by that name."
      highlight={highlight}
      onHighlight={onHighlight}
      onPick={(at) => onPick(commands[at])}
      onClose={onClose}
    />
  );
}

const capitalized = (name: string) => name.charAt(0).toUpperCase() + name.slice(1);

/** The goal's flag as the box has it, in cyan; a mark for the others. */
function iconOf(command: ChatCommand) {
  if (command.name === "goal") return <IconFlag className="pick-goal" />;
  if (command.name === "compact") return <IconCompact />;
  return command.kind === "skill" ? <IconBook /> : <IconText />;
}

/** The folder's files and folders an `@` opens: a file's name, then where it is. */
function FileMenu({
  paths,
  found,
  query,
  highlight,
  onHighlight,
  onPick,
  onClose,
}: {
  paths: Paths | null;
  found: string[];
  query: string;
  highlight: number;
  onHighlight: (at: number) => void;
  onPick: (path: string) => void;
  onClose: () => void;
}) {
  const open = new Set(paths?.open);
  const grouped = query === "" && open.size > 0;
  const picks = found.map((path) => {
    const name = nameOf(path);
    return {
      key: path,
      group: grouped ? (open.has(path) ? "Open" : "Workspace") : undefined,
      icon: <FileIcon path={path} />,
      name,
      about: path.slice(0, path.length - name.length),
    };
  });
  const empty =
    paths === null
      ? "Listing the folder's files…"
      : paths.entries.length === 0
        ? "Open a folder to mention its files."
        : "No file by that name.";
  return (
    <PickMenu
      className="file-menu"
      trigger="mention"
      label="Files"
      picks={picks}
      empty={empty}
      highlight={highlight}
      onHighlight={onHighlight}
      onPick={(at) => onPick(found[at])}
      onClose={onClose}
    />
  );
}

/** What VS Code puts on a drag from the explorer or a tab: every dragged file's URI (`text/uri-list` has the first). */
const CODE_URIS = "application/vnd.code.uri-list";

/** A drag the message box takes: files, or files' URIs. */
function droppable(data: DataTransfer | null): data is DataTransfer {
  return !!data && ["Files", CODE_URIS, "text/uri-list"].some((type) => data.types.includes(type));
}

/** An image the extension read, as a file to attach. */
function fileOf({ name, mimeType, data }: { name: string; mimeType: string; data: string }): File {
  return new File([Uint8Array.from(atob(data), (char) => char.charCodeAt(0))], name, { type: mimeType });
}

/** The images the message carries, each with its remove button. */
function Attachments({ images, onRemove }: { images: Attachment[]; onRemove: (at: number) => void }) {
  return (
    <div className="attachments">
      {images.map((image, at) => (
        <div className="attachment" key={at}>
          <img src={image.thumb} alt={image.name} title={image.name} />
          <button
            className="attachment-remove"
            type="button"
            aria-label={`Remove ${image.name}`}
            onClick={() => onRemove(at)}
          >
            <IconCross />
          </button>
        </div>
      ))}
    </div>
  );
}

/** What a waiting message's drag carries: its id. */
const QUEUED = "application/x-unharnessed-queued";

/**
 * The messages waiting for the running turn, on top of the box, in the
 * order they go: those sent into the turn and not taken yet first. One goes
 * now, comes back to the box to be changed, or leaves; dragged, it takes
 * another place. A stop pauses them, play sends them again.
 */
function Queue({ queue, paused, busy }: { queue: QueuedMessage[]; paused: boolean; busy: boolean }) {
  const [open, setOpen] = useState(true);
  const [dragged, setDragged] = useState<number | null>(null);
  /** Where the dragged one would land: before the waiting message at this place, or last past them. */
  const [target, setTarget] = useState<number | null>(null);
  const waiting = queue.filter((message) => message.state !== "steering");
  const from = waiting.findIndex((message) => message.id === dragged);
  // Before or after itself, it would not move.
  const landing = target !== null && from >= 0 && target !== from && target !== from + 1 ? target : null;

  const end = () => {
    setDragged(null);
    setTarget(null);
  };
  const drop = () => {
    if (dragged !== null && landing !== null) {
      post({ type: "moveQueued", id: dragged, to: landing > from ? landing - 1 : landing });
    }
    end();
  };

  return (
    <div className={paused ? "queue is-paused" : "queue"}>
      <div className="queue-head">
        <button className="queue-toggle" type="button" aria-expanded={open} onClick={() => setOpen(!open)}>
          <IconChevronDown />
          <span>{paused ? "Paused" : "Waiting"}</span>
          <span className="queue-count">{queue.length}</span>
        </button>
        {paused && (
          <button
            className="queue-resume"
            type="button"
            data-tip="Send them again, in order"
            data-tip-align="end"
            onClick={() => post({ type: "resumeQueue" })}
          >
            <IconPlay />
            Resume
          </button>
        )}
      </div>
      <div className={open ? "queue-drawer is-open" : "queue-drawer"}>
        <ol className="queue-list" aria-label="Waiting messages">
          {queue.map((message) => {
            const at = waiting.indexOf(message);
            const movable = message.state === "waiting" && waiting.length > 1;
            const edge =
              landing === null
                ? undefined
                : landing === at
                  ? "before"
                  : landing === at + 1 && at === waiting.length - 1
                    ? "after"
                    : undefined;
            return (
              <li
                key={message.id}
                className={message.id === dragged ? "queued is-dragged" : "queued"}
                data-state={message.state}
                data-drop={edge}
                draggable={movable}
                onDragStart={(event) => {
                  event.dataTransfer.setData(QUEUED, String(message.id));
                  event.dataTransfer.effectAllowed = "move";
                  setDragged(message.id);
                }}
                onDragOver={(event) => {
                  if (dragged === null || at < 0) return;
                  event.preventDefault();
                  event.dataTransfer.dropEffect = "move";
                  const box = event.currentTarget.getBoundingClientRect();
                  setTarget(event.clientY > box.top + box.height / 2 ? at + 1 : at);
                }}
                onDrop={(event) => {
                  event.preventDefault();
                  drop();
                }}
                onDragEnd={end}
              >
                <span className="queued-mark" aria-hidden="true">
                  {message.state === "steering" ? (
                    <span className="queued-dot" />
                  ) : message.state === "held" ? (
                    <IconPencil />
                  ) : (
                    <>
                      <span className="queued-index">{at + 1}</span>
                      {movable && <IconGrip className="queued-grip" />}
                    </>
                  )}
                </span>
                <button
                  className="queued-text"
                  type="button"
                  disabled={message.state !== "waiting"}
                  onClick={() => post({ type: "edit", id: message.id })}
                >
                  {message.text || `${message.images.length} image${message.images.length > 1 ? "s" : ""}`}
                </button>
                {message.images.length > 0 && (
                  <span className="queued-thumbs">
                    {message.images.slice(0, 3).map((image, i) => (
                      <img key={i} src={image.thumb} alt={image.name} />
                    ))}
                    {message.images.length > 3 && <span className="queued-more">+{message.images.length - 3}</span>}
                  </span>
                )}
                {message.state === "waiting" ? (
                  <span className="queued-actions">
                    <button
                      className="queued-action is-now"
                      type="button"
                      aria-label="Send now"
                      data-tip={busy ? "Send now: the agent reads it at its next step" : "Send now"}
                      onClick={() => post({ type: "sendNow", id: message.id })}
                    >
                      <IconSend />
                    </button>
                    <button
                      className="queued-action"
                      type="button"
                      aria-label="Edit"
                      data-tip="Edit"
                      onClick={() => post({ type: "edit", id: message.id })}
                    >
                      <IconPencil />
                    </button>
                    <button
                      className="queued-action"
                      type="button"
                      aria-label="Remove"
                      data-tip="Remove"
                      data-tip-align="end"
                      onClick={() => post({ type: "unqueue", id: message.id })}
                    >
                      <IconCross />
                    </button>
                  </span>
                ) : (
                  <span className="queued-tag">{message.state === "steering" ? "Sending" : "Editing"}</span>
                )}
              </li>
            );
          })}
        </ol>
      </div>
    </div>
  );
}

/** The box's placeholder; on focus it streams in again, letter by letter. */
function Placeholder({ streaming }: { streaming: boolean }) {
  return (
    <span className={streaming ? "composer-placeholder is-streaming" : "composer-placeholder"} aria-hidden="true">
      {[...PLACEHOLDER].map((char, i) => (
        <span key={i} style={streaming ? { animationDelay: `${i * 28}ms` } : undefined}>
          {char}
        </span>
      ))}
    </span>
  );
}

function ModelButton({
  session,
  providers,
  unconnected,
  open,
  onClick,
  onProviders,
}: {
  session: SessionState | null;
  providers: Provider[];
  unconnected: boolean;
  open: boolean;
  onClick: () => void;
  onProviders: () => void;
}) {
  const model = session?.kind === "open" ? session.config.model : null;
  const current = model?.choices.find((choice) => choice.value === model.current);
  // Nothing to pick from: no provider is connected (the session still opens on a model), or the session has no model.
  if (unconnected || (!model && session?.kind === "open")) {
    return (
      <button className="chip" type="button" onClick={onProviders}>
        <IconPlug />
        <span className="chip-label">Connect a provider</span>
      </button>
    );
  }
  if (!model) {
    const label = session?.kind === "failed" ? "No session" : "Loading models";
    return (
      <span className="chip is-idle" data-tip={session?.kind === "failed" ? session.error : undefined} data-tip-align="start">
        {session?.kind === "opening" && <span className="spinner" />}
        <span className="chip-label">{label}</span>
      </span>
    );
  }
  return (
    <button
      className={open ? "chip is-open" : "chip"}
      type="button"
      data-tip="Model"
      data-tip-align="start"
      aria-haspopup="listbox"
      aria-expanded={open}
      data-menu-trigger="model"
      onClick={onClick}
    >
      {current && (
        <ProviderLogo
          provider={current.provider}
          name={providerName(providers, current.provider)}
          local={isLocal(providers, current.provider)}
        />
      )}
      <span className="chip-label">{current?.name ?? model.current}</span>
      <IconChevronDown className="chip-chevron" />
    </button>
  );
}

function FastToggle({ on }: { on: boolean }) {
  return (
    <button
      className={on ? "chip is-on" : "chip"}
      type="button"
      data-tip={on ? "Fast mode on: priority speed and price" : "Fast mode off"}
      aria-label="Fast mode"
      aria-pressed={on}
      onClick={() => set({ configId: "fast_mode", value: !on })}
    >
      <IconBolt />
    </button>
  );
}

/** The model picker, above the composer. */
function ModelMenu({
  model,
  providers,
  onClose,
}: {
  model: NonNullable<SessionConfig["model"]>;
  providers: Provider[];
  onClose: () => void;
}) {
  return (
    <Popover className="menu" trigger="model" onClose={onClose}>
      <ModelList
        choices={model.choices}
        selected={model.current}
        keyOf={(choice) => choice.value}
        providers={providers}
        onPick={(choice) => {
          if (choice && choice.value !== model.current) set({ configId: "model", value: choice.value });
          onClose();
        }}
      />
    </Popover>
  );
}

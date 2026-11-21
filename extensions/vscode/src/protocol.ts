// Messages between the extension host and the panel webview. Types only:
// both bundles import this file.

/**
 * The `unharnessed --mode acp` process as the panel shows it. Versions, pid
 * and binary path stay in the log: the panel only says whether it runs.
 */
export type AgentState =
  | { kind: "starting" }
  | { kind: "ready" }
  | { kind: "failed"; error: string; stderr: string[] };

/** One provider row, as the agent's `_unharnessed/providers/list` gives it. */
export type Provider = {
  id: string;
  name: string;
  /** What the credential is. Anthropic and xAI have a row of each. */
  method: "subscription" | "apiKey";
  /** `browser`: the agent runs the sign-in. `apiKey`: the panel asks for the key. */
  login: "browser" | "apiKey";
  connected: boolean;
  /** The credential is in the agent's auth.json, so it can be removed. */
  removable: boolean;
  /** Where an outside key comes from, or why the row is not connected. */
  note?: string;
  /** The Anthropic subscription warning, on the row that uses it. */
  warning?: string;
  /** A server on this machine, from the agent's models.json: its address and the models picked from it. */
  local?: { baseUrl: string; models: string[] };
};

/** A model a local server serves, with what the server tells of it. */
export type LocalModel = { id: string; contextWindow?: number; image: boolean; reasoning: boolean };

/** A local server (`_unharnessed/providers/local_probe`, `local_scan`). */
export type LocalServer = {
  /** The provider id it gets, when it is not added yet. */
  id: string;
  /** What it is (`Ollama`), else its address. */
  name: string;
  baseUrl: string;
  models: LocalModel[];
};

/** What a server serves, or why it could not be read. */
export type LocalProbe =
  | { server: LocalServer }
  | { error: { kind: "invalid" | "unreachable" | "needsKey"; message: string } };

/** A server to add, or one added whose models are picked again (`providerId`). */
export type LocalSave = {
  providerId?: string;
  id: string;
  name: string;
  baseUrl: string;
  /** Absent: the saved key stays. */
  apiKey?: string;
  /** A model added before needs only its id: it keeps its definition. */
  models: (Pick<LocalModel, "id"> & Partial<LocalModel>)[];
};

/** One step of a running sign-in (`_unharnessed/providers/login_update`). */
export type LoginUpdate =
  | { kind: "progress"; message: string }
  | { kind: "waiting"; message: string }
  | { kind: "url"; url: string; instructions: string | null }
  | {
      kind: "prompt";
      promptId: number;
      message: string;
      secret: boolean;
      allowEmpty: boolean;
      /** An alternative to the browser sign-in running beside it. */
      optional: boolean;
    }
  | { kind: "team"; promptId: number; teams: Team[]; current: string | null };

export type Team = { id: string; name: string; role: string | null };

/** What a prompt gets back. */
export type LoginAnswer =
  | { kind: "text"; value: string }
  /** `null`: the personal account. */
  | { kind: "team"; teamId: string | null }
  /** On a team picker: keep the stored team. */
  | { kind: "dismiss" };

/** The browser sign-in in progress, as the panel shows it. */
export type LoginState = {
  providerId: string;
  name: string;
  url?: { url: string; instructions: string | null };
  /** The latest progress line; `waiting` while the provider waits on the browser. */
  status?: { message: string; waiting: boolean };
  prompt?: Extract<LoginUpdate, { kind: "prompt" }>;
  team?: Extract<LoginUpdate, { kind: "team" }>;
  /** Set when the sign-in failed; the screen stays until the user leaves it. */
  error?: string;
};

/** One model the session can switch to: a value of the `model` option. */
export type ModelChoice = {
  /** The agent's opaque value, sent back to select it. */
  value: string;
  id: string;
  name: string;
  /** The provider id (`openai`); the panel names it from the providers list. */
  provider: string;
  /** Tokens; 0 when the agent did not say. */
  contextWindow: number;
  /** What the model reads: `text`, `image`. */
  input: string[];
  /** Dollars per million tokens; absent when the agent did not say. */
  cost?: { input: number; output: number };
};

/** The session's pickers: its config options, kept current by the agent. */
export type SessionConfig = {
  model: { current: string; choices: ModelChoice[] } | null;
  /** The reasoning effort; `null` when the model has none. A single level is shown, with nothing to pick. */
  effort: { current: string; levels: string[] } | null;
  /** The fast mode (priority tier); `null` when the model has none. */
  fast: boolean | null;
  /**
   * yusAgent (YA-10, ticket 24) : la valeur `permissions` que l'agent annonce (`manual`, `auto`) ;
   * `null` sans l'option. Le bandeau D4 la suit ; le panneau ne la change pas (D1).
   */
  permissions: string | null;
};

/** The agent session (`session/new`) as the panel shows it. */
export type SessionState =
  | { kind: "opening" }
  /** `error`: the last selection the agent refused. */
  | { kind: "open"; config: SessionConfig; error?: string }
  | { kind: "failed"; error: string };

/** The options the panel sets. */
export type ConfigChange =
  | { configId: "model" | "thought_level"; value: string }
  | { configId: "fast_mode"; value: boolean };

/** The global settings, as `_unharnessed/settings/get` gives them. Keys are the file's dotted paths. */
export type SettingsView = {
  /** Each key's value in `~/.unharnessed/settings.json`; `null` when unset. */
  values: Record<string, unknown>;
  /** What the agent uses for an unset key; `null`: nothing. */
  defaults: Record<string, unknown>;
  /** Keys read when the agent starts; the others apply on their next use. */
  restart: string[];
};

/**
 * One group of the `modelInstructions` setting: its text joins the agent's
 * system prompt when it runs on one of its models (`provider/model-id`).
 */
export type ModelInstructions = { models: string[]; text: string };

/** One MCP server, as the agent's `_unharnessed/mcp/list` gives it. */
export type McpServer = {
  id: string;
  label: string;
  /** `catalog`: a service of Prime's catalog, the Marketplace; `user`: one of the `mcpServers` setting. */
  source: "catalog" | "user";
  /** `pending`: signed in, the connection not checked yet. */
  status: "connected" | "pending" | "not_connected" | "setup_required" | "disabled" | "error";
  /** An OAuth sign-in can run now. */
  signIn: boolean;
  /** A sign-in or token is stored: signing out removes it. */
  signedIn: boolean;
  description?: string;
  /** What it needs, or why it fails. */
  hint?: string;
  category?: string;
  publisher?: string;
  /** Other names to find it by. */
  aliases?: string[];
  /** A catalog service connected by a pasted token: what to paste ("GitHub personal access token"). */
  token?: string;
};

/** One skill, as the agent's `_unharnessed/skills/list` gives it. */
export type Skill = {
  name: string;
  description: string;
  /** Its file. */
  path: string;
  /** Its folder, `~` for the home folder. */
  location: string;
  /** `user`: the agent's skills folders; `folder`: a folder added here; `project`: the workspace's. */
  source: "builtin" | "user" | "folder" | "project";
  /** The agent loads it when it starts. */
  enabled: boolean;
  /** The running agent has it. */
  active: boolean;
};

export type SkillsView = {
  skills: Skill[];
  /** The folders added to the `skills` setting. */
  folders: string[];
  /** What keeps a skill from loading. */
  problems: { message: string; path?: string }[];
  /** The next start loads other skills than the running agent has. */
  restart: boolean;
};

/** The settings screen's state. */
export type SettingsState = {
  /** `null` until the agent has answered. */
  view: SettingsView | null;
  /** What a restart would apply: the restart keys changed since the agent started, `skills`, `mcp`. */
  pending: string[];
  mcp: McpServer[] | null;
  skills: SkillsView | null;
};

/** An image attached to a message: what the agent gets, and a small copy to show. */
export type Attachment = {
  /** Base64, without the `data:` prefix. */
  data: string;
  mimeType: string;
  /** A small JPEG data URL. */
  thumb: string;
  name: string;
};

/** What the transcript keeps of an attachment. */
export type Thumb = Pick<Attachment, "thumb" | "name">;

/** One slash command, typed as `/name` at the start of a message. */
export type ChatCommand = {
  name: string;
  description: string;
  /** What to type after the name. */
  hint?: string;
  /** `command`: the agent runs it (`/compact`); `prompt`: a prompt template; `panel`: the panel's own (`/new`). */
  kind: "command" | "prompt" | "skill" | "panel";
};

/**
 * What a Python cell does, guessed from its code (cell.ts): it edits a
 * file, writes one, looks at images, runs a command, looks for files,
 * reads them, or none of these.
 */
export type Cell = {
  kind: "edit" | "write" | "view" | "shell" | "search" | "read" | "python";
  /** The files it edits, writes, looks at or reads, as its code names them. */
  files: string[];
  /** Those of `files` out of the agent's folder, and the folder each lies in, as shown (cell.ts, awayDir). */
  away?: Record<string, string>;
  /** The command it runs: the first line of the first one. */
  command?: string;
  /**
   * An edit's changes, from the texts its code replaces (no line of the file
   * around them), and how many lines it adds and removes; or the lines a
   * write writes, all added, when its code holds them.
   */
  diff?: { lines: DiffLine[]; added: number; removed: number };
};

/** A line of a diff: kept, removed or added; `…` stands for lines left out. */
export type DiffLine = [" " | "-" | "+" | "…", string];

/** What a turn shows, in the order the agent did it. */
export type TurnItem =
  /**
   * The model's reasoning: when it started (`Date.now()`), and `ms`, how long
   * it took, once it is over. Some providers send little or none of its text.
   */
  | { kind: "thought"; text: string; started: number; ms?: number }
  | { kind: "text"; text: string }
  | {
      kind: "tool";
      id: string;
      /** `Python cell`, or the tool's name. */
      title: string;
      status: "running" | "done" | "failed";
      /** The cell's code, or the tool's input as JSON. */
      input: string;
      output?: string;
      /** What a Python cell does; absent on another tool. */
      cell?: Cell;
      /** Images the cell loaded into the context. */
      images?: number;
      /** The exception a failed cell raised: `AssertionError`. */
      error?: string;
    }
  /**
   * The conversation compacted into a summary: on its own (`auto`), or by
   * `/compact`. Once done, the tokens it summarized and the summary.
   */
  | {
      kind: "compaction";
      state: "running" | "done";
      auto: boolean;
      tokensBefore?: number;
      summary?: string;
    }
  /** A command's answer, a goal's change. */
  | { kind: "notice"; tone: "info" | "warning" | "error"; text: string };

export type TurnHead = {
  kind: "turn";
  id: number;
  /** What the user sent; absent on a turn the agent started on its own. */
  prompt?: { text: string; images: Thumb[] };
  /** What woke the agent, on its own turns: a background command ended, a usage limit reset, a goal's pause ended. */
  wake?: "bash" | "quota" | "goal";
  state: "running" | "done" | "cancelled" | "failed";
  error?: string;
  /** How long it worked, once done; absent on a turn the agent replays. */
  ms?: number;
  /** Where the agent goes back to send the prompt again, changed; absent when it cannot (an earlier agent's turn). */
  rewindTo?: string;
};

/**
 * The line where the agent restarted: it remembers nothing above it; or
 * where its kernel came back with the conversation's variables, or could
 * not, `at` (`Date.now()`) when it did.
 */
export type ChatLine = { kind: "restart"; id: number } | { kind: "kernel"; id: number; restored: boolean; at: number };

/** A turn, or a line between turns. */
export type EntryHead = TurnHead | ChatLine;
export type ChatEntry = (TurnHead & { items: TurnItem[] }) | ChatLine;

/** One saved conversation of the panel's folder, as the agent's `session/list` gives it. */
export type SavedChat = {
  id: string;
  /** The start of its first prompt; absent when that had only images. */
  title?: string;
  /** When it last changed (ms since the epoch). */
  updated: number;
  /** The conversation in the panel. */
  current: boolean;
  mark?: ChatMark;
};

/** A conversation the panel left: a turn runs in it, or one ended there unseen. */
export type ChatMark = "working" | "unread";

/** The message box: its text and images. */
export type Draft = { text: string; images: Attachment[] };

/** The history menu's state. */
export type HistoryState = {
  /** `null` until the agent has answered. */
  chats: SavedChat[] | null;
  /** A conversation ended in the background, unseen since: the menu's button says so. */
  unread: boolean;
  /** Why the list or a delete failed. */
  error?: string;
};

/**
 * A message waiting for the running turn to end. `held`: back in the
 * message box to be changed, the queue waits at it. `steering`: sent into
 * the running turn, which has not taken it yet.
 */
export type QueuedMessage = { id: number; text: string; images: Thumb[]; state: "waiting" | "held" | "steering" };

/**
 * What the conversation has used, as the agent reports it after each
 * message (`_meta.unharnessed.usage`): its context against the model's
 * window and the automatic compaction's threshold, and what it cost.
 */
export type Usage = {
  /** The context's tokens; absent until a reply measures it, and again right after a compaction. */
  tokens?: number;
  /** The model's context window; 0 when unknown. */
  window: number;
  /** Past this, a compaction runs on its own; absent when that is off. */
  compactAt?: number;
  /** Dollars so far: the replies and the compaction summaries. */
  cost: number;
  /** The provider billed `cost` (OpenRouter); otherwise it is an estimate at the catalog's prices. */
  billed: boolean;
  /** Prompt tokens each reply could reuse from the provider's cache (the request before it), and those it did. */
  cacheReusable: number;
  cacheReused: number;
  /** Dollars the cache reads saved, at each reply's own prices. */
  cacheSaved: number;
  /** A compaction runs. */
  compacting: boolean;
};

/**
 * The conversation's goal (`/goal`), as the agent reports it: what it is
 * after, where it stands, and when it was set and last changed (Unix ms),
 * for its timer. A goal reached or failed stays until the next message.
 * `lastReason`: why it stands there, what a blocked goal shows.
 */
export type Goal = {
  status: "active" | "paused" | "blocked" | "budget_limited" | "complete" | "error";
  objective: string;
  createdAt?: number;
  updatedAt?: number;
  lastReason?: string;
};

/**
 * A command the agent left running in the kernel (`h = bash(cmd)`), or that
 * ended since, as the agent's `_unharnessed/kernel/changed` gives it. A
 * one-shot `await bash(cmd)` is never one.
 */
export type KernelCommand = {
  id: string;
  command: string;
  /** ISO 8601. */
  startedAt: string;
  status: "running" | "finished";
  exitCode: number | null;
  /** Stopped by `kill()`: from the panel, or by the agent. */
  killed: boolean;
  /** The TCP ports it listens on, while it runs. */
  ports: number[];
  /** How long it ran, once finished. */
  durationMs?: number;
};

/**
 * One variable the agent made in its REPL: its name and type, then the
 * value itself when short, a function's signature, or a size. `rev` changes
 * when the name is bound to another object.
 */
export type KernelVariable = {
  name: string;
  type: string;
  rev: string;
  value?: string;
  signature?: string;
  len?: number;
  unit?: "items" | "keys" | "chars" | "bytes";
  shape?: number[];
  dtype?: string;
};

/** The kernel on the panel: the commands still on its list, the variables. */
export type KernelState = { commands: KernelCommand[]; variables: KernelVariable[] };

/** A value's start, as the kernel formats it: `text` wraps, `code` keeps its columns. */
export type VariablePreview =
  | { text: string; style: "code" | "text"; truncated: boolean }
  /** `busy`: a cell runs, the value is not read meanwhile. */
  | { error: string };

export type ChatState = {
  entries: ChatEntry[];
  /** A turn runs: one the panel sent, or one the agent started. */
  busy: boolean;
  /** A saved conversation is coming back; the panel gets it once it is whole. */
  loading?: boolean;
  queue: QueuedMessage[];
  /** Stopped: the waiting messages go once the queue plays again. */
  paused: boolean;
  commands: ChatCommand[];
  /** `null` until the agent has reported it. */
  usage: Usage | null;
  goal: Goal | null;
};

/** What changed since the last post: entries without their items, then the items by entry id and index. */
export type ChatPatch = {
  heads: EntryHead[];
  items: [entry: number, at: number, item: TurnItem][];
  busy: boolean;
  queue: QueuedMessage[];
  paused: boolean;
  commands?: ChatCommand[];
  usage?: Usage | null;
  goal?: Goal | null;
};

/** yusAgent (YA-10, ticket 24, PR I) : les fonctions dont le pont sert les méthodes (`src/ya/methods.ts`). */
export type Features = {
  history: boolean;
  steer: boolean;
  rewind: boolean;
  kernel: boolean;
  providers: boolean;
  settings: boolean;
  /** Liens `@fichier` et images dans le prompt (blocs texte seulement, §5.4). */
  attachments: boolean;
};

export type HostMessage =
  /** Answer to `ready`; `splash` is true once per window. */
  | {
      type: "init";
      splash: boolean;
      /** yusAgent : ce que le panneau montre. */
      features: Features;
      agent: AgentState;
      providers: Provider[] | null;
      login: LoginState | null;
      session: SessionState | null;
      settings: SettingsState;
      chat: ChatState;
      history: HistoryState;
      kernel: KernelState;
    }
  /** Another conversation is in the panel: all of it, and its message box. */
  | {
      type: "conversation";
      agent: AgentState;
      session: SessionState | null;
      chat: ChatState;
      kernel: KernelState;
      draft: Draft;
    }
  | { type: "chat"; chat: ChatState }
  /** yusAgent (tests e2e) : la webview renvoie ce qu'elle affiche. */
  | { type: "probe" }
  | { type: "kernel"; kernel: KernelState }
  /** A command's last lines; `null` when the agent could not give them. */
  | { type: "kernelTail"; id: string; tail: string | null }
  | { type: "kernelPreview"; name: string; preview: VariablePreview }
  | { type: "history"; history: HistoryState }
  | { type: "chatPatch"; patch: ChatPatch }
  /**
   * A sent prompt, back in the message box to be changed and sent again in
   * place of turn `id`'s; or waiting message `id`, to be changed in its place.
   */
  | { type: "chatEdit"; id: number; text: string; images: Attachment[]; queued: boolean }
  | { type: "agent"; agent: AgentState }
  /** `null` while the agent is not running. */
  | { type: "session"; session: SessionState | null }
  /** `providers` is `null` until the agent has listed them. */
  | { type: "providers"; providers: Provider[] | null; login: LoginState | null }
  /** How a saved key, a disconnect or a local server's change went; `error` is shown on the row. */
  | { type: "providerResult"; action: "apiKey" | "logout" | "local"; providerId: string; error?: string }
  /** The local servers at their usual addresses; `null` when the agent could not look. */
  | { type: "localScan"; servers: LocalServer[] | null }
  /** What the server at `baseUrl`, as typed, serves with `apiKey`. */
  | { type: "localProbe"; baseUrl: string; apiKey: string; probe: LocalProbe }
  | { type: "settings"; settings: SettingsState }
  /**
   * How a settings write went; `error` is shown under the setting. MCP and
   * skills actions answer under `mcp:<id>`, `skill:<path>`, `skills/create`
   * and `skills/folder`.
   */
  | { type: "settingResult"; key: string; error?: string }
  /** The folder's files for the `@` menu, relative and `/`-separated; `open` are those in an editor. */
  | { type: "files"; files: string[]; open: string[] }
  /** The images among the files of a `readImages`, base64; one that could not be read is a sentence in `errors`. */
  | { type: "droppedImages"; images: { name: string; mimeType: string; data: string }[]; errors: string[] };

/** yusAgent (tests e2e) : ce que la webview affiche, texte visible et sélecteurs. */
export type Probe = {
  text: string;
  selects: { label: string | null; value: string; disabled: boolean; options: string[] }[];
};

export type WebviewMessage =
  | { type: "ready" }
  /** yusAgent : réponse de la webview à `probe` (tests e2e). */
  | { type: "probeResult"; probe: Probe }
  /** The screen now on display, for the log. */
  | { type: "screen"; screen: Screen }
  /** The panel crashed: what threw, and where. */
  | { type: "crashed"; error: string }
  | { type: "restartAgent" }
  | { type: "setApiKey"; providerId: string; apiKey: string }
  | { type: "logout"; providerId: string }
  | { type: "login"; providerId: string }
  /** Look for the local servers at their usual addresses. */
  | { type: "scanLocal" }
  | { type: "probeLocal"; baseUrl: string; apiKey: string }
  /** `rowId` names the row that answers: the server's id, or the new server's row. */
  | { type: "saveLocal"; rowId: string; server: LocalSave }
  | { type: "removeLocal"; providerId: string }
  | { type: "loginAnswer"; promptId: number; answer: LoginAnswer }
  | { type: "loginCancel" }
  /** Leave a failed sign-in's screen. */
  | { type: "loginClose" }
  /** Open the running sign-in's page in the browser. */
  | { type: "loginOpen" }
  | ({ type: "setConfig" } & ConfigChange)
  /** `null` removes the key: its default applies again. */
  | { type: "setSetting"; key: string; value: unknown }
  /**
   * Save a group of instructions (`index` null: a new one), or delete it
   * (`group` null). `models` are the ones checked among the connected
   * providers' models; the group's others stay as they are.
   */
  | { type: "setInstructions"; index: number | null; group: ModelInstructions | null }
  | { type: "mcpSignIn"; id: string; label: string }
  /** Add a Marketplace service connected by a pasted token. */
  | { type: "mcpSetToken"; id: string; token: string }
  | { type: "mcpSignOut"; id: string }
  | { type: "setSkill"; path: string; enabled: boolean }
  /** The new skill's file opens once it is written. */
  | { type: "createSkill"; name: string; description: string }
  | { type: "skillFolder"; path: string; add: boolean }
  /** Open a skill's file in the editor. */
  | { type: "openFile"; path: string }
  | { type: "copy"; text: string }
  /** A tool's code (its input) or its whole output, in an editor tab. */
  | { type: "openCell"; id: string; part: "input" | "output" }
  /**
   * Sent now, or queued while a turn runs; its `@` mentions go as links.
   * `edit` replaces that turn's prompt: the conversation goes back to before
   * it; or that waiting message, in its place. `now`: into the running turn,
   * which reads it before its next step.
   */
  | { type: "send"; text: string; images: Attachment[]; edit?: number; now?: boolean }
  /** A sent prompt or a waiting message, to change: it comes back as `chatEdit`. */
  | { type: "edit"; id: number }
  /** The waiting message being changed goes back unchanged. */
  | { type: "release"; id: number }
  /** The folder's files, for the `@` menu. */
  | { type: "listFiles" }
  /** Files dropped from VS Code (the explorer, a tab), by URI: the panel can't read them, `droppedImages` brings the images. */
  | { type: "readImages"; uris: string[] }
  /** Stop the running turn; the waiting messages wait, the queue paused. */
  | { type: "cancel" }
  /** The goal's pause (the running turn stops), play, and end (the running turn stops, the goal goes). */
  | { type: "goal"; action: "pause" | "resume" | "end" }
  | { type: "unqueue"; id: number }
  /** A waiting message to place `to` in the queue. */
  | { type: "moveQueued"; id: number; to: number }
  /** A waiting message, now: into the running turn, or sent if none runs. */
  | { type: "sendNow"; id: number }
  /** The paused queue plays again. */
  | { type: "resumeQueue" }
  /** A new conversation, its own agent; `draft` stays with the one the panel leaves. */
  | { type: "newChat"; draft: Draft }
  /** The saved conversations of the panel's folder. */
  | { type: "listChats" }
  /** A saved conversation in the panel: as it is when open, else its agent starts on it. */
  | { type: "openChat"; id: string; draft: Draft }
  | { type: "deleteChat"; id: string }
  /** A link in a message: web pages open in the browser. */
  | { type: "openLink"; url: string }
  /** A background command's last lines, as `kernelTail`. */
  | { type: "kernelTail"; id: string }
  | { type: "kernelKill"; id: string }
  /** A finished command leaves the list. */
  | { type: "kernelDismiss"; id: string }
  /** A variable's value, as `kernelPreview`. */
  | { type: "kernelPreview"; name: string };

export type Screen = "splash" | "status" | "providers" | "login" | "settings";

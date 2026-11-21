// État d'un chat et son réducteur, purs : partagés par l'hôte (qui tient l'état) et la webview
// (qui l'affiche). Aucune dépendance à Node ni à `vscode`.

import type { ConfigOption, SessionUpdate, ToolStatus } from "../acp/types";

export type Phase =
  | "starting" // processus lancé, session pas encore ouverte
  | "ready" // session ouverte, aucun tour
  | "turn" // un prompt est en cours
  | "stopping" // `session/cancel` envoyé, réponse du prompt attendue
  | "stopped" // plus de processus (binaire absent, trop d'arrêts, pas de dossier)
  | "closed"; // onglet fermé

export type ItemStatus = ToolStatus | "cancelled";

export type NoticeAction = "openSetting" | "retry";

export type Item =
  | { kind: "user"; id: number; text: string }
  | { kind: "agent"; id: number; text: string }
  | { kind: "thought"; id: number; text: string }
  | { kind: "tool"; id: number; toolCallId: string; title: string; status: ItemStatus }
  | { kind: "permission"; id: number; requestKey: string; toolCallId: string }
  | { kind: "auth"; id: number; binaryPath: string }
  | { kind: "notice"; id: number; level: "info" | "error"; text: string; actions: NoticeAction[] };

export interface ChatState {
  phase: Phase;
  workspaceName: string | undefined;
  items: Item[];
  /** Les éléments d'indice inférieur viennent d'une session terminée (affichés grisés). */
  staleBefore: number;
  configOptions: ConfigOption[];
  nextId: number;
}

export type Action =
  | { type: "starting" }
  | { type: "started"; configOptions: ConfigOption[] }
  | { type: "noWorkspace" }
  | { type: "spawnFailed"; text: string }
  | { type: "startFailed"; text: string }
  | { type: "userPrompt"; text: string }
  | { type: "update"; update: SessionUpdate }
  | { type: "turnEnded"; stopReason: string }
  | { type: "turnFailed"; text: string }
  | { type: "authFailed"; binaryPath: string }
  | { type: "stopRequested" }
  | { type: "permissionRequested"; requestKey: string; toolCallId: string }
  | { type: "permissionClosed"; requestKey: string }
  | { type: "configOptions"; configOptions: ConfigOption[] }
  | { type: "configFailed"; text: string }
  | { type: "processExited"; description: string; restarting: boolean }
  | { type: "closed" };

export function initialState(workspaceName: string | undefined): ChatState {
  return {
    phase: "starting",
    workspaceName,
    items: [],
    staleBefore: 0,
    configOptions: [],
    nextId: 1,
  };
}

/** Le bandeau D4 suit la valeur annoncée par l'agent (`configOptions`, `config_option_update`). */
export function autoWithoutSandbox(state: ChatState): boolean {
  return state.configOptions.some(
    (option) => option.id === "permissions" && option.currentValue === "auto",
  );
}

/** Sélecteurs affichés : modèle et effort, repérés par la catégorie ACP. */
export function selectors(state: ChatState): ConfigOption[] {
  return ["model", "thought_level"]
    .map((category) => state.configOptions.find((option) => option.category === category))
    .filter((option): option is ConfigOption => option !== undefined);
}

export const TEXTS = {
  noWorkspace: "Ouvrez un dossier : le chat yusAgent démarre dans le dossier du workspace.",
  turnInterrupted: "Tour interrompu par l'arrêt du processus.",
  restarting: (description: string) =>
    `yusAgent s'est arrêté (${description}) : nouvelle session, l'agent ne connaît plus la conversation ci-dessus.`,
  stoppedTooOften: (description: string) =>
    `yusAgent s'est arrêté (${description}), deuxième arrêt en moins de 60 s : pas de relance automatique.`,
  auth: (binaryPath: string) =>
    `Authentification requise : lancez ${binaryPath} dans un terminal, puis /login.`,
  cancelled: "Arrêté.",
};

/** Un élément sans son id (Omit distribué sur chaque variante). */
type NewItem = Item extends infer Variant ? (Variant extends Item ? Omit<Variant, "id"> : never) : never;

function withItem(state: ChatState, item: NewItem): ChatState {
  return {
    ...state,
    items: [...state.items, { ...item, id: state.nextId } as Item],
    nextId: state.nextId + 1,
  };
}

function notice(state: ChatState, level: "info" | "error", text: string, actions: NoticeAction[] = []) {
  return withItem(state, { kind: "notice", level, text, actions });
}

/** Les appels pas finis de la session courante passent à `status` (inchangés si `null`) ;
 * les attentes d'accord partent. */
function settleTools(state: ChatState, status: ItemStatus | null): ChatState {
  const items = state.items
    .filter((item, index) => index < state.staleBefore || item.kind !== "permission")
    .map((item, index) =>
      status !== null &&
      index >= state.staleBefore &&
      item.kind === "tool" &&
      (item.status === "pending" || item.status === "in_progress")
        ? { ...item, status }
        : item,
    );
  return { ...state, items };
}

function appendChunk(state: ChatState, kind: "agent" | "thought", text: string): ChatState {
  if (!text) return state;
  const last = state.items[state.items.length - 1];
  if (last && last.kind === kind && state.items.length - 1 >= state.staleBefore) {
    const items = state.items.slice(0, -1);
    items.push({ ...last, text: last.text + text });
    return { ...state, items };
  }
  return withItem(state, { kind, text });
}

function applyUpdate(state: ChatState, update: SessionUpdate): ChatState {
  switch (update.sessionUpdate) {
    case "agent_message_chunk":
    case "agent_thought_chunk": {
      const content = (update as { content?: { type?: string; text?: string } }).content;
      if (content?.type !== "text" || typeof content.text !== "string") return state;
      return appendChunk(
        state,
        update.sessionUpdate === "agent_message_chunk" ? "agent" : "thought",
        content.text,
      );
    }
    case "tool_call": {
      const call = update as { toolCallId: string; title?: string; status?: ToolStatus };
      const existing = state.items.findIndex(
        (item, index) =>
          index >= state.staleBefore && item.kind === "tool" && item.toolCallId === call.toolCallId,
      );
      if (existing >= 0) return state;
      return withItem(state, {
        kind: "tool",
        toolCallId: call.toolCallId,
        title: call.title || "Tool call",
        status: call.status ?? "in_progress",
      });
    }
    case "tool_call_update": {
      const call = update as { toolCallId: string; status?: ToolStatus };
      if (!call.status) return state;
      const items = state.items.map((item, index) =>
        index >= state.staleBefore &&
        item.kind === "tool" &&
        item.toolCallId === call.toolCallId &&
        item.status !== "cancelled"
          ? { ...item, status: call.status as ItemStatus }
          : item,
      );
      return { ...state, items };
    }
    case "config_option_update": {
      const options = (update as { configOptions?: ConfigOption[] }).configOptions;
      return Array.isArray(options) ? { ...state, configOptions: options } : state;
    }
    default:
      // Le reste (session_info_update, bash_*, user_message_chunk…) n'est pas affiché en PR 2.
      return state;
  }
}

export function reduce(state: ChatState, action: Action): ChatState {
  switch (action.type) {
    case "starting":
      return { ...state, phase: "starting" };
    case "started":
      return { ...state, phase: "ready", configOptions: action.configOptions };
    case "noWorkspace":
      return notice({ ...state, phase: "stopped" }, "error", TEXTS.noWorkspace);
    case "spawnFailed":
      return notice({ ...state, phase: "stopped", configOptions: [] }, "error", action.text, [
        "openSetting",
        "retry",
      ]);
    case "startFailed":
      return notice({ ...state, phase: "stopped", configOptions: [] }, "error", action.text, [
        "retry",
      ]);
    case "userPrompt":
      return withItem({ ...state, phase: "turn" }, { kind: "user", text: action.text });
    case "update":
      return applyUpdate(state, action.update);
    case "turnEnded": {
      const cancelled = action.stopReason === "cancelled";
      let next = settleTools({ ...state, phase: "ready" }, cancelled ? "cancelled" : null);
      if (cancelled) next = notice(next, "info", TEXTS.cancelled);
      return next;
    }
    case "turnFailed":
      return notice(settleTools({ ...state, phase: "ready" }, "failed"), "error", action.text);
    case "authFailed":
      return withItem(settleTools({ ...state, phase: "ready" }, "failed"), {
        kind: "auth",
        binaryPath: action.binaryPath,
      });
    case "stopRequested":
      return state.phase === "turn"
        ? settleTools({ ...state, phase: "stopping" }, "cancelled")
        : state;
    case "permissionRequested":
      return withItem(state, {
        kind: "permission",
        requestKey: action.requestKey,
        toolCallId: action.toolCallId,
      });
    case "permissionClosed":
      return {
        ...state,
        items: state.items.filter(
          (item) => !(item.kind === "permission" && item.requestKey === action.requestKey),
        ),
      };
    case "configOptions":
      return { ...state, configOptions: action.configOptions };
    case "configFailed":
      return notice(state, "error", action.text);
    case "processExited": {
      const wasTurn = state.phase === "turn" || state.phase === "stopping";
      let next = settleTools(state, "failed");
      if (wasTurn) next = notice(next, "error", TEXTS.turnInterrupted);
      next = { ...next, staleBefore: next.items.length, configOptions: [] };
      return action.restarting
        ? notice({ ...next, phase: "starting" }, "info", TEXTS.restarting(action.description))
        : notice({ ...next, phase: "stopped" }, "error", TEXTS.stoppedTooOften(action.description), [
            "retry",
          ]);
    }
    case "closed":
      return { ...state, phase: "closed" };
  }
}

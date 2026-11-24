// yusAgent (YA-10, ticket 24, PR 3) : la boîte de permission, au-dessus de la saisie (C2).
// - Elle montre la première demande ouverte de la conversation (`yaPermissions` de l'hôte), le code
//   de sa cellule (pris dans la carte de l'appel, `pending` ; pas de boîte avant elle), dépliable, caractères invisibles
//   échappés (`visibleCode`), et une action par option du pont (« Allow », « Reject » ; D3).
// - Touches (C3, `src/ya/permission-keys.ts`) : `y`, `n`, `Esc`, seulement quand la boîte a le
//   focus, après 1 s de calme ; jamais Entrée. La boîte prend le focus d'elle-même seulement si la
//   zone de message est vide : un texte en cours de saisie ne peut pas devenir une réponse.
// - Boutons inactifs et grisés pendant 1 s après l'ouverture, sans relance par les frappes.
// - Elle se ferme quand l'hôte retire la demande : réponse, « Stop », `$/cancel_request`, sortie de
//   `pending`.
// Visuel provisoire (passe de design plus tard).
import { useEffect, useRef, useState, useSyncExternalStore, type KeyboardEvent } from "react";

import type { ChatState, PermissionAsk, TurnItem } from "../src/protocol";
import { BUTTONS_DELAY_MS, buttonsReady, QuietKeys, takesFocus, visibleCode } from "../src/ya/permission-keys";
import { boxClosed, boxOpened, type BoxState } from "./ya-box-state";
import { onHostMessage, post } from "./vscode";

/** Lignes du code montrées avant « Show all ». */
const FOLDED_LINES = 8;

let asks: PermissionAsk[] = [];
const listeners = new Set<() => void>();
onHostMessage((message) => {
  if (message.type !== "yaPermissions") return;
  asks = message.asks;
  for (const listener of listeners) listener();
});

function useAsks(): PermissionAsk[] {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => asks,
  );
}

type Tool = Extract<TurnItem, { kind: "tool" }>;

export function PermissionBox({ chat }: { chat: ChatState }) {
  const open = useAsks();
  const ask = open[0];
  // Pas de boîte sans le code : la demande arrive tout de suite, la carte de son appel avec le lot
  // suivant du chat (`src/chat.ts`, `FLUSH_MS`). La fenêtre de calme part à l'affichage.
  const tool = ask && findTool(chat, ask.toolCallId);
  if (!ask || !tool) return null;
  return <Box key={ask.toolCallId} ask={ask} count={open.length} tool={tool} />;
}

function Box({ ask, count, tool }: { ask: PermissionAsk; count: number; tool: Tool }) {
  // Une fenêtre de calme par demande : `key` du composant = l'appel visé.
  const keys = useRef(new QuietKeys(Date.now()));
  const box = useRef<HTMLDivElement>(null);
  // Ticket 31 : l'état lu par la sonde des tests e2e (`ya-box-state.ts`).
  const state = useRef<BoxState | null>(null);
  const [unfolded, setUnfolded] = useState(false);
  const [ready, setReady] = useState(false);
  useEffect(() => {
    const openedAt = Date.now();
    keys.current.reset(openedAt);
    const composer = document.querySelector<HTMLTextAreaElement>("textarea.composer-input");
    if (takesFocus(composer?.value ?? "")) box.current?.focus();
    const opened: BoxState = { keys: keys.current, openedAt, log: [] };
    state.current = opened;
    boxOpened(opened);
    // Un minuteur peut partir un peu avant l'échéance mesurée par `Date.now()` : il repart alors.
    let timer: ReturnType<typeof setTimeout>;
    const tick = () => {
      if (buttonsReady(openedAt, Date.now())) setReady(true);
      else timer = setTimeout(tick, 20);
    };
    timer = setTimeout(tick, BUTTONS_DELAY_MS);
    return () => {
      clearTimeout(timer);
      boxClosed(opened);
    };
  }, []);
  const allow = ask.options.find((option) => option.kind === "allow_once");
  const deny = ask.options.find((option) => option.kind === "reject_once");
  const answer = (optionId: string | undefined) => optionId && post({ type: "permission", toolCallId: ask.toolCallId, optionId });
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const now = Date.now();
    const armed = keys.current.armed(now);
    const result = keys.current.key(event, now);
    state.current?.log.push({ key: event.key, armed, result });
    if (result === "ignored") return;
    event.preventDefault();
    event.stopPropagation();
    answer(result === "allow" ? allow?.optionId : deny?.optionId);
  };
  const code = visibleCode(tool.input);
  const lines = code.split("\n");
  const folded = !unfolded && lines.length > FOLDED_LINES;
  return (
    <div ref={box} className="ya-permission" role="group" aria-label="Permission request" tabIndex={0} onKeyDown={onKeyDown}>
      <div className="ya-permission-head">
        <strong>Allow this {tool.title} to run?</strong>
        {count > 1 && <span className="call-meta">1 of {count}</span>}
      </div>
      <pre className="call-code ya-permission-code">{folded ? lines.slice(0, FOLDED_LINES).join("\n") : code}</pre>
      {lines.length > FOLDED_LINES && (
        <button className="ya-permission-fold" type="button" onClick={() => setUnfolded(!unfolded)}>
          {folded ? `Show all ${lines.length} lines` : "Show less"}
        </button>
      )}
      <div className="ya-permission-actions">
        {ask.options.map((option) => (
          <button
            key={option.optionId}
            className={option.kind === "allow_once" ? "button" : "button is-secondary"}
            type="button"
            disabled={!ready}
            onClick={() => answer(option.optionId)}
          >
            {option.name}
          </button>
        ))}
        <span className="note ya-permission-keys">y allows · n or Esc rejects, once this box has focus</span>
      </div>
    </div>
  );
}

function findTool(chat: ChatState, toolCallId: string): Tool | undefined {
  for (let at = chat.entries.length - 1; at >= 0; at--) {
    const entry = chat.entries[at];
    if (entry.kind !== "turn") continue;
    const tool = entry.items.find((item): item is Tool => item.kind === "tool" && item.id === toolCallId);
    if (tool) return tool;
  }
  return undefined;
}

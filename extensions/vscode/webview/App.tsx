import { useEffect, useRef, useState, type KeyboardEvent } from "react";

import type { ToHost } from "../src/shared/messages";
import {
  autoWithoutSandbox,
  selectors,
  TEXTS,
  type ChatState,
  type Item,
  type ItemStatus,
  type NoticeAction,
} from "../src/shared/state";

const STATUS_LABEL: Record<ItemStatus, string> = {
  pending: "en attente d'accord",
  in_progress: "en cours",
  completed: "terminé",
  failed: "échoué",
  cancelled: "annulé",
};

const ACTION_LABEL: Record<NoticeAction, string> = {
  openSetting: "Ouvrir le réglage yusagent.binaryPath",
  retry: "Réessayer",
};

const PHASE_LABEL: Record<ChatState["phase"], string> = {
  starting: "démarrage…",
  ready: "prêt",
  turn: "en cours…",
  stopping: "arrêt…",
  stopped: "arrêté",
  closed: "fermé",
};

export function App({ state, post }: { state: ChatState; post: (message: ToHost) => void }) {
  const [draft, setDraft] = useState("");
  const bottom = useRef<HTMLDivElement>(null);
  const busy = state.phase === "turn" || state.phase === "stopping";

  useEffect(() => {
    bottom.current?.scrollIntoView({ block: "end" });
  }, [state.items]);

  const send = () => {
    if (state.phase !== "ready" || !draft.trim()) return;
    post({ type: "send", text: draft });
    setDraft("");
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      send();
    }
  };

  return (
    <div className="chat">
      <header className="header">
        <span className="workspace">{state.workspaceName ?? "aucun dossier"}</span>
        <span className="phase">{PHASE_LABEL[state.phase]}</span>
      </header>
      {autoWithoutSandbox(state) && (
        <div className="banner" role="alert">
          ⚠ Mode auto sans sandbox : les cellules s'exécutent sans demander, avec vos droits.
        </div>
      )}
      <main className="messages">
        {state.items.map((item, index) => (
          <ItemView
            key={item.id}
            item={item}
            stale={index < state.staleBefore}
            post={post}
          />
        ))}
        <div ref={bottom} />
      </main>
      <footer className="composer">
        <textarea
          className="input"
          value={draft}
          placeholder={state.phase === "ready" ? "Message à yusAgent (Entrée pour envoyer, Maj+Entrée pour un saut de ligne)" : ""}
          disabled={state.phase === "stopped" || state.phase === "closed"}
          rows={3}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={onKeyDown}
        />
        <div className="controls">
          {selectors(state).map((option) => (
            <select
              key={option.id}
              className="selector"
              aria-label={option.name}
              title={option.name}
              value={option.currentValue}
              disabled={state.phase !== "ready"}
              onChange={(event) =>
                post({ type: "setConfig", configId: option.id, value: event.target.value })
              }
            >
              {option.options.map((choice) => (
                <option key={choice.value} value={choice.value}>
                  {choice.name}
                </option>
              ))}
            </select>
          ))}
          <span className="spacer" />
          {busy ? (
            <button
              className="button stop"
              disabled={state.phase === "stopping"}
              onClick={() => post({ type: "stop" })}
            >
              Arrêter
            </button>
          ) : (
            <button
              className="button"
              disabled={state.phase !== "ready" || !draft.trim()}
              onClick={send}
            >
              Envoyer
            </button>
          )}
        </div>
      </footer>
    </div>
  );
}

function ItemView({ item, stale, post }: { item: Item; stale: boolean; post: (message: ToHost) => void }) {
  const className = stale ? "item stale" : "item";
  switch (item.kind) {
    case "user":
      return <div className={`${className} user`}>{item.text}</div>;
    case "agent":
      return <div className={`${className} agent`}>{item.text}</div>;
    case "thought":
      return (
        <details className={`${className} thought`}>
          <summary>Raisonnement</summary>
          <div className="thought-text">{item.text}</div>
        </details>
      );
    case "tool":
      return (
        <div className={`${className} tool status-${item.status}`}>
          {item.title} · {STATUS_LABEL[item.status]}
        </div>
      );
    case "permission":
      return (
        <div className={`${className} permission`}>
          Demande d'autorisation en attente (boîte en PR 3) : Arrêter pour annuler.
        </div>
      );
    case "auth":
      return <div className={`${className} notice error`}>{TEXTS.auth(item.binaryPath)}</div>;
    case "notice":
      return (
        <div className={`${className} notice ${item.level}`}>
          <div>{item.text}</div>
          {!stale && item.actions.length > 0 && (
            <div className="actions">
              {item.actions.map((action) => (
                <button key={action} className="button secondary" onClick={() => post({ type: action })}>
                  {ACTION_LABEL[action]}
                </button>
              ))}
            </div>
          )}
        </div>
      );
  }
}

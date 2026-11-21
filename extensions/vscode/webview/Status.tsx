import { useFeatures } from "./ya-features"; // yusAgent
import { PermissionBox } from "./ya-permission"; // yusAgent (PR 3)
import { YaBusyContext } from "./ya-cells"; // yusAgent (PR 3, étape 4)
import { useEffect, useState } from "react";
import type { AgentState, ChatState, HistoryState, KernelState, Provider, SessionState } from "../src/protocol";
import { Transcript } from "./Chat";
import { Composer, type Draft, type ImageSettings } from "./Composer";
import { HistoryMenu } from "./History";
import { IconCross, IconHistory, IconNewChat, IconPlug, IconRestart, IconSliders } from "./icons";
import { post } from "./vscode";
import { Wordmark } from "./Wordmark";

type Failed = Extract<AgentState, { kind: "failed" }>;

/** Said only while the agent is not there to talk to: a running agent goes without saying. */
const LABELS: Record<Exclude<AgentState["kind"], "ready">, string> = {
  starting: "Starting agent",
  failed: "Agent stopped",
};

const restart = () => post({ type: "restartAgent" });

/** Home: the conversation, the agent's state while it is not ready, and the message box. */
export function Status({
  agent,
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
  onSettings,
  history,
}: {
  agent: AgentState;
  session: SessionState | null;
  providers: Provider[];
  /** No provider is connected. */
  unconnected: boolean;
  chat: ChatState;
  kernel: KernelState;
  images: ImageSettings;
  draft: Draft;
  onDraft: (update: (draft: Draft) => Draft) => void;
  /** The turn whose prompt is being changed in the message box. */
  editing: number | null;
  onEditEnd: () => void;
  onProviders: () => void;
  onSettings: () => void;
  history: HistoryState;
}) {
  const [browsing, setBrowsing] = useState(false);
  // yusAgent : entrées masquées tant que le pont ne sert pas leurs méthodes ; bandeau D4.
  const features = useFeatures();
  const auto = session?.kind === "open" && session.config.permissions === "auto";
  // The agent lists the conversations: the menu closes when it stops.
  useEffect(() => {
    if (agent.kind !== "ready") setBrowsing(false);
  }, [agent.kind]);
  // A saved conversation coming back: the agent restarts on it, then replays it.
  const kind = agent.kind === "failed" ? "failed" : chat.loading || agent.kind === "starting" ? "starting" : null;
  const label = kind === "starting" && chat.loading ? "Opening the conversation" : kind && LABELS[kind];
  return (
    <div className="screen">
      <header className="topbar">
        <Wordmark />
        <div className="topbar-actions">
          <button
            className="icon-button"
            type="button"
            aria-label="Providers"
            hidden={!features.providers}
            data-tip="Providers"
            data-tip-side="below"
            disabled={agent.kind !== "ready"}
            onClick={onProviders}
          >
            <IconPlug />
          </button>
          <button
            className="icon-button"
            type="button"
            aria-label="Settings"
            hidden={!features.settings}
            data-tip="Settings"
            data-tip-side="below"
            disabled={agent.kind !== "ready"}
            onClick={onSettings}
          >
            <IconSliders />
          </button>
          <button
            className={browsing ? "icon-button is-open" : "icon-button"}
            type="button"
            aria-label="History"
            hidden={!features.history}
            aria-haspopup="menu"
            aria-expanded={browsing}
            data-tip={browsing ? undefined : "History"}
            data-tip-side="below"
            data-menu-trigger="history"
            disabled={agent.kind !== "ready"}
            onClick={() => setBrowsing(!browsing)}
          >
            <IconHistory />
            {history.unread && <span className="icon-dot" />}
          </button>
          <button
            className="icon-button"
            type="button"
            aria-label="New conversation"
            data-tip="New conversation"
            data-tip-side="below"
            data-tip-align="end"
            disabled={agent.kind === "starting"}
            onClick={() => post({ type: "newChat", draft })}
          >
            <IconNewChat />
          </button>
        </div>
        {browsing && <HistoryMenu history={history} draft={draft} onClose={() => setBrowsing(false)} />}
      </header>
      {auto && (
        <div className="ya-banner" role="alert">
          ⚠ Mode auto sans sandbox : les cellules s'exécutent sans demander, avec vos droits.
        </div>
      )}
      {/* yusAgent (PR 3, étape 4) : « Revert » grisé pendant un tour. */}
      <YaBusyContext.Provider value={chat.busy}>
      <Transcript chat={chat} editing={editing}>
        {kind && (
          <div className={`agent-line is-${kind}`}>
            {kind === "failed" ? <IconCross className="agent-mark" /> : <span className="agent-mark" />}
            <span>{label}</span>
          </div>
        )}
        {agent.kind === "failed" && <Failure agent={agent} />}
      </Transcript>
      </YaBusyContext.Provider>
      {/* yusAgent (PR 3, C2) : la boîte de permission, au-dessus de la saisie. */}
      {agent.kind === "ready" && <PermissionBox chat={chat} />}
      {agent.kind === "ready" && (
        <Composer
          session={session}
          providers={providers}
          unconnected={unconnected}
          chat={chat}
          kernel={kernel}
          images={images}
          draft={draft}
          onDraft={onDraft}
          editing={editing}
          onEditEnd={onEditEnd}
          onProviders={onProviders}
        />
      )}
    </div>
  );
}

function Failure({ agent }: { agent: Failed }) {
  return (
    <>
      <p className="error">{agent.error}</p>
      {agent.stderr.length > 0 && <pre className="log">{agent.stderr.join("\n")}</pre>}
      <button className="button" type="button" onClick={restart}>
        <IconRestart />
        Restart
      </button>
    </>
  );
}

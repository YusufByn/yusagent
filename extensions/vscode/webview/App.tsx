import { useCallback, useEffect, useRef, useState } from "react";
import type {
  AgentState,
  ChatState,
  HistoryState,
  HostMessage,
  KernelState,
  LoginState,
  Provider,
  Screen,
  SessionState,
  SettingsState,
} from "../src/protocol";
import { applyPatch, EMPTY_CHAT } from "./Chat";
import type { Draft, ImageSettings } from "./Composer";
import { PathsContext, toPaths, type Paths } from "./mentions";
import { Providers } from "./Providers";
import { Settings, type SettingsTab } from "./Settings";
import { SignIn } from "./SignIn";
import { Splash } from "./Splash";
import { Status } from "./Status";
import { onHostMessage, post, saveDraft, savedDraft } from "./vscode";

type Phase = "boot" | "splash" | "main";
export type ProviderResult = Extract<HostMessage, { type: "providerResult" }>;
type LocalScan = Extract<HostMessage, { type: "localScan" }>;
type LocalProbe = Extract<HostMessage, { type: "localProbe" }>;
export type SettingResult = Extract<HostMessage, { type: "settingResult" }>;

export function App() {
  const [phase, setPhase] = useState<Phase>("boot");
  const [agent, setAgent] = useState<AgentState | null>(null);
  const [providers, setProviders] = useState<Provider[] | null>(null);
  const [login, setLogin] = useState<LoginState | null>(null);
  const [session, setSession] = useState<SessionState | null>(null);
  const [result, setResult] = useState<ProviderResult | null>(null);
  const [localScan, setLocalScan] = useState<LocalScan | null>(null);
  const [localProbe, setLocalProbe] = useState<LocalProbe | null>(null);
  const [settings, setSettings] = useState<SettingsState>({ view: null, pending: [], mcp: null, skills: null });
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("general");
  const [settingResult, setSettingResult] = useState<SettingResult | null>(null);
  const [chat, setChat] = useState<ChatState>(EMPTY_CHAT);
  const [history, setHistory] = useState<HistoryState>({ chats: null, unread: false });
  const [kernel, setKernel] = useState<KernelState>({ commands: [], variables: [] });
  const [paths, setPaths] = useState<Paths | null>(null);
  const [draft, setDraft] = useState<Draft>(() => ({ text: savedDraft(), images: [] }));
  /**
   * A sent prompt being changed in the message box, or a waiting message
   * (`queued`), and the draft it pushed out, back once the edit ends.
   */
  const [editing, setEditing] = useState<{ id: number; saved: Draft; queued: boolean } | null>(null);
  const draftNow = useRef(draft);
  useEffect(() => {
    draftNow.current = draft;
  }, [draft]);
  const editingNow = useRef(editing);
  useEffect(() => {
    editingNow.current = editing;
  }, [editing]);
  const [managing, setManaging] = useState(false);
  const [tuning, setTuning] = useState(false);
  const splashDone = useCallback(() => setPhase("main"), []);
  useEffect(() => saveDraft(draft.text), [draft.text]);

  useEffect(() => {
    const off = onHostMessage((message) => {
      switch (message.type) {
        case "init":
          setAgent(message.agent);
          setProviders(message.providers);
          setLogin(message.login);
          setSession(message.session);
          setSettings(message.settings);
          setChat(message.chat);
          setHistory(message.history);
          setKernel(message.kernel);
          setPhase(message.splash ? "splash" : "main");
          break;
        case "conversation":
          // Another conversation, with its own message box.
          setAgent(message.agent);
          setSession(message.session);
          setChat(message.chat);
          setKernel(message.kernel);
          setEditing(null);
          setDraft(message.draft);
          break;
        case "kernel":
          setKernel(message.kernel);
          break;
        case "chat":
          setChat(message.chat);
          break;
        case "chatPatch":
          setChat((chat) => applyPatch(chat, message.patch));
          break;
        case "history":
          setHistory(message.history);
          break;
        case "chatEdit": {
          const { id, text, images, queued } = message;
          const was = editingNow.current;
          // The waiting message changed before stays as it was.
          if (was?.queued && was.id !== id) post({ type: "release", id: was.id });
          setEditing({ id, saved: was?.saved ?? draftNow.current, queued });
          setDraft({ text, images });
          break;
        }
        case "agent":
          setAgent(message.agent);
          break;
        case "session":
          setSession(message.session);
          break;
        case "providers":
          setProviders(message.providers);
          setLogin(message.login);
          break;
        case "providerResult":
          setResult(message);
          break;
        case "localScan":
          setLocalScan(message);
          break;
        case "localProbe":
          setLocalProbe(message);
          break;
        case "settings":
          setSettings(message.settings);
          break;
        case "settingResult":
          setSettingResult(message);
          break;
        case "files":
          setPaths(toPaths(message.files, message.open));
          break;
      }
    });
    post({ type: "ready" });
    return off;
  }, []);

  const endEdit = () => {
    if (editing?.queued) post({ type: "release", id: editing.id });
    if (editing) setDraft(editing.saved);
    setEditing(null);
  };
  // The edited turn or waiting message left (a new conversation, a restarted agent): the text stays, as a new message.
  useEffect(() => {
    if (!editing) return;
    const turn = chat.entries.find((entry) => entry.id === editing.id);
    const left = editing.queued
      ? !chat.queue.some((message) => message.id === editing.id)
      : turn?.kind !== "turn" || turn.rewindTo === undefined;
    if (left) setEditing(null);
  }, [chat.entries, chat.queue, editing]);

  // Nothing connected: home's model picker leads to the providers screen.
  const unconnected = providers !== null && !providers.some((p) => p.connected);
  let screen: Screen | null = null;
  if (phase === "splash") screen = "splash";
  else if (phase === "main" && agent) {
    if (agent.kind === "ready" && login) screen = "login";
    else if (agent.kind === "ready" && managing) screen = "providers";
    // It stays while a restart it asked for runs.
    else if (tuning && agent.kind !== "failed") screen = "settings";
    else screen = "status";
  }

  useEffect(() => {
    if (screen) post({ type: "screen", screen });
  }, [screen]);

  if (!screen || !agent) return null;
  switch (screen) {
    case "splash":
      return <Splash agent={agent} onDone={splashDone} />;
    case "login":
      return login && <SignIn login={login} />;
    case "providers":
      return (
        <Providers
          providers={providers ?? []}
          result={result}
          local={{ scan: localScan, probe: localProbe }}
          onBack={() => setManaging(false)}
        />
      );
    case "settings":
      return (
        <Settings
          settings={settings}
          ready={agent.kind === "ready"}
          session={session}
          providers={providers ?? []}
          result={settingResult}
          tab={settingsTab}
          onTab={setSettingsTab}
          onBack={() => setTuning(false)}
        />
      );
    default:
      return (
        <PathsContext.Provider value={paths}>
          <Status
            agent={agent}
            session={session}
            providers={providers ?? []}
            unconnected={unconnected}
            chat={chat}
            kernel={kernel}
            images={imageSettings(settings)}
            draft={draft}
            onDraft={setDraft}
            editing={editing?.id ?? null}
            onEditEnd={endEdit}
            onProviders={() => setManaging(true)}
            onSettings={() => setTuning(true)}
            history={history}
          />
        </PathsContext.Provider>
      );
  }
}

/** The image settings as the agent applies them: the file's value, else the agent's default. */
function imageSettings({ view }: SettingsState): ImageSettings {
  const value = (key: string) => view?.values[key] ?? view?.defaults[key] ?? null;
  return {
    block: value("images.blockImages") === true,
    resize: value("images.autoResize") !== false,
    imageModel: typeof value("imageModel") === "string" ? (value("imageModel") as string) : null,
  };
}

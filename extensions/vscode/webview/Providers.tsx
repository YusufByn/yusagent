import { useEffect, useState, type ReactNode, type SubmitEvent } from "react";
import type { Provider } from "../src/protocol";
import type { ProviderResult } from "./App";
import { IconBack, IconChevron, IconPlus, IconSearch, IconUnlink } from "./icons";
import { LocalForm, NEW_LOCAL, type LocalReplies } from "./Local";
import { ProviderAvatar } from "./logos";
import { post } from "./vscode";

/** From this many rows on, a search field filters them. */
const SEARCH_FROM = 9;

/**
 * The row whose form is open: its key form, the confirmation before a
 * sign-in that replaces a credential, the one before a disconnect, or a
 * local server's form.
 */
type Open = { id: string; method: Provider["method"]; mode: "key" | "signin" | "disconnect" | "local" };

type Run = (message: Parameters<typeof post>[0]) => void;

/** How a row is connected, or why it is not. */
function status(provider: Provider): ReactNode {
  const { note } = provider;
  if (provider.connected) {
    let how: string;
    if (note?.startsWith("env: ")) how = `with ${note.slice(5)} from your environment`;
    else if (note) how = `via ${note}`;
    else how = provider.method === "subscription" ? "with your subscription" : "with a saved API key";
    return (
      <>
        <span className="row-status">Connected</span> {how}
      </>
    );
  }
  switch (note) {
    case "subscription configured":
      return "The subscription sign-in is used instead";
    case "API key configured":
      return "The saved API key is used instead";
    case "expired":
      return "Sign-in expired";
    default:
      return note;
  }
}

/**
 * Every provider the agent knows, in four groups: connected, the servers on
 * this machine (local), subscriptions (a browser sign-in) and API keys (a
 * key pasted here). While nothing is connected, it says what to do first.
 */
export function Providers({
  providers,
  result,
  local,
  onBack,
}: {
  providers: Provider[];
  result: ProviderResult | null;
  local: LocalReplies;
  onBack: () => void;
}) {
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState<Open | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  // The host answers each key save or disconnect with a result.
  useEffect(() => {
    if (!result) return;
    setBusy(false);
    if (result.error) setError(result.error);
    else {
      setOpen(null);
      setError(undefined);
    }
  }, [result]);

  const toggle = (next: Open) => {
    const same = open?.id === next.id && open.method === next.method && open.mode === next.mode;
    setOpen(same ? null : next);
    setBusy(false);
    setError(undefined);
  };
  const run: Run = (message) => {
    setBusy(true);
    setError(undefined);
    post(message);
  };
  const modeOf = (provider: Provider) =>
    open?.id === provider.id && open.method === provider.method ? open.mode : null;

  const words = query.trim().toLowerCase();
  const shown = words ? providers.filter((p) => `${p.name} ${p.id}`.toLowerCase().includes(words)) : providers;
  const servers = shown.filter((p) => p.local);
  const others = shown.filter((p) => !p.local);
  const groups: [string, Provider[]][] = [
    ["Connected", others.filter((p) => p.connected)],
    ["Subscriptions", others.filter((p) => !p.connected && p.method === "subscription")],
    ["API keys", others.filter((p) => !p.connected && p.method === "apiKey")],
  ];
  const adding = open?.id === NEW_LOCAL;

  const group = ([label, rows]: [string, Provider[]]) =>
    rows.length > 0 && (
      <section className="section" key={label}>
        <h2 className="section-label">{label}</h2>
        <ul className="list">
          {rows.map((provider) => {
            // Anthropic and xAI: the other kind of credential, in the same slot.
            const other = providers.find((p) => p.id === provider.id && p.method !== provider.method && p.connected);
            return (
              <Row
                key={`${provider.id}/${provider.method}`}
                provider={provider}
                other={other}
                mode={modeOf(provider)}
                busy={busy}
                error={modeOf(provider) ? error : undefined}
                onToggle={(mode) => toggle({ id: provider.id, method: provider.method, mode })}
                onRun={run}
              />
            );
          })}
        </ul>
      </section>
    );

  return (
    <div className="screen">
      <header className="topbar">
        <div className="topbar-title">
          <button
            className="icon-button"
            type="button"
            aria-label="Back"
            data-tip="Back"
            data-tip-side="below"
            data-tip-align="start"
            onClick={onBack}
          >
            <IconBack />
          </button>
          <span>Providers</span>
        </div>
      </header>
      <main className="content">
        {!providers.some((provider) => provider.connected) && (
          <div className="intro">
            <h1>Connect a provider</h1>
            <p>Sign in with a subscription, add an API key, or use a model on this machine.</p>
          </div>
        )}
        {providers.length >= SEARCH_FROM && (
          <label className="search">
            <IconSearch />
            <input
              className="input"
              type="search"
              placeholder="Search providers"
              aria-label="Search providers"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
          </label>
        )}
        {group(groups[0])}
        {(!words || servers.length > 0) && (
          <section className="section">
            <h2 className="section-label">Local</h2>
            <ul className="list">
              {servers.map((provider) => (
                <LocalRow
                  key={provider.id}
                  provider={provider}
                  replies={local}
                  mode={modeOf(provider)}
                  busy={busy}
                  error={modeOf(provider) ? error : undefined}
                  onToggle={(mode) => toggle({ id: provider.id, method: provider.method, mode })}
                  onRun={run}
                />
              ))}
              {!words && (
                <li className="row">
                  <div className="row-line">
                    <button
                      className={`row-body is-button${adding ? " is-open" : ""}`}
                      type="button"
                      aria-expanded={adding}
                      onClick={() => toggle({ id: NEW_LOCAL, method: "apiKey", mode: "local" })}
                    >
                      <span className="badge">
                        <IconPlus />
                      </span>
                      <span className="row-main">
                        <span className="row-name">Add a local model</span>
                        <span className="row-meta">Ollama, LM Studio, llama.cpp, vLLM</span>
                      </span>
                      <IconChevron className="row-end" />
                    </button>
                  </div>
                  {adding && (
                    <LocalForm
                      replies={local}
                      busy={busy}
                      error={error}
                      onSave={(server) => run({ type: "saveLocal", rowId: NEW_LOCAL, server })}
                      onCancel={() => toggle({ id: NEW_LOCAL, method: "apiKey", mode: "local" })}
                    />
                  )}
                </li>
              )}
            </ul>
          </section>
        )}
        {groups.slice(1).map(group)}
        {shown.length === 0 && <p className="note">No provider matches “{query.trim()}”.</p>}
      </main>
    </div>
  );
}

/** A server on this machine: its models, picked again in its form; removed with its models. */
function LocalRow({
  provider,
  replies,
  mode,
  busy,
  error,
  onToggle,
  onRun,
}: {
  provider: Provider;
  replies: LocalReplies;
  mode: Open["mode"] | null;
  busy: boolean;
  error?: string;
  onToggle: (mode: Open["mode"]) => void;
  onRun: Run;
}) {
  return (
    <li className="row">
      <div className="row-line">
        <button
          className={`row-body is-button${mode === "local" ? " is-open" : ""}`}
          type="button"
          aria-expanded={mode === "local"}
          onClick={() => onToggle("local")}
        >
          <ProviderAvatar className="badge is-on" provider={provider.id} name={provider.name} local />
          <span className="row-main">
            <span className="row-name">{provider.name}</span>
            <span className="row-meta">{provider.local?.models.join(", ")}</span>
          </span>
          <IconChevron className="row-end" />
        </button>
        <button
          className="icon-button"
          type="button"
          data-tip="Remove"
          data-tip-align="end"
          aria-label={`Remove ${provider.name}`}
          onClick={() => onToggle("disconnect")}
        >
          <IconUnlink />
        </button>
      </div>
      {mode === "local" && (
        <LocalForm
          provider={provider}
          replies={replies}
          busy={busy}
          error={error}
          onSave={(server) => onRun({ type: "saveLocal", rowId: provider.id, server })}
          onCancel={() => onToggle("local")}
        />
      )}
      {mode === "disconnect" && (
        <div className="row-form">
          <p className="note">Remove {provider.name} and its models from the model menu?</p>
          {error && <p className="error">{error}</p>}
          <div className="actions">
            <button
              className="button is-danger"
              type="button"
              disabled={busy}
              onClick={() => onRun({ type: "removeLocal", providerId: provider.id })}
            >
              Remove
            </button>
            <button className="button is-quiet" type="button" onClick={() => onToggle("disconnect")}>
              Cancel
            </button>
          </div>
        </div>
      )}
    </li>
  );
}

function Row({
  provider,
  other,
  mode,
  busy,
  error,
  onToggle,
  onRun,
}: {
  provider: Provider;
  /** The provider's other credential, when that one is connected. */
  other?: Provider;
  mode: Open["mode"] | null;
  busy: boolean;
  error?: string;
  onToggle: (mode: Open["mode"]) => void;
  onRun: Run;
}) {
  const browser = provider.login === "browser";
  const meta = status(provider);
  // A sign-in that would replace a working credential asks first.
  const activate = () => {
    if (!browser) onToggle("key");
    else if (provider.connected || other) onToggle("signin");
    else post({ type: "login", providerId: provider.id });
  };

  return (
    <li className="row">
      <div className="row-line">
        <button
          className={`row-body is-button${mode === "key" || mode === "signin" ? " is-open" : ""}`}
          type="button"
          aria-expanded={mode === "key" || mode === "signin"}
          onClick={activate}
        >
          <ProviderAvatar
            className={provider.connected ? "badge is-on" : "badge"}
            provider={provider.id}
            name={provider.name}
          />
          <span className="row-main">
            <span className="row-name">{provider.name}</span>
            {meta && <span className="row-meta">{meta}</span>}
          </span>
          <IconChevron className="row-end" />
        </button>
        {provider.removable && (
          <button
            className="icon-button"
            type="button"
            data-tip="Disconnect"
            data-tip-align="end"
            aria-label={`Disconnect ${provider.name}`}
            onClick={() => onToggle("disconnect")}
          >
            <IconUnlink />
          </button>
        )}
      </div>
      {provider.warning && <p className="row-warning">{provider.warning}</p>}
      {mode === "signin" && (
        <div className="row-form">
          <p className="note">
            {provider.connected
              ? `You are signed in to ${provider.name}. Sign in again to switch accounts or renew the sign-in.`
              : other?.removable
                ? `Signing in replaces the saved ${other.name} API key.`
                : `Once you are signed in, the subscription is used instead of the ${other?.name} API key.`}
          </p>
          <div className="actions">
            <button
              className="button"
              type="button"
              onClick={() => {
                onToggle("signin");
                post({ type: "login", providerId: provider.id });
              }}
            >
              {provider.connected ? "Sign in again" : "Sign in"}
            </button>
            <button className="button is-quiet" type="button" onClick={() => onToggle("signin")}>
              Cancel
            </button>
          </div>
        </div>
      )}
      {mode === "key" && (
        <KeyForm
          provider={provider}
          other={other}
          busy={busy}
          error={error}
          onSave={(apiKey) => onRun({ type: "setApiKey", providerId: provider.id, apiKey })}
          onCancel={() => onToggle("key")}
        />
      )}
      {mode === "disconnect" && (
        <div className="row-form">
          <p className="note">
            {provider.method === "subscription"
              ? `Sign out of ${provider.name}?`
              : `Remove the saved ${provider.name} API key?`}
          </p>
          {error && <p className="error">{error}</p>}
          <div className="actions">
            <button
              className="button is-danger"
              type="button"
              disabled={busy}
              onClick={() => onRun({ type: "logout", providerId: provider.id })}
            >
              Disconnect
            </button>
            <button className="button is-quiet" type="button" onClick={() => onToggle("disconnect")}>
              Cancel
            </button>
          </div>
        </div>
      )}
    </li>
  );
}

function KeyForm({
  provider,
  other,
  busy,
  error,
  onSave,
  onCancel,
}: {
  provider: Provider;
  other?: Provider;
  busy: boolean;
  error?: string;
  onSave: (apiKey: string) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState("");
  const empty = value.trim() === "";
  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    if (!empty && !busy) onSave(value);
  };

  // What saving does to the key already in use.
  let replaces: string | undefined;
  if (provider.connected && provider.note?.startsWith("env: ")) {
    replaces = `${provider.note.slice(5)} from your environment is in use. A key saved here is used instead.`;
  } else if (provider.connected && provider.removable) {
    replaces = "A key is saved. A new one replaces it.";
  } else if (other) {
    replaces = `Saving a key replaces the ${other.name} sign-in.`;
  }

  return (
    <form className="row-form" onSubmit={submit} onKeyDown={(event) => event.key === "Escape" && onCancel()}>
      {replaces && <p className="note">{replaces}</p>}
      <input
        className="input is-secret"
        type="password"
        placeholder={`${provider.name} API key`}
        aria-label={`${provider.name} API key`}
        autoComplete="off"
        spellCheck={false}
        autoFocus
        disabled={busy}
        value={value}
        onChange={(event) => setValue(event.target.value)}
      />
      {error && <p className="error">{error}</p>}
      <div className="actions">
        <button className="button" type="submit" disabled={empty || busy}>
          {busy ? "Saving" : "Save"}
        </button>
        <button className="button is-quiet" type="button" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </form>
  );
}

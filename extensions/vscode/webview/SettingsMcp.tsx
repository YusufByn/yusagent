import { useState, type ReactNode, type SubmitEvent } from "react";
import type { McpServer } from "../src/protocol";
import { IconChevron, IconPlus, IconSearch } from "./icons";
import { McpLogo, ServiceAvatar } from "./logos";
import { Switch, useSettled, type Actions, type Write } from "./Settings";
import { post } from "./vscode";

/** One entry of the `mcpServers` setting, as the file holds it. */
type ServerConfig = Record<string, unknown>;

/** The name the agent addresses a server by. */
const NAME = /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/;

/**
 * The MCP servers: yours (the `mcpServers` setting and the services you
 * added), then the Marketplace, Prime's catalog of services to add in one
 * click. The agent calls their tools from its Python kernel; it keeps the
 * servers and sign-ins it started with.
 */
export function McpTab({
  servers,
  config,
  saved,
  actions,
  write,
}: {
  servers: McpServer[] | null;
  /** The `mcpServers` setting of the global file, with the change being written. */
  config: Record<string, ServerConfig>;
  /** The same as the file holds it. */
  saved: Record<string, ServerConfig>;
  actions: Actions;
  write: Write;
}) {
  // The row whose details are open, or `+` for a new server.
  const [open, setOpen] = useState<string | null>(null);
  if (!servers) return <p className="note">Reading the MCP servers…</p>;

  const listed = new Map(servers.map((server) => [server.id, server]));
  // The file's servers, a server being added before the list has it, and
  // the ones a workspace's settings add; not one being removed.
  const yours = [
    ...Object.keys(config),
    ...servers
      .filter((server) => server.source === "user" && !(server.id in config) && !(server.id in saved))
      .map((server) => server.id),
  ];
  // A catalog service is yours once signed in, or its token stored.
  const services = servers.filter((server) => server.source === "catalog" && server.signedIn);
  const market = servers
    .filter((server) => server.source === "catalog" && !server.signedIn)
    .sort((a, b) => a.label.localeCompare(b.label));
  const toggle = (id: string) => setOpen((current) => (current === id ? null : id));
  const save = (previous: string | null, id: string, next: ServerConfig) => {
    const map = { ...config };
    if (previous !== null && previous !== id) delete map[previous];
    map[id] = next;
    write("mcpServers", map);
  };
  const remove = (id: string) => {
    const { [id]: _, ...rest } = config;
    write("mcpServers", rest);
    if (listed.get(id)?.signedIn) signOut(id);
    setOpen(null);
  };
  const signOut = (id: string) => actions.act(`mcp:${id}`, true, { type: "mcpSignOut", id });

  return (
    <>
      <p className="note section-help">
        The agent calls MCP servers' tools from its Python kernel. Changes reach the agent with your next message.
      </p>
      <section className="section">
        <h2 className="section-label">Your servers</h2>
        <ul className="list">
          {yours.map((id) => (
            <UserServer
              key={id}
              id={id}
              server={listed.get(id)}
              config={config[id]}
              taken={Object.keys(config)}
              open={open === id}
              actions={actions}
              onToggle={() => toggle(id)}
              onSave={(name, next) => save(id, name, next)}
              onEnable={(on) => {
                const { enabled: _, ...rest } = config[id];
                save(id, id, on ? rest : { ...rest, enabled: false });
              }}
              onRemove={() => remove(id)}
              onSignOut={() => signOut(id)}
            />
          ))}
          {services.map((server) => (
            <Service
              key={server.id}
              server={server}
              open={open === server.id}
              actions={actions}
              onToggle={() => toggle(server.id)}
              onSignOut={() => signOut(server.id)}
            />
          ))}
          <li className="row">
            <button
              className={`row-body is-button${open === "+" ? " is-open" : ""}`}
              type="button"
              aria-expanded={open === "+"}
              onClick={() => toggle("+")}
            >
              <span className="badge">
                <IconPlus />
              </span>
              <span className="row-main">
                <span className="row-name">Add a server</span>
              </span>
            </button>
            {open === "+" && (
              <ServerForm
                taken={Object.keys(config)}
                actions={actions}
                onSave={(name, next) => save(null, name, next)}
                onDone={() => setOpen(null)}
              />
            )}
          </li>
        </ul>
        {open === null && actions.errors.mcpServers && <p className="error">{actions.errors.mcpServers}</p>}
      </section>
      <Marketplace
        services={market}
        none={!servers.some((server) => server.source === "catalog")}
        open={open}
        actions={actions}
        onToggle={toggle}
        onClose={() => setOpen(null)}
      />
    </>
  );
}

/**
 * Prime's catalog, the services not added yet, A to Z. Add signs in to one
 * at once; a service connected by a token opens its field first.
 */
function Marketplace({
  services,
  none,
  open,
  actions,
  onToggle,
  onClose,
}: {
  services: McpServer[];
  /** No catalog at all: it could not be read. */
  none: boolean;
  /** The open row, as `market:<id>`. */
  open: string | null;
  actions: Actions;
  onToggle: (key: string) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const words = query.trim().toLowerCase();
  const shown = services.filter((service) => !words || found(service, words));
  return (
    <section className="section">
      <h2 className="section-label">Marketplace</h2>
      {none ? (
        <p className="note section-help">The Marketplace could not be read. It is tried again within the hour.</p>
      ) : (
        <>
          <label className="search">
            <IconSearch />
            <input
              className="input"
              type="search"
              placeholder={`Search ${services.length} services`}
              aria-label="Search the Marketplace"
              spellCheck={false}
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
          </label>
          {shown.length > 0 ? (
            <ul className="list">
              {shown.map((service) => (
                <MarketService
                  key={service.id}
                  service={service}
                  open={open === `market:${service.id}`}
                  actions={actions}
                  onToggle={() => onToggle(`market:${service.id}`)}
                  onClose={onClose}
                />
              ))}
            </ul>
          ) : (
            <p className="note section-help">{words ? "No service matches." : "Every service is added."}</p>
          )}
        </>
      )}
    </section>
  );
}

/** Whether the service answers to the words: its name, other names, category or publisher. */
function found(service: McpServer, words: string): boolean {
  return [service.label, service.id, ...(service.aliases ?? []), service.category, service.publisher].some(
    (text) => text?.toLowerCase().includes(words),
  );
}

function MarketService({
  service,
  open,
  actions,
  onToggle,
  onClose,
}: {
  service: McpServer;
  open: boolean;
  actions: Actions;
  onToggle: () => void;
  onClose: () => void;
}) {
  const key = `mcp:${service.id}`;
  const busy = key in actions.drafts;
  // Nothing to add it by: what it needs instead.
  const blocked = !service.signIn && !service.token;
  const add = () => {
    if (service.token) onToggle();
    // The sign-in screen takes over until it is done.
    else post({ type: "mcpSignIn", id: service.id, label: service.label });
  };
  return (
    <li className="row">
      <div className="row-line">
        <div className={open ? "row-body is-open" : "row-body"}>
          <ServiceAvatar className="badge" service={service.id} name={service.label} />
          <span className="row-main">
            <span className="row-name">{service.label}</span>
            {blocked && <span className="row-meta">{service.hint ?? "Cannot be added"}</span>}
          </span>
          {open && <IconChevron className="row-end" />}
        </div>
        {!open && (
          <button className="button row-add" type="button" disabled={!actions.ready || blocked || busy} onClick={add}>
            Add
          </button>
        )}
      </div>
      {open && service.token && (
        <TokenForm service={service} label={service.token} actions={actions} onDone={onClose} />
      )}
    </li>
  );
}

/** The token a Marketplace service is added by; it is checked with the service first. */
function TokenForm({
  service,
  label,
  actions,
  onDone,
}: {
  service: McpServer;
  label: string;
  actions: Actions;
  onDone: () => void;
}) {
  const [token, setToken] = useState("");
  const key = `mcp:${service.id}`;
  const busy = key in actions.drafts;
  // Once taken, the service is yours: its row leaves the Marketplace.
  const sent = useSettled(key, actions, onDone);
  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    const trimmed = token.trim();
    if (!trimmed || busy) return;
    sent();
    actions.act(key, true, { type: "mcpSetToken", id: service.id, token: trimmed });
  };
  return (
    <form className="row-form" onSubmit={submit} onKeyDown={(event) => event.key === "Escape" && onDone()}>
      <label className="form-field">
        <span className="form-label">{label}</span>
        <input
          className="input is-secret"
          type="password"
          aria-label={label}
          autoComplete="off"
          spellCheck={false}
          autoFocus
          disabled={busy}
          value={token}
          onChange={(event) => setToken(event.target.value)}
        />
      </label>
      {actions.errors[key] && <p className="error">{actions.errors[key]}</p>}
      <div className="actions">
        <button className="button" type="submit" disabled={!token.trim() || busy || !actions.ready}>
          {busy ? "Adding" : "Add"}
        </button>
        <button className="button is-quiet" type="button" onClick={onDone}>
          Cancel
        </button>
      </div>
    </form>
  );
}

/** What the server's state comes to, for its row. */
function status(server: McpServer | undefined): ReactNode {
  if (!server) return "Saving";
  switch (server.status) {
    case "disabled":
      return "Off";
    case "connected":
      // Nothing to sign in to: the agent reaches it when it needs it.
      return server.source === "user" && !server.signedIn ? "Ready" : <span className="row-status">Connected</span>;
    case "pending":
      return <span className="row-status">Signed in</span>;
    case "not_connected":
      return server.signIn ? "Not signed in" : (server.hint ?? "Not connected");
    default:
      return server.hint ?? "Needs setup";
  }
}

function usable(server: McpServer | undefined): boolean {
  return server?.status === "connected" || server?.status === "pending";
}

/** Where a server is: its URL's host, or its command. */
function target(config: ServerConfig | undefined): string | undefined {
  if (!config) return undefined;
  if (config.type === "stdio") return typeof config.command === "string" ? config.command : undefined;
  try {
    return new URL(String(config.url)).host;
  } catch {
    return undefined;
  }
}

/** The sign-in, or the sign-out, a server offers. */
function SignInActions({ server, actions, onSignOut }: { server: McpServer; actions: Actions; onSignOut: () => void }) {
  const key = `mcp:${server.id}`;
  const busy = key in actions.drafts;
  if (!server.signIn && !server.signedIn) return null;
  return (
    <>
      {actions.errors[key] && <p className="error">{actions.errors[key]}</p>}
      <div className="actions">
        {server.signIn && (
          <button
            className="button"
            type="button"
            disabled={!actions.ready}
            // The sign-in screen takes over until it is done.
            onClick={() => post({ type: "mcpSignIn", id: server.id, label: server.label })}
          >
            {server.signedIn ? "Sign in again" : "Sign in"}
          </button>
        )}
        {server.signedIn && (
          <button className="button is-danger" type="button" disabled={!actions.ready || busy} onClick={onSignOut}>
            {busy ? "Signing out" : "Sign out"}
          </button>
        )}
      </div>
    </>
  );
}

function UserServer({
  id,
  server,
  config,
  taken,
  open,
  actions,
  onToggle,
  onSave,
  onEnable,
  onRemove,
  onSignOut,
}: {
  id: string;
  server?: McpServer;
  /** `undefined`: set in the workspace's settings, not here. */
  config?: ServerConfig;
  /** Names already in use. */
  taken: string[];
  open: boolean;
  actions: Actions;
  onToggle: () => void;
  onSave: (id: string, config: ServerConfig) => void;
  onEnable: (on: boolean) => void;
  onRemove: () => void;
  onSignOut: () => void;
}) {
  const where = target(config);
  return (
    <li className="row">
      <div className="row-line">
        <button
          className={`row-body is-button${open ? " is-open" : ""}`}
          type="button"
          aria-expanded={open}
          onClick={onToggle}
        >
          <span className={usable(server) ? "badge is-on" : "badge"}>
            <McpLogo />
          </span>
          <span className="row-main">
            <span className="row-name">{id}</span>
            <span className="row-meta">
              {status(server)}
              {where && ` · ${where}`}
            </span>
          </span>
          <IconChevron className="row-end" />
        </button>
        {config && (
          <Switch
            on={config.enabled !== false}
            label={`Use ${id}`}
            disabled={!actions.ready}
            onChange={onEnable}
          />
        )}
      </div>
      {open && !config && (
        <div className="row-form">
          <p className="note">Set in this workspace's settings, not here.</p>
          {server && <SignInActions server={server} actions={actions} onSignOut={onSignOut} />}
        </div>
      )}
      {open && config && (
        <ServerForm
          id={id}
          config={config}
          server={server}
          taken={taken}
          actions={actions}
          onSave={onSave}
          onRemove={onRemove}
          onSignOut={onSignOut}
          onDone={onToggle}
        />
      )}
    </li>
  );
}

function Service({
  server,
  open,
  actions,
  onToggle,
  onSignOut,
}: {
  server: McpServer;
  open: boolean;
  actions: Actions;
  onToggle: () => void;
  onSignOut: () => void;
}) {
  return (
    <li className="row">
      <button
        className={`row-body is-button${open ? " is-open" : ""}`}
        type="button"
        aria-expanded={open}
        onClick={onToggle}
      >
        <ServiceAvatar className={usable(server) ? "badge is-on" : "badge"} service={server.id} name={server.label} />
        <span className="row-main">
          <span className="row-name">{server.label}</span>
          <span className="row-meta">{status(server)}</span>
        </span>
        <IconChevron className="row-end" />
      </button>
      {open && (
        <div className="row-form">
          {server.description && <p className="note">{server.description}</p>}
          {server.hint && server.status !== "not_connected" && <p className="note">{server.hint}</p>}
          <SignInActions server={server} actions={actions} onSignOut={onSignOut} />
        </div>
      )}
    </li>
  );
}

type Kind = "http" | "stdio";
type Auth = "none" | "oauth" | "variable";

/**
 * A server's details: by URL (with its sign-in) or by command (with the
 * variables it gets). Fields the form does not show (headers, tool filters,
 * timeouts) are kept as the file has them.
 */
function ServerForm({
  id,
  config,
  server,
  taken,
  actions,
  onSave,
  onRemove,
  onSignOut,
  onDone,
}: {
  /** `undefined`: a new server. */
  id?: string;
  config?: ServerConfig;
  server?: McpServer;
  /** Names already in use. */
  taken: string[];
  actions: Actions;
  onSave: (id: string, config: ServerConfig) => void;
  onRemove?: () => void;
  onSignOut?: () => void;
  onDone: () => void;
}) {
  const [name, setName] = useState(id ?? "");
  const [kind, setKind] = useState<Kind>(config?.type === "stdio" ? "stdio" : "http");
  const [url, setUrl] = useState(typeof config?.url === "string" ? config.url : "");
  const [auth, setAuth] = useState<Auth>(
    typeof config?.bearerTokenEnvVar === "string" ? "variable" : config?.oauth === true ? "oauth" : "none",
  );
  const [variable, setVariable] = useState(
    typeof config?.bearerTokenEnvVar === "string" ? config.bearerTokenEnvVar : "",
  );
  const [command, setCommand] = useState(commandLine(config));
  const [passed, setPassed] = useState(envLine(config));
  const [removing, setRemoving] = useState(false);
  const sent = useSettled("mcpServers", actions, onDone);
  const busy = "mcpServers" in actions.drafts;

  const trimmed = name.trim();
  let problem: string | undefined;
  if (trimmed && !NAME.test(trimmed)) problem = "A name is letters, digits, - and _, starting with a letter or digit.";
  else if (trimmed !== id && taken.includes(trimmed)) problem = `There is a server named ${trimmed} already.`;
  const words = splitCommand(command);
  const complete =
    trimmed !== "" &&
    (kind === "http" ? url.trim() !== "" && (auth !== "variable" || variable.trim() !== "") : words.length > 0);

  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    if (!complete || problem || busy) return;
    // Start from the file's entry: what the form does not show stays.
    const next: ServerConfig = { ...config };
    for (const field of ["url", "oauth", "bearerTokenEnvVar", "headers", "command", "args", "cwd", "env"]) {
      delete next[field];
    }
    next.type = kind;
    if (kind === "http") {
      next.url = url.trim();
      if (config?.type !== "stdio" && config?.headers) next.headers = config.headers;
      if (auth === "oauth") next.oauth = true;
      if (auth === "variable") next.bearerTokenEnvVar = variable.trim();
    } else {
      next.command = words[0];
      if (words.length > 1) next.args = words.slice(1);
      if (config?.type === "stdio" && config.cwd) next.cwd = config.cwd;
      const env = parseEnv(passed);
      if (Object.keys(env).length > 0) next.env = env;
    }
    sent();
    onSave(trimmed, next);
  };

  if (removing) {
    return (
      <div className="row-form">
        <p className="note">
          Remove {id}?{server?.signedIn ? " Its sign-in goes too." : ""}
        </p>
        <div className="actions">
          <button className="button is-danger" type="button" disabled={!actions.ready} onClick={onRemove}>
            Remove
          </button>
          <button className="button is-quiet" type="button" onClick={() => setRemoving(false)}>
            Cancel
          </button>
        </div>
      </div>
    );
  }

  return (
    <form className="row-form" onSubmit={submit} onKeyDown={(event) => event.key === "Escape" && onDone()}>
      {server?.hint && server.status !== "not_connected" && <p className="note">{server.hint}</p>}
      {server && onSignOut && <SignInActions server={server} actions={actions} onSignOut={onSignOut} />}
      <label className="form-field">
        <span className="form-label">Name</span>
        <input
          className="input"
          placeholder="e.g. docs"
          spellCheck={false}
          autoFocus={id === undefined}
          value={name}
          onChange={(event) => setName(event.target.value)}
        />
      </label>
      <Choice
        label="Reached by"
        value={kind}
        options={[
          ["http", "URL"],
          ["stdio", "Command"],
        ]}
        onChange={setKind}
      />
      {kind === "http" ? (
        <>
          <label className="form-field">
            <span className="form-label">URL</span>
            <input
              className="input is-mono"
              placeholder="e.g. https://mcp.example.com/mcp"
              spellCheck={false}
              value={url}
              onChange={(event) => setUrl(event.target.value)}
            />
          </label>
          <Choice
            label="Sign-in"
            value={auth}
            options={[
              ["none", "None"],
              ["oauth", "OAuth"],
              ["variable", "Token variable"],
            ]}
            onChange={setAuth}
          />
          {auth === "oauth" && <p className="note">Once saved, sign in from this row.</p>}
          {auth === "variable" && (
            <label className="form-field">
              <span className="form-label">Variable holding the token</span>
              <input
                className="input is-mono"
                placeholder="e.g. DOCS_TOKEN"
                spellCheck={false}
                value={variable}
                onChange={(event) => setVariable(event.target.value)}
              />
            </label>
          )}
        </>
      ) : (
        <>
          <label className="form-field">
            <span className="form-label">Command</span>
            <input
              className="input is-mono"
              placeholder="e.g. npx -y @modelcontextprotocol/server-github"
              spellCheck={false}
              value={command}
              onChange={(event) => setCommand(event.target.value)}
            />
          </label>
          <label className="form-field">
            <span className="form-label">Variables it gets</span>
            <input
              className="input is-mono"
              placeholder="e.g. GITHUB_TOKEN"
              spellCheck={false}
              value={passed}
              onChange={(event) => setPassed(event.target.value)}
            />
            <span className="form-help">
              From your environment, separated by spaces; it gets PATH and HOME anyway. NAME=OTHER passes OTHER as
              NAME.
            </span>
          </label>
        </>
      )}
      {problem && <p className="error">{problem}</p>}
      {actions.errors.mcpServers && <p className="error">{actions.errors.mcpServers}</p>}
      <div className="actions">
        <button className="button" type="submit" disabled={!complete || !!problem || busy || !actions.ready}>
          {busy ? "Saving" : id === undefined ? "Add" : "Save"}
        </button>
        <button className="button is-quiet" type="button" onClick={onDone}>
          Cancel
        </button>
        {onRemove && (
          <button className="button is-quiet is-end" type="button" onClick={() => setRemoving(true)}>
            Remove
          </button>
        )}
      </div>
    </form>
  );
}

/** A choice of a few, as pills in a soft track. */
function Choice<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: [T, string][];
  onChange: (value: T) => void;
}) {
  return (
    <div className="form-field">
      <span className="form-label">{label}</span>
      <div className="choice" role="radiogroup" aria-label={label}>
        {options.map(([option, text]) => (
          <button
            key={option}
            className={option === value ? "choice-option is-on" : "choice-option"}
            type="button"
            role="radio"
            aria-checked={option === value}
            onClick={() => onChange(option)}
          >
            {text}
          </button>
        ))}
      </div>
    </div>
  );
}

/** The command and its arguments as one line, quoted where needed. */
function commandLine(config: ServerConfig | undefined): string {
  if (config?.type !== "stdio" || typeof config.command !== "string") return "";
  const args = Array.isArray(config.args) ? config.args.map(String) : [];
  return [config.command, ...args].map((word) => (/^[^\s"'\\]+$/.test(word) ? word : JSON.stringify(word))).join(" ");
}

/** Words split on spaces; quotes keep spaces in, a backslash keeps the next character. */
function splitCommand(line: string): string[] {
  const words: string[] = [];
  let word = "";
  let quote: string | null = null;
  let started = false;
  for (let at = 0; at < line.length; at++) {
    const char = line[at];
    if (char === "\\" && at + 1 < line.length && quote !== "'") {
      word += line[++at];
      started = true;
    } else if (quote) {
      if (char === quote) quote = null;
      else word += char;
    } else if (char === '"' || char === "'") {
      quote = char;
      started = true;
    } else if (/\s/.test(char)) {
      if (started) words.push(word);
      word = "";
      started = false;
    } else {
      word += char;
      started = true;
    }
  }
  if (started) words.push(word);
  return words;
}

/** The `env` entries as `NAME`, or `NAME=OTHER` when another variable feeds it. */
function envLine(config: ServerConfig | undefined): string {
  if (config?.type !== "stdio" || !config.env || typeof config.env !== "object") return "";
  return Object.entries(config.env as Record<string, { env?: string }>)
    .map(([name, from]) => (from?.env && from.env !== name ? `${name}=${from.env}` : name))
    .join(" ");
}

function parseEnv(line: string): Record<string, { env: string }> {
  const env: Record<string, { env: string }> = {};
  for (const word of line.split(/[\s,]+/).filter(Boolean)) {
    const [name, from] = word.split("=", 2);
    if (name) env[name] = { env: from || name };
  }
  return env;
}

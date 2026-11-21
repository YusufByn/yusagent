import { useEffect, useRef, useState, type SubmitEvent } from "react";
import type { HostMessage, LocalModel, LocalProbe, LocalSave, LocalServer, Provider } from "../src/protocol";
import { IconImage } from "./icons";
import { tokens } from "./models";
import { post } from "./vscode";

type ScanReply = Extract<HostMessage, { type: "localScan" }>;
type ProbeReply = Extract<HostMessage, { type: "localProbe" }>;

/** The host's last answers about local servers. */
export type LocalReplies = { scan: ScanReply | null; probe: ProbeReply | null };

/** The row of the server to add. */
export const NEW_LOCAL = "local:new";

/** How long the address rests before the server is asked. */
const PROBE_AFTER_MS = 450;

const keyOf = (url: string, apiKey: string) => `${url.trim()}\n${apiKey.trim()}`;

/**
 * A server on this machine and the models picked from it. A new one: the
 * usual addresses are looked at first and the first server found is filled
 * in, else its address is typed; all its models come switched on. A server
 * added: its models are listed again, the ones added switched on.
 */
export function LocalForm({
  provider,
  replies,
  busy,
  error,
  onSave,
  onCancel,
}: {
  /** The server added, when it is one. */
  provider?: Provider;
  replies: LocalReplies;
  busy: boolean;
  error?: string;
  onSave: (server: LocalSave) => void;
  onCancel: () => void;
}) {
  const added = provider?.local;
  const [url, setUrl] = useState(added?.baseUrl ?? "");
  const [apiKey, setApiKey] = useState("");
  const [probes, setProbes] = useState<ReadonlyMap<string, LocalProbe>>(() => new Map());
  /** The servers at the usual addresses; `null` while they are looked at. */
  const [found, setFound] = useState<LocalServer[] | null>(null);
  const [picked, setPicked] = useState<ReadonlySet<string>>(() => new Set(added?.models));
  /** Picks the user made; until then a new server's models are all on. */
  const touched = useRef(added !== undefined);

  useEffect(() => {
    if (!added) post({ type: "scanLocal" });
  }, [added]);

  useReply(replies.scan, ({ servers }) => {
    const list = servers ?? [];
    setFound(list);
    setProbes((probes) => {
      const next = new Map(probes);
      for (const server of list) next.set(keyOf(server.baseUrl, ""), { server });
      return next;
    });
    if (list[0]) setUrl((url) => url || list[0].baseUrl);
  });
  useReply(replies.probe, ({ baseUrl, apiKey, probe }) => {
    setProbes((probes) => new Map(probes).set(keyOf(baseUrl, apiKey), probe));
  });

  const probe = probes.get(keyOf(url, apiKey));
  const server = probe && "server" in probe ? probe.server : undefined;
  const failure = probe && "error" in probe ? probe.error : undefined;
  // While the scan runs, the address is the scan's to fill.
  const scanning = !added && found === null && !url.trim();

  useEffect(() => {
    if (!url.trim() || probe) return;
    const timer = setTimeout(() => post({ type: "probeLocal", baseUrl: url.trim(), apiKey: apiKey.trim() }), PROBE_AFTER_MS);
    return () => clearTimeout(timer);
  }, [url, apiKey, probe]);

  useEffect(() => {
    if (server && !touched.current) setPicked(new Set(server.models.map((model) => model.id)));
  }, [server]);

  const choose = (next: string) => {
    if (!added) touched.current = false;
    setUrl(next);
  };
  const again = () => {
    setProbes((probes) => {
      const next = new Map(probes);
      next.delete(keyOf(url, apiKey));
      return next;
    });
  };
  const toggle = (id: string) => {
    touched.current = true;
    setPicked((picked) => {
      const next = new Set(picked);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  };

  // The models added that the server does not list (yet) stay, to be switched off.
  const listed = server?.models ?? [];
  const missing = (added?.models ?? []).filter((id) => !listed.some((model) => model.id === id));
  const models: (LocalModel & { missing?: boolean })[] = [
    ...listed,
    ...missing.map((id) => ({ id, image: false, reasoning: false, missing: true })),
  ];
  const kept = added !== undefined && url.trim() === added.baseUrl;
  const ready = picked.size > 0 && (server !== undefined || kept);

  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    if (!ready || busy) return;
    const chosen = models.filter((model) => picked.has(model.id));
    onSave({
      providerId: provider?.id,
      id: server?.id ?? provider?.id ?? "local",
      name: server?.name ?? provider?.name ?? "",
      baseUrl: server?.baseUrl ?? added?.baseUrl ?? url.trim(),
      ...(apiKey.trim() ? { apiKey: apiKey.trim() } : {}),
      // A model added before keeps its definition: its id is enough.
      models: chosen.map(({ missing, ...model }) => (missing ? { id: model.id } : model)),
    });
  };

  let status: string | null = null;
  if (scanning) status = "Looking for a server on this machine…";
  else if (!url.trim() && !added && found?.length === 0)
    status = "No server at the usual addresses. Start Ollama, LM Studio, llama.cpp or vLLM, or enter the address of yours.";
  else if (url.trim() && !probe) status = `Looking at ${url.trim()}…`;
  else if (server && server.models.length === 0) status = `${server.name} serves no model yet.`;

  let label = "Add";
  if (busy) label = "Saving";
  else if (added) label = "Save";
  else if (picked.size > 1) label = `Add ${picked.size} models`;

  return (
    <form className="row-form" onSubmit={submit} onKeyDown={(event) => event.key === "Escape" && onCancel()}>
      {!added && found && found.length > 0 && (
        <div className="choice" role="radiogroup" aria-label="Servers on this machine">
          {found.map((option) => (
            <button
              key={option.baseUrl}
              className={url.trim() === option.baseUrl ? "choice-option is-on" : "choice-option"}
              type="button"
              role="radio"
              aria-checked={url.trim() === option.baseUrl}
              disabled={busy}
              onClick={() => choose(option.baseUrl)}
            >
              {option.name}
            </button>
          ))}
        </div>
      )}
      <input
        className="input is-mono"
        type="text"
        placeholder="http://localhost:11434/v1"
        aria-label="Server address"
        autoComplete="off"
        spellCheck={false}
        disabled={busy}
        value={url}
        onChange={(event) => choose(event.target.value)}
      />
      {(failure?.kind === "needsKey" || apiKey !== "") && (
        <input
          className="input is-secret"
          type="password"
          placeholder="API key"
          aria-label="API key"
          autoComplete="off"
          spellCheck={false}
          autoFocus
          disabled={busy}
          value={apiKey}
          onChange={(event) => setApiKey(event.target.value)}
        />
      )}
      {status && <p className="note">{status}</p>}
      {failure && (
        <p className={failure.kind === "needsKey" ? "note" : "error"}>
          {failure.message}
          {failure.kind === "unreachable" && (
            <>
              {" "}
              <button className="link" type="button" onClick={again}>
                Try again
              </button>
            </>
          )}
        </p>
      )}
      {models.length > 0 && (
        <ul className="local-models" aria-label="Models">
          {models.map((model) => (
            <li key={model.id}>
              <button
                className="local-model"
                type="button"
                role="switch"
                aria-checked={picked.has(model.id)}
                disabled={busy}
                onClick={() => toggle(model.id)}
              >
                <span className="local-model-name">{model.id}</span>
                <span className="local-model-specs">
                  {model.missing && server ? "not listed" : model.contextWindow ? tokens(model.contextWindow) : null}
                  {model.image && (
                    <span className="local-model-image" data-tip="Reads images">
                      <IconImage />
                    </span>
                  )}
                </span>
                <span className={picked.has(model.id) ? "switch is-on" : "switch"} aria-hidden="true">
                  <span className="switch-knob" />
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
      {error && <p className="error">{error}</p>}
      <div className="actions">
        <button className="button" type="submit" disabled={!ready || busy}>
          {label}
        </button>
        <button className="button is-quiet" type="button" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </form>
  );
}

/** Each answer that comes after the form opened, once. */
function useReply<T>(reply: T | null, take: (reply: T) => void) {
  const before = useRef(reply);
  const latest = useRef(take);
  latest.current = take;
  useEffect(() => {
    if (reply && reply !== before.current) latest.current(reply);
  }, [reply]);
}

import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { savedGroups } from "../src/instructions";
import type { ModelChoice, Provider, SessionState, SettingsState, WebviewMessage } from "../src/protocol";
import type { SettingResult } from "./App";
import { IconBack, IconChevron } from "./icons";
import { ProviderLogo } from "./logos";
import { isLocal, ModelList, providerName } from "./models";
import { InstructionsTab } from "./SettingsInstructions";
import { McpTab } from "./SettingsMcp";
import { SkillsTab } from "./SettingsSkills";
import { post } from "./vscode";

export type SettingsTab = "general" | "mcp" | "skills" | "instructions";

// yusAgent (ticket 31, C9) : l'onglet Instructions (`modelInstructions`, ajouté au prompt système
// chez Unharnessed) est masqué ; son code reste jusqu'au ticket 32 ou au visuel.
const TABS: [SettingsTab, string][] = [
  ["general", "General"],
  ["mcp", "MCP"],
  ["skills", "Skills"],
];

/** How a number is shown and typed; the file holds tokens, counts or ms. */
type Unit = "tokens" | "count" | "seconds" | "hours";

type Field =
  | { kind: "switch"; key: string; label: string; help?: string }
  | { kind: "number"; key: string; label: string; help?: string; unit: Unit; max: number }
  | { kind: "model"; key: string; label: string; help?: string; none: string; images?: boolean };

type Group = {
  label: string;
  help?: string;
  fields: Field[];
  /** The switch whose off state makes the other fields moot. */
  gate?: string;
};

// yusAgent (ticket 31, décision 5) : seulement les clés appliquées au message suivant sans miss de
// cache (`retry.*`, `auxiliaryModel`, `compaction.enabled`), celles que sert `_yus/settings/*` ; plus
// les images, masquées (§5.4 du ticket 24).
const GROUPS: Group[] = [
  {
    label: "Compaction",
    help: "Summarizes older messages when the context fills up.",
    gate: "compaction.enabled",
    fields: [
      {
        kind: "switch",
        key: "compaction.enabled",
        label: "Compact automatically",
        help: "Before the context is full.",
      },
      {
        kind: "model",
        key: "auxiliaryModel",
        label: "Summary model",
        help: "Writes the summaries.",
        none: "Same as the chat model",
      },
    ],
  },
  {
    label: "Retries",
    help: "When a request to the provider fails, it is sent again after a wait that doubles each time.",
    gate: "retry.enabled",
    fields: [
      { kind: "switch", key: "retry.enabled", label: "Retry failed requests" },
      { kind: "number", key: "retry.maxRetries", label: "Retries", unit: "count", max: 100 },
      { kind: "number", key: "retry.baseDelayMs", label: "First wait", unit: "seconds", max: 3_600_000 },
      {
        kind: "number",
        key: "retry.provider.maxRetryDelayMs",
        label: "Longest wait",
        help: "When the provider asks to wait longer, the request fails.",
        unit: "seconds",
        max: 86_400_000,
      },
    ],
  },
  {
    label: "Switching providers",
    help: "When another connected provider serves the same model, a request that keeps failing moves to it. These retries then replace the ones above.",
    gate: "retry.failover.enabled",
    fields: [
      { kind: "switch", key: "retry.failover.enabled", label: "Switch providers" },
      { kind: "number", key: "retry.failover.maxRetries", label: "Retries per provider", unit: "count", max: 100 },
      { kind: "number", key: "retry.failover.baseDelayMs", label: "First wait", unit: "seconds", max: 3_600_000 },
      { kind: "number", key: "retry.failover.maxDelayMs", label: "Longest wait", unit: "seconds", max: 3_600_000 },
    ],
  },
  {
    label: "Usage limits",
    help: "When a plan's usage limit is reached.",
    gate: "retry.provider.waitForUsage.pauseUntilReset",
    fields: [
      {
        kind: "switch",
        key: "retry.provider.waitForUsage.pauseUntilReset",
        label: "Wait for the reset",
        help: "Pause, then resume on its own once the limit resets.",
      },
      {
        kind: "number",
        key: "retry.provider.waitForUsage.maxPauseMs",
        label: "Longest pause",
        help: "Up to 168 hours, a week.",
        unit: "hours",
        max: 604_800_000,
      },
      {
        kind: "number",
        key: "retry.provider.waitForUsage.maxParks",
        label: "Pauses per limit",
        unit: "count",
        max: 100,
      },
    ],
  },
];

const UNITS: Record<Unit, { label: string; scale: number }> = {
  tokens: { label: "tokens", scale: 1 },
  count: { label: "", scale: 1 },
  seconds: { label: "s", scale: 1000 },
  hours: { label: "h", scale: 3_600_000 },
};

export type Write = (key: string, value: unknown) => void;

/**
 * Send an action under `key`: `draft` shows at once (`drafts[key]`), until
 * the agent's answer replaces it or a refusal lands in `errors[key]`.
 */
export type Act = (key: string, draft: unknown, message: WebviewMessage) => void;

/** What the tabs act through. */
export type Actions = {
  drafts: Record<string, unknown>;
  errors: Record<string, string>;
  act: Act;
  /** The agent runs: changes can be sent. */
  ready: boolean;
};

/**
 * Calls `done` when the action sent under `key` after `start()` settles
 * without an error: a form closes once the agent took it.
 */
export function useSettled(key: string, { drafts, errors }: Actions, done: () => void): () => void {
  const sent = useRef(false);
  const busy = key in drafts;
  useEffect(() => {
    if (!sent.current || busy) return;
    sent.current = false;
    if (!(key in errors)) done();
  }, [busy]);
  return () => {
    sent.current = true;
  };
}

/**
 * The agent's global settings (`~/.yusagent/settings.json`), its MCP
 * servers and its skills, a tab each. A change is written at once and
 * reaches the agent with the next message (yusAgent, ticket 31).
 */
export function Settings({
  settings,
  ready,
  session,
  providers,
  result,
  tab,
  onTab,
  onBack,
}: {
  settings: SettingsState;
  /** The agent runs: settings can be written. */
  ready: boolean;
  session: SessionState | null;
  providers: Provider[];
  result: SettingResult | null;
  /** Held by the app: a sign-in screen in between keeps it. */
  tab: SettingsTab;
  onTab: (tab: SettingsTab) => void;
  onBack: () => void;
}) {
  // Shown before the agent answers; its answer, or a refusal, replaces it.
  const [drafts, setDrafts] = useState<Record<string, unknown>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});

  useEffect(() => {
    if (!result) return;
    setDrafts(({ [result.key]: _, ...rest }) => rest);
    setErrors(({ [result.key]: _, ...rest }) => (result.error ? { ...rest, [result.key]: result.error } : rest));
  }, [result]);

  const act: Act = (key, draft, message) => {
    setDrafts((current) => ({ ...current, [key]: draft }));
    setErrors(({ [key]: _, ...rest }) => rest);
    post(message);
  };
  const actions: Actions = { drafts, errors, act, ready };
  const view = settings.view;
  const value = (key: string): unknown => {
    if (key in drafts) return drafts[key];
    return view?.values[key] ?? view?.defaults[key] ?? null;
  };
  // A value equal to the default is removed from the file: the default applies.
  const write: Write = (key, next) => {
    const stored = next !== null && JSON.stringify(next) === JSON.stringify(view?.defaults[key]) ? null : next;
    act(key, next ?? view?.defaults[key] ?? null, { type: "setSetting", key, value: stored });
  };
  const choices = session?.kind === "open" ? (session.config.model?.choices ?? null) : null;
  // The light under the current tab: tabs are as wide as their label.
  const track = useRef<HTMLDivElement>(null);
  const [light, setLight] = useState<{ left: number; width: number } | null>(null);
  useLayoutEffect(() => {
    const tabs = track.current;
    if (!tabs) return;
    const place = () => {
      const on = tabs.querySelector<HTMLElement>(".tab.is-on");
      if (on) setLight({ left: on.offsetLeft, width: on.offsetWidth });
    };
    place();
    // A label's width moves with the font and the side bar's.
    const observer = new ResizeObserver(place);
    for (const button of tabs.querySelectorAll(".tab")) observer.observe(button);
    return () => observer.disconnect();
  }, [tab]);

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
          <span>Settings</span>
        </div>
      </header>
      <div className="tabs" role="tablist" aria-label="Settings" ref={track}>
        {light && <span className="tab-light" style={{ width: light.width, translate: `${light.left}px 0` }} />}
        {TABS.map(([id, label]) => (
          <button
            key={id}
            className={id === tab ? "tab is-on" : "tab"}
            type="button"
            role="tab"
            aria-selected={id === tab}
            onClick={() => onTab(id)}
          >
            {label}
          </button>
        ))}
      </div>
      <main className="content">
        {tab === "general" && (
          <General
            settings={settings}
            ready={ready}
            session={session}
            providers={providers}
            errors={errors}
            value={value}
            write={write}
          />
        )}
        {tab === "mcp" && (
          <McpTab
            servers={settings.mcp}
            config={(value("mcpServers") ?? {}) as Record<string, Record<string, unknown>>}
            saved={(view?.values.mcpServers ?? {}) as Record<string, Record<string, unknown>>}
            actions={actions}
            write={write}
          />
        )}
        {tab === "skills" && <SkillsTab skills={settings.skills} actions={actions} />}
        {tab === "instructions" && (
          <InstructionsTab
            groups={view ? savedGroups(view) : null}
            choices={choices}
            providers={providers}
            actions={actions}
          />
        )}
      </main>
    </div>
  );
}

/** Compaction, retries, switching providers, usage limits. */
function General({
  settings,
  ready,
  session,
  providers,
  errors,
  value,
  write,
}: {
  settings: SettingsState;
  ready: boolean;
  session: SessionState | null;
  providers: Provider[];
  errors: Record<string, string>;
  value: (key: string) => unknown;
  write: Write;
}) {
  const [open, setOpen] = useState<string | null>(null);
  const view = settings.view;
  const choices = session?.kind === "open" ? (session.config.model?.choices ?? null) : null;
  return (
    <>
      {!view && <p className="note">Reading the settings…</p>}
      {view &&
        GROUPS.map((group) => {
          const gated = group.gate !== undefined && value(group.gate) === false;
          return (
            <section className="section" key={group.label}>
              <h2 className="section-label">{group.label}</h2>
              {group.help && <p className="note section-help">{group.help}</p>}
              <ul className="list">
                {group.fields.map((field) => (
                  <FieldRow
                    key={field.key}
                    field={field}
                    value={value(field.key)}
                    fallback={view.defaults[field.key]}
                    // The gate itself stays live; the rest dims while it is off.
                    dim={gated && field.key !== group.gate}
                    disabled={!ready}
                    error={errors[field.key]}
                    choices={choices}
                    providers={providers}
                    open={open === field.key}
                    onOpen={(on) => setOpen(on ? field.key : null)}
                    write={write}
                  />
                ))}
              </ul>
            </section>
          );
        })}
    </>
  );
}

function FieldRow({
  field,
  value,
  fallback,
  dim,
  disabled,
  error,
  choices,
  providers,
  open,
  onOpen,
  write,
}: {
  field: Field;
  value: unknown;
  fallback: unknown;
  dim: boolean;
  disabled: boolean;
  error?: string;
  choices: ModelChoice[] | null;
  providers: Provider[];
  open: boolean;
  onOpen: (open: boolean) => void;
  write: Write;
}) {
  const label = (
    <span className="row-main">
      <span className="row-name">
        {field.label}
      </span>
      {field.help && <span className="setting-help">{field.help}</span>}
    </span>
  );
  let control: ReactNode;
  switch (field.kind) {
    case "switch":
      control = <Switch on={value === true} label={field.label} disabled={disabled} onChange={(on) => write(field.key, on)} />;
      break;
    case "number":
      control = (
        <NumberField
          field={field}
          value={typeof value === "number" ? value : null}
          fallback={typeof fallback === "number" ? fallback : null}
          disabled={disabled}
          write={write}
        />
      );
      break;
    case "model":
      return (
        <ModelRow
          field={field}
          value={typeof value === "string" ? value : null}
          label={label}
          dim={dim}
          disabled={disabled}
          error={error}
          choices={choices}
          providers={providers}
          open={open}
          onOpen={onOpen}
          write={write}
        />
      );
  }
  return (
    <li className={dim ? "setting is-dim" : "setting"}>
      <div className="setting-line">
        {label}
        {control}
      </div>
      {error && <p className="error setting-error">{error}</p>}
    </li>
  );
}

/** A pill with a knob: cyan and to the right when on. */
export function Switch({
  on,
  label,
  disabled,
  onChange,
}: {
  on: boolean;
  label: string;
  disabled: boolean;
  onChange: (on: boolean) => void;
}) {
  return (
    <button
      className={on ? "switch is-on" : "switch"}
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!on)}
    >
      <span className="switch-knob" />
    </button>
  );
}

/**
 * A number in its unit, written on Enter or when the field is left; empty
 * puts the default back, which shows as the placeholder.
 */
function NumberField({
  field,
  value,
  fallback,
  disabled,
  write,
}: {
  field: Extract<Field, { kind: "number" }>;
  value: number | null;
  fallback: number | null;
  disabled: boolean;
  write: Write;
}) {
  const unit = UNITS[field.unit];
  const shown = (stored: number | null) => (stored === null ? "" : String(round(stored / unit.scale)));
  // Only a value set in the file fills the field; the default is the placeholder.
  const set = value !== null && value !== fallback ? value : null;
  const [text, setText] = useState(shown(set));
  const [invalid, setInvalid] = useState(false);

  useEffect(() => {
    setText(shown(set));
    setInvalid(false);
  }, [set]);

  const commit = () => {
    const typed = text.trim();
    if (typed === "") {
      setInvalid(false);
      if (set !== null) write(field.key, null);
      return;
    }
    const number = Number(typed);
    const stored = Math.round(number * unit.scale);
    const whole = field.unit === "count" || field.unit === "tokens" ? Number.isInteger(number) : true;
    if (!Number.isFinite(number) || number < 0 || !whole || stored > field.max) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    if (stored === fallback) {
      // The default: the placeholder says it.
      setText("");
      if (set !== null) write(field.key, null);
    } else if (stored !== set) {
      write(field.key, stored);
    }
  };

  return (
    <label className={invalid ? "number-field is-invalid" : "number-field"}>
      <input
        className="input"
        type="text"
        inputMode="decimal"
        aria-label={field.label}
        placeholder={shown(fallback)}
        spellCheck={false}
        disabled={disabled}
        value={text}
        onChange={(event) => setText(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === "Enter") event.currentTarget.blur();
          if (event.key === "Escape") {
            setText(shown(set));
            setInvalid(false);
          }
        }}
      />
      {unit.label && <span className="number-unit">{unit.label}</span>}
    </label>
  );
}

/** A model setting: the model, or the `none` label; it opens a model list in the row. */
function ModelRow({
  field,
  value,
  label,
  dim,
  disabled,
  error,
  choices,
  providers,
  open,
  onOpen,
  write,
}: {
  field: Extract<Field, { kind: "model" }>;
  value: string | null;
  label: ReactNode;
  dim: boolean;
  disabled: boolean;
  error?: string;
  choices: ModelChoice[] | null;
  providers: Provider[];
  open: boolean;
  onOpen: (open: boolean) => void;
  write: Write;
}) {
  const keyOf = (choice: ModelChoice) => `${choice.provider}/${choice.id}`;
  const offered = (choices ?? []).filter((choice) => !field.images || choice.input.includes("image"));
  const current = (choices ?? []).find((choice) => keyOf(choice) === value);
  const unusable = !disabled && choices === null;
  return (
    <li className={dim ? "setting is-dim" : "setting"}>
      <button
        className={`row-body is-button setting-line${open ? " is-open" : ""}`}
        type="button"
        aria-expanded={open}
        disabled={disabled || unusable}
        onClick={() => onOpen(!open)}
      >
        {label}
        <span className="setting-value">
          {current && (
            <ProviderLogo
              provider={current.provider}
              name={providerName(providers, current.provider)}
              local={isLocal(providers, current.provider)}
            />
          )}
          <span>{current?.name ?? value ?? field.none}</span>
          <IconChevron className="row-end" />
        </span>
      </button>
      {open && (
        <div className="setting-models">
          <ModelList
            choices={offered}
            selected={value}
            keyOf={keyOf}
            providers={providers}
            none={field.none}
            onPick={(choice) => {
              write(field.key, choice ? keyOf(choice) : null);
              onOpen(false);
            }}
          />
        </div>
      )}
      {unusable && <p className="note setting-error">The models show once the session is open.</p>}
      {error && <p className="error setting-error">{error}</p>}
    </li>
  );
}

/** Up to three decimals, no trailing zeros. */
function round(value: number): number {
  return Number(value.toFixed(3));
}

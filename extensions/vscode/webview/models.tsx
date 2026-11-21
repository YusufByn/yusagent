import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import type { ModelChoice, Provider } from "../src/protocol";
import { IconCheck, IconImage, IconSearch, IconText } from "./icons";
import { ProviderAvatar } from "./logos";

/**
 * The models with a search, each with its provider's mark and name; the
 * selected model's provider first, then by provider. `keyOf` names a model
 * the way the caller stores it. With `none`, a first row picks no model.
 * With `checked`, several are checked instead: a pick checks or unchecks.
 */
export function ModelList({
  choices,
  selected,
  checked,
  keyOf,
  providers,
  none,
  autoFocus = true,
  onPick,
}: {
  choices: ModelChoice[];
  selected: string | null;
  checked?: ReadonlySet<string>;
  keyOf: (choice: ModelChoice) => string;
  providers: Provider[];
  /** The label of the row that picks no model. */
  none?: string;
  autoFocus?: boolean;
  /** `null`: the `none` row. */
  onPick: (choice: ModelChoice | null) => void;
}) {
  const [query, setQuery] = useState("");
  const current = choices.find((choice) => keyOf(choice) === selected);
  const matched = matching(choices, query, providers, current?.provider);
  // The `none` row heads the list while the search is empty.
  const rows: (Row | null)[] = none !== undefined && query.trim() === "" ? [null, ...matched] : matched;
  const isSelected = (row: Row | null) =>
    checked ? row !== null && checked.has(keyOf(row.choice)) : row ? keyOf(row.choice) === selected : selected === null;
  const [active, setActive] = useState(() => Math.max(0, rows.findIndex(isSelected)));
  const list = useRef<HTMLUListElement>(null);

  useEffect(() => {
    list.current?.querySelector(".menu-row.is-active")?.scrollIntoView({ block: "nearest" });
  }, [active, query]);

  const pick = (row: Row | null | undefined) => {
    if (row !== undefined) onPick(row?.choice ?? null);
  };
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      setActive((index) => (rows.length ? (index + step + rows.length) % rows.length : 0));
    } else if (event.key === "Enter") {
      event.preventDefault();
      pick(rows[active]);
    }
  };

  return (
    <>
      <label className="search menu-search">
        <IconSearch />
        <input
          className="input"
          type="search"
          placeholder="Search models"
          aria-label="Search models"
          autoFocus={autoFocus}
          spellCheck={false}
          value={query}
          onChange={(event) => {
            // A new search starts at its first result.
            setQuery(event.target.value);
            setActive(0);
          }}
          onKeyDown={onKeyDown}
        />
      </label>
      <ul className="menu-list" role="listbox" aria-label="Models" aria-multiselectable={!!checked} ref={list}>
        {rows.map((row, at) => (
          <li key={row ? keyOf(row.choice) : ""} role="option" aria-selected={isSelected(row)}>
            <button
              className={at === active ? "menu-row model-row is-active" : "menu-row model-row"}
              type="button"
              tabIndex={-1}
              onMouseMove={() => setActive(at)}
              onClick={() => pick(row)}
            >
              {row ? (
                <>
                  <ProviderAvatar
                    className="model-badge"
                    provider={row.choice.provider}
                    name={row.provider.name}
                    local={row.provider.local}
                  />
                  <span className="menu-row-main">
                    <span className="model-title">
                      <span className="menu-row-name">{row.choice.name}</span>
                      <span className="model-provider">{row.provider.name}</span>
                    </span>
                    <Specs choice={row.choice} subscription={row.provider.subscription} />
                  </span>
                </>
              ) : (
                <span className="menu-row-main">
                  <span className="menu-row-name">{none}</span>
                </span>
              )}
              {checked ? (
                <span className={isSelected(row) ? "model-tick is-on" : "model-tick"}>
                  <IconCheck />
                </span>
              ) : (
                isSelected(row) && <IconCheck className="menu-check" />
              )}
            </button>
          </li>
        ))}
        {rows.length === 0 && <li className="menu-empty">No model matches “{query.trim()}”.</li>}
      </ul>
    </>
  );
}

/**
 * What the model is, labelled: its context window, what it reads (text,
 * images; dim when not), its price per million tokens (none on a
 * subscription, where the plan pays, nor when unknown).
 */
function Specs({ choice, subscription }: { choice: ModelChoice; subscription: boolean }) {
  const text = choice.input.length === 0 || choice.input.includes("text");
  const image = choice.input.includes("image");
  const cost = choice.cost;
  const priced = !subscription && cost && (cost.input > 0 || cost.output > 0);
  return (
    <span className="specs">
      {choice.contextWindow > 0 && (
        <span className="spec">
          <span className="spec-label">Context</span>
          <span className="spec-value">{tokens(choice.contextWindow)}</span>
        </span>
      )}
      <span className="spec" aria-label={image ? "Input: text and images" : "Input: text only"}>
        <span className="spec-label">Input</span>
        <IconText className={text ? undefined : "is-off"} />
        <IconImage className={image ? undefined : "is-off"} />
      </span>
      {priced && (
        <span className="spec">
          <span className="spec-label">Price</span>
          <span className="spec-value">
            {dollars(cost.input)}/M in · {dollars(cost.output)}/M out
          </span>
        </span>
      )}
    </span>
  );
}

type Row = { choice: ModelChoice; provider: { name: string; subscription: boolean; local: boolean } };

/** The models the search matches, the `first` provider's first, then by provider name. */
function matching(choices: ModelChoice[], query: string, providers: Provider[], first: string | undefined): Row[] {
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const rows: Row[] = [];
  for (const choice of choices) {
    const name = providerName(providers, choice.provider);
    const haystack = `${choice.name} ${choice.id} ${name}`.toLowerCase();
    if (!words.every((word) => haystack.includes(word))) continue;
    const provider = { name, subscription: subscription(providers, choice.provider), local: isLocal(providers, choice.provider) };
    rows.push({ choice, provider });
  }
  // Stable: each provider's models keep the agent's order.
  return rows.sort((a, b) => {
    if (a.choice.provider === b.choice.provider) return 0;
    if (a.choice.provider === first) return -1;
    if (b.choice.provider === first) return 1;
    return a.provider.name.localeCompare(b.provider.name) || a.choice.provider.localeCompare(b.choice.provider);
  });
}

/**
 * The connected row of the provider; Anthropic and xAI have a subscription
 * row and an API key row under the same id, the subscription first.
 */
function connectedRow(providers: Provider[], id: string): Provider | undefined {
  return providers.find((p) => p.id === id && p.connected) ?? providers.find((p) => p.id === id);
}

export function providerName(providers: Provider[], id: string): string {
  return connectedRow(providers, id)?.name ?? id;
}

/** A server on this machine, added from the providers screen or by hand. */
export function isLocal(providers: Provider[], id: string): boolean {
  return providers.some((p) => p.id === id && p.local !== undefined);
}

/** A subscription pays for the models: no price per token to show. */
function subscription(providers: Provider[], id: string): boolean {
  const row = connectedRow(providers, id);
  return row?.connected === true && row.method === "subscription";
}

/** 1000000 → `1M`, 400000 → `400K`, 1048576 → `1M`. */
export function tokens(count: number): string {
  if (count >= 1_000_000) return `${trim(count / 1_000_000, 1)}M`;
  if (count >= 1_000) return `${Math.round(count / 1_000)}K`;
  return String(count);
}

/** 4 → `$4`, 1.25 → `$1.25`, 0.075 → `$0.075`. */
function dollars(amount: number): string {
  return `$${trim(amount, amount < 1 ? 3 : 2)}`;
}

function trim(value: number, digits: number): string {
  return String(Number(value.toFixed(digits)));
}

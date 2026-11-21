import { useState, type SubmitEvent } from "react";
import { connectedModels, modelKey } from "../src/instructions";
import type { ModelChoice, ModelInstructions, Provider } from "../src/protocol";
import { IconChevron, IconChevronDown, IconCross, IconPlus } from "./icons";
import { ProviderLogo } from "./logos";
import { isLocal, ModelList, providerName } from "./models";
import { Popover } from "./Popover";
import { useSettled, type Actions } from "./Settings";

const KEY = "modelInstructions";

/**
 * Groups of instructions: a text the agent's system prompt carries when it
 * runs on one of the group's models. A group shows only the models of
 * connected providers; the others stay in the file and come back with
 * their provider, and so does a group left with none.
 */
export function InstructionsTab({
  groups,
  choices,
  providers,
  actions,
}: {
  /** The `modelInstructions` setting of the global file. */
  groups: ModelInstructions[] | null;
  /** The session's models; `null` until it is open. */
  choices: ModelChoice[] | null;
  providers: Provider[];
  actions: Actions;
}) {
  // The group whose card is open, by its place in the file, or `+` for a new one.
  const [open, setOpen] = useState<number | "+" | null>(null);
  if (!groups) return <p className="note">Reading the settings…</p>;
  if (!choices) return <p className="note">The groups show once the session is open.</p>;

  const connected = connectedModels(choices, providers);
  const shown = groups
    .map((group, index) => ({ index, text: group.text, models: models(group, connected) }))
    .filter((group) => group.models.length > 0);
  const toggle = (id: number | "+") => setOpen((current) => (current === id ? null : id));

  return (
    <>
      <p className="note section-help">
        A group's text joins the agent's instructions whenever it runs on one of the group's models. Changes apply from
        the next conversation.
      </p>
      <section className="section">
        <ul className="list">
          {shown.map((group) => (
            <li className="row" key={group.index}>
              {open === group.index ? (
                // Open, the form shows the models and the text: the head is a title.
                <button
                  className="row-body is-button is-open"
                  type="button"
                  aria-expanded
                  onClick={() => toggle(group.index)}
                >
                  <span className="row-main">
                    <span className="row-name">Group</span>
                  </span>
                  <IconChevron className="row-end" />
                </button>
              ) : (
                <button
                  className="row-body is-button group-body"
                  type="button"
                  aria-expanded={false}
                  onClick={() => toggle(group.index)}
                >
                  <span className="row-main">
                    <ModelChips models={group.models} providers={providers} />
                    <span className="group-text">{group.text}</span>
                  </span>
                  <IconChevron className="row-end" />
                </button>
              )}
              {open === group.index && (
                <GroupForm
                  index={group.index}
                  checked={group.models.map(modelKey)}
                  text={group.text}
                  choices={[...connected.values()]}
                  providers={providers}
                  actions={actions}
                  onDone={() => setOpen(null)}
                />
              )}
            </li>
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
                <span className="row-name">New group</span>
              </span>
            </button>
            {open === "+" && (
              <GroupForm
                index={null}
                checked={[]}
                text=""
                choices={[...connected.values()]}
                providers={providers}
                actions={actions}
                onDone={() => setOpen(null)}
              />
            )}
          </li>
        </ul>
        {open === null && actions.errors[KEY] && <p className="error">{actions.errors[KEY]}</p>}
      </section>
    </>
  );
}

/** The group's models the screen shows, in its order. */
function models(group: ModelInstructions, connected: Map<string, ModelChoice>): ModelChoice[] {
  return group.models.flatMap((key) => connected.get(key) ?? []);
}

/** Models in a line that wraps, a mark and a name each. */
function ModelChips({ models, providers }: { models: ModelChoice[]; providers: Provider[] }) {
  return (
    <span className="group-models">
      {models.map((choice) => (
        <span key={modelKey(choice)} className="group-model">
          <ProviderLogo
            provider={choice.provider}
            name={providerName(providers, choice.provider)}
            local={isLocal(providers, choice.provider)}
          />
          <span className="group-model-name">{choice.name}</span>
        </span>
      ))}
    </span>
  );
}

/**
 * An open card: the models to check, among the connected providers' ones,
 * and the text. The models' field holds the checked ones, each with a
 * cross that unchecks it; a press elsewhere on the field opens their list,
 * which closes on a press outside it or Escape. It saves with at least one model and some
 * text; the group's hidden models stay as they are (the extension keeps
 * them).
 */
function GroupForm({
  index,
  checked: initial,
  text: initialText,
  choices,
  providers,
  actions,
  onDone,
}: {
  /** `null`: a new group. */
  index: number | null;
  checked: string[];
  text: string;
  choices: ModelChoice[];
  providers: Provider[];
  actions: Actions;
  onDone: () => void;
}) {
  const [checked, setChecked] = useState<string[]>(initial);
  const [text, setText] = useState(initialText);
  const [deleting, setDeleting] = useState(false);
  const [picking, setPicking] = useState(false);
  const sent = useSettled(KEY, actions, onDone);
  const busy = KEY in actions.drafts;
  const complete = checked.length > 0 && text.trim() !== "";
  const toggle = (key: string) =>
    setChecked((current) => (current.includes(key) ? current.filter((other) => other !== key) : [...current, key]));
  const send = (group: ModelInstructions | null) => {
    sent();
    actions.act(KEY, true, { type: "setInstructions", index, group });
  };

  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    if (complete && !busy) send({ models: checked, text: text.trim() });
  };

  if (deleting) {
    return (
      <div className="row-form">
        <p className="note">Delete this group?</p>
        {actions.errors[KEY] && <p className="error">{actions.errors[KEY]}</p>}
        <div className="actions">
          <button className="button is-danger" type="button" disabled={!actions.ready || busy} onClick={() => send(null)}>
            {busy ? "Deleting" : "Delete"}
          </button>
          <button className="button is-quiet" type="button" onClick={() => setDeleting(false)}>
            Cancel
          </button>
        </div>
      </div>
    );
  }

  const picked = checked.flatMap((key) => choices.find((choice) => modelKey(choice) === key) ?? []);
  return (
    // Escape closes the models' list first, the card after.
    <form className="row-form" onSubmit={submit} onKeyDown={(event) => event.key === "Escape" && !picking && onDone()}>
      <div className="form-field">
        <span className="form-label">Models</span>
        {/* The chevron is the keyboard's way in; the pointer's is anywhere on the field. */}
        <div
          className={picking ? "input picker-field is-open" : "input picker-field"}
          data-menu-trigger="group-models"
          onClick={() => setPicking((open) => !open)}
        >
          {picked.length > 0 ? (
            <span className="picker-pills">
              {picked.map((choice) => (
                <button
                  key={modelKey(choice)}
                  className="model-pill"
                  type="button"
                  aria-label={`Uncheck ${choice.name}`}
                  onClick={(event) => {
                    event.stopPropagation();
                    toggle(modelKey(choice));
                  }}
                >
                  <ProviderLogo
                    provider={choice.provider}
                    name={providerName(providers, choice.provider)}
                    local={isLocal(providers, choice.provider)}
                  />
                  <span className="group-model-name">{choice.name}</span>
                  <IconCross className="model-pill-cross" />
                </button>
              ))}
            </span>
          ) : (
            <span className="picker-placeholder">Choose models</span>
          )}
          <button
            className="picker-toggle"
            type="button"
            aria-label="Choose models"
            aria-expanded={picking}
            onClick={(event) => {
              event.stopPropagation();
              setPicking((open) => !open);
            }}
          >
            <IconChevronDown />
          </button>
        </div>
        {picking && (
          <Popover className="setting-models group-pick" trigger="group-models" onClose={() => setPicking(false)}>
            <ModelList
              choices={choices}
              selected={null}
              checked={new Set(checked)}
              keyOf={modelKey}
              providers={providers}
              onPick={(choice) => choice && toggle(modelKey(choice))}
            />
          </Popover>
        )}
      </div>
      <label className="form-field">
        <span className="form-label">Instructions</span>
        <textarea
          className="input is-area"
          rows={5}
          placeholder="e.g. Be concise. No summary at the end."
          value={text}
          onChange={(event) => setText(event.target.value)}
        />
      </label>
      {actions.errors[KEY] && <p className="error">{actions.errors[KEY]}</p>}
      <div className="actions">
        <button className="button" type="submit" disabled={!complete || busy || !actions.ready}>
          {busy ? "Saving" : "Save"}
        </button>
        <button className="button is-quiet" type="button" onClick={onDone}>
          Cancel
        </button>
        {index !== null && (
          <button className="button is-quiet is-end" type="button" onClick={() => setDeleting(true)}>
            Delete
          </button>
        )}
      </div>
    </form>
  );
}

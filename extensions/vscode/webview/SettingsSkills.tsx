import { useState, type SubmitEvent } from "react";
import type { Skill, SkillsView } from "../src/protocol";
import { IconCross, IconExternal, IconFolder, IconPlus, IconSearch } from "./icons";
import { Switch, useSettled, type Actions } from "./Settings";
import { post } from "./vscode";

/** From this many skills on, a search field filters them. */
const SEARCH_FROM = 9;

/** A skill's name: lowercase letters, digits and single hyphens. */
const NAME = /^[a-z0-9]+(-[a-z0-9]+)*$/;

const GROUPS: [string, (skill: Skill) => boolean][] = [
  ["Yours", (skill) => skill.source === "user" || skill.source === "folder"],
  ["This workspace", (skill) => skill.source === "project"],
  ["Built in", (skill) => skill.source === "builtin"],
];

/**
 * The skills the agent finds, on or off; a new one; the folders it reads
 * them from. It loads them when it starts.
 */
export function SkillsTab({ skills, actions }: { skills: SkillsView | null; actions: Actions }) {
  const [creating, setCreating] = useState(false);
  const [query, setQuery] = useState("");
  if (!skills) return <p className="note">Reading the skills…</p>;
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const shown = skills.skills.filter((skill) => {
    const haystack = `${skill.name} ${skill.description}`.toLowerCase();
    return words.every((word) => haystack.includes(word));
  });
  return (
    <>
      <p className="note section-help">
        Instructions the agent follows when a task calls for them: it sees each skill's name and description, and
        reads the rest when needed. Changes apply when it restarts.
      </p>
      {creating ? (
        <ul className="list">
          <li className="row">
            <NewSkill actions={actions} onDone={() => setCreating(false)} />
          </li>
        </ul>
      ) : (
        <button className="button" type="button" disabled={!actions.ready} onClick={() => setCreating(true)}>
          <IconPlus />
          New skill
        </button>
      )}
      {skills.skills.length >= SEARCH_FROM && (
        <label className="search">
          <IconSearch />
          <input
            className="input"
            type="search"
            placeholder="Search skills"
            aria-label="Search skills"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
      )}
      {GROUPS.map(([label, belongs]) => {
        const rows = shown.filter(belongs);
        return (
          rows.length > 0 && (
            <section className="section" key={label}>
              <h2 className="section-label">{label}</h2>
              <ul className="list">
                {rows.map((skill) => (
                  <SkillRow key={skill.path} skill={skill} actions={actions} />
                ))}
              </ul>
            </section>
          )
        );
      })}
      {shown.length === 0 && <p className="note">No skill matches “{query.trim()}”.</p>}
      {skills.problems.length > 0 && (
        <section className="section">
          <h2 className="section-label">Not loaded</h2>
          <ul className="list">
            {skills.problems.map((problem, at) => (
              <li className="setting" key={at}>
                <div className="setting-line">
                  <span className="row-main">
                    <span className="setting-help">{problem.message}</span>
                    {problem.path && <span className="row-meta is-mono">{problem.path}</span>}
                  </span>
                </div>
              </li>
            ))}
          </ul>
        </section>
      )}
      <Folders folders={skills.folders} actions={actions} />
    </>
  );
}

function SkillRow({ skill, actions }: { skill: Skill; actions: Actions }) {
  const key = `skill:${skill.path}`;
  const on = key in actions.drafts ? actions.drafts[key] === true : skill.enabled;
  const builtin = skill.source === "builtin";
  return (
    <li className="setting">
      <div className="row-line">
        <button
          className="row-body is-button skill-body"
          type="button"
          data-tip="Open SKILL.md"
          data-tip-align="start"
          onClick={() => post({ type: "openFile", path: skill.path })}
        >
          <span className="row-main">
            <span className="row-name">
              {skill.name}
              {on !== skill.active && (
                <span className="tag" data-tip="Applies when the agent restarts">
                  restart
                </span>
              )}
            </span>
            <span className="skill-description">{skill.description}</span>
            {!builtin && <Location path={skill.location} />}
            {builtin && !on && <span className="row-meta">Off in settings.json</span>}
          </span>
          <IconExternal className="row-end skill-open" />
        </button>
        {!builtin && (
          <Switch
            on={on}
            label={`Use ${skill.name}`}
            disabled={!actions.ready}
            onChange={(enabled) => actions.act(key, enabled, { type: "setSkill", path: skill.path, enabled })}
          />
        )}
      </div>
      {actions.errors[key] && <p className="error setting-error">{actions.errors[key]}</p>}
    </li>
  );
}

/** A folder's path, its icon saying what it is. */
function Location({ path }: { path: string }) {
  return (
    <span className="location">
      <IconFolder />
      <span>{path}</span>
    </span>
  );
}

/** A new skill in the agent's skills folder; its file opens once written. */
function NewSkill({ actions, onDone }: { actions: Actions; onDone: () => void }) {
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const key = "skills/create";
  const busy = key in actions.drafts;
  const sent = useSettled(key, actions, onDone);
  const trimmed = name.trim();
  const problem =
    trimmed && !NAME.test(trimmed) ? "Lowercase letters, digits and single hyphens, like pdf-reports." : undefined;
  const complete = trimmed !== "" && description.trim() !== "" && !problem;

  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    if (!complete || busy) return;
    sent();
    actions.act(key, true, { type: "createSkill", name: trimmed, description: description.trim() });
  };

  return (
    <form className="row-form" onSubmit={submit} onKeyDown={(event) => event.key === "Escape" && onDone()}>
      <label className="form-field">
        <span className="form-label">Name</span>
        <input
          className="input"
          placeholder="e.g. pdf-reports"
          spellCheck={false}
          autoFocus
          maxLength={64}
          value={name}
          onChange={(event) => setName(event.target.value)}
        />
      </label>
      <label className="form-field">
        <span className="form-label">Description</span>
        <input
          className="input"
          placeholder="e.g. Write PDF reports from data. Use when asked for a report."
          maxLength={1024}
          value={description}
          onChange={(event) => setDescription(event.target.value)}
        />
        <span className="form-help">What it does and when to use it: the agent picks skills by it.</span>
      </label>
      {problem && <p className="error">{problem}</p>}
      {actions.errors[key] && <p className="error">{actions.errors[key]}</p>}
      <div className="actions">
        <button className="button" type="submit" disabled={!complete || busy || !actions.ready}>
          {busy ? "Creating" : "Create and open"}
        </button>
        <button className="button is-quiet" type="button" onClick={onDone}>
          Cancel
        </button>
      </div>
    </form>
  );
}

/** The folders added to the `skills` setting; the usual ones are always read. */
function Folders({ folders, actions }: { folders: string[]; actions: Actions }) {
  const [path, setPath] = useState("");
  const key = "skills/folder";
  const busy = key in actions.drafts;
  const sent = useSettled(key, actions, () => setPath(""));

  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    if (!path.trim() || busy) return;
    sent();
    actions.act(key, true, { type: "skillFolder", path: path.trim(), add: true });
  };

  return (
    <section className="section">
      <h2 className="section-label">Folders</h2>
      <p className="note section-help">
        The agent reads ~/.yusagent/skills, ~/.agents/skills, and the workspace's .yusagent/skills and
        .agents/skills. It also reads the folders added here.
      </p>
      <ul className="list">
        {folders.map((folder) => (
          <li className="row" key={folder}>
            <div className="row-line">
              <span className="row-body">
                <span className="row-main">
                  <Location path={folder} />
                </span>
              </span>
              <button
                className="icon-button"
                type="button"
                data-tip="Remove"
                data-tip-align="end"
                aria-label={`Remove ${folder}`}
                disabled={!actions.ready || busy}
                onClick={() => actions.act(key, true, { type: "skillFolder", path: folder, add: false })}
              >
                <IconCross />
              </button>
            </div>
          </li>
        ))}
        <li>
          <form className="folder-add" onSubmit={submit}>
            <input
              className="input is-mono"
              placeholder="e.g. ~/.claude/skills"
              aria-label="Folder to add"
              spellCheck={false}
              value={path}
              onChange={(event) => setPath(event.target.value)}
            />
            <button className="button" type="submit" disabled={!path.trim() || busy || !actions.ready}>
              Add
            </button>
          </form>
        </li>
      </ul>
      {actions.errors[key] && <p className="error">{actions.errors[key]}</p>}
    </section>
  );
}

import { useEffect, useState, type SubmitEvent } from "react";
import type { LoginState } from "../src/protocol";
import { IconBack, IconCheck, IconCopy, IconCross, IconExternal, IconRestart } from "./icons";
import { post } from "./vscode";

/** The device flows (xAI, GitHub Copilot) give their code this way. */
const DEVICE_CODE = /^Enter code:\s*(\S+)$/;

type PromptUpdate = NonNullable<LoginState["prompt"]>;

/**
 * A browser sign-in. One action: open the page; the panel moves on by itself
 * once the provider has answered. A paste that races the browser (a redirect
 * URL, a Prime API key) stays behind a link: it is only needed when the
 * browser cannot reach the agent (VS Code remote, callback port taken).
 */
export function SignIn({ login }: { login: LoginState }) {
  const failed = login.error !== undefined;
  const cancel = () => post({ type: "loginCancel" });
  const close = () => post({ type: "loginClose" });
  const optional = login.url && login.prompt?.optional ? login.prompt : undefined;
  const required = optional ? undefined : login.prompt;

  return (
    <div className="screen">
      <header className="topbar">
        <div className="topbar-title">
          <button
            className="icon-button"
            type="button"
            aria-label={failed ? "Back" : "Cancel sign-in"}
            data-tip={failed ? "Back" : "Cancel sign-in"}
            data-tip-side="below"
            data-tip-align="start"
            onClick={failed ? close : cancel}
          >
            {failed ? <IconBack /> : <IconCross />}
          </button>
          <span>{login.name}</span>
        </div>
      </header>
      <main className="content">
        {failed ? (
          <>
            <p className="error">{login.error}</p>
            <div className="actions">
              <button className="button" type="button" onClick={() => post({ type: "login", providerId: login.providerId })}>
                <IconRestart />
                Try again
              </button>
              <button className="button is-quiet" type="button" onClick={close}>
                Back
              </button>
            </div>
          </>
        ) : (
          <>
            {login.url && <Page url={login.url} />}
            {required && <Prompt key={required.promptId} prompt={required} label={required.message} />}
            {login.team && <TeamPicker key={login.team.promptId} team={login.team} />}
            {!required && !login.team && (
              <div className="status-line">
                <span className="spinner" />
                <span>{login.status?.message ?? (login.url ? "Waiting for you to sign in" : "Starting sign-in")}</span>
              </div>
            )}
            {optional && <Fallback key={optional.promptId} prompt={optional} providerId={login.providerId} />}
            <button className="button is-quiet" type="button" onClick={cancel}>
              Cancel
            </button>
          </>
        )}
      </main>
    </div>
  );
}

function Page({ url }: { url: NonNullable<LoginState["url"]> }) {
  const code = url.instructions?.match(DEVICE_CODE)?.[1];
  const open = () => {
    if (code) post({ type: "copy", text: code });
    post({ type: "loginOpen" });
  };

  return (
    <section className="step">
      {code ? (
        <>
          <h2 className="step-title">Enter this code on the sign-in page</h2>
          <div className="code-line">
            <span className="code">{code}</span>
            <CopyCode code={code} />
          </div>
        </>
      ) : (
        <>
          <h2 className="step-title">Sign in with your browser</h2>
          <p className="note">This panel moves on by itself once you are signed in.</p>
        </>
      )}
      <div className="actions">
        <button className="button" type="button" onClick={open}>
          <IconExternal />
          {code ? "Copy code and open the page" : "Open the sign-in page"}
        </button>
      </div>
    </section>
  );
}

/** The paste racing the browser, folded until asked for. */
function Fallback({ prompt, providerId }: { prompt: PromptUpdate; providerId: string }) {
  const [open, setOpen] = useState(false);
  // Prime's alternative is a key, the others' a redirect the browser could not deliver.
  const apiKey = providerId === "prime-inference";
  if (!open) {
    return (
      <button className="link" type="button" onClick={() => setOpen(true)}>
        {apiKey ? "Use an API key instead" : "Signed in, but nothing happens?"}
      </button>
    );
  }
  return (
    <Prompt
      prompt={prompt}
      label={
        apiKey
          ? "Prime API key"
          : "Copy the address of the page your browser ended on (it may not load) and paste it here."
      }
      secret={apiKey}
    />
  );
}

function Prompt({ prompt, label, secret = prompt.secret }: { prompt: PromptUpdate; label: string; secret?: boolean }) {
  const [value, setValue] = useState("");
  const ready = prompt.allowEmpty || value.trim() !== "";
  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    if (ready) post({ type: "loginAnswer", promptId: prompt.promptId, answer: { kind: "text", value } });
  };

  return (
    <form className="step" onSubmit={submit}>
      <label className="note" htmlFor="sign-in-answer">
        {label}
      </label>
      <input
        id="sign-in-answer"
        className={secret ? "input is-secret" : "input"}
        type={secret ? "password" : "text"}
        autoComplete="off"
        spellCheck={false}
        autoFocus
        value={value}
        onChange={(event) => setValue(event.target.value)}
      />
      <div className="actions">
        <button className="button" type="submit" disabled={!ready}>
          Continue
        </button>
      </div>
    </form>
  );
}

function TeamPicker({ team }: { team: NonNullable<LoginState["team"]> }) {
  const answer = (teamId: string | null) =>
    post({ type: "loginAnswer", promptId: team.promptId, answer: { kind: "team", teamId } });
  const options = [{ id: null, name: "Personal account", role: null }, ...team.teams];

  return (
    <section className="step">
      <h2 className="step-title">Choose a team</h2>
      <ul className="list">
        {options.map((option) => (
          <li className="row" key={option.id ?? ""}>
            <button className="row-body is-button" type="button" onClick={() => answer(option.id)}>
              <span className="row-main">
                <span className="row-name">{option.name}</span>
                {option.role && <span className="row-meta">{option.role}</span>}
              </span>
              {option.id === team.current && <IconCheck className="row-end" />}
            </button>
          </li>
        ))}
      </ul>
      <button
        className="button is-quiet"
        type="button"
        onClick={() => post({ type: "loginAnswer", promptId: team.promptId, answer: { kind: "dismiss" } })}
      >
        Keep the current choice
      </button>
    </section>
  );
}

function CopyCode({ code }: { code: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1200);
    return () => clearTimeout(timer);
  }, [copied]);
  const copy = () => {
    post({ type: "copy", text: code });
    setCopied(true);
  };

  return (
    <button className="icon-button" type="button" aria-label="Copy code" data-tip={copied ? "Copied" : "Copy code"} onClick={copy}>
      {copied ? <IconCheck /> : <IconCopy />}
    </button>
  );
}

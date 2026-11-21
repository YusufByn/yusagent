import { useEffect, useReducer, useRef, useState, type CSSProperties } from "react";
import type { KernelCommand, KernelState, KernelVariable, VariablePreview } from "../src/protocol";
import { IconCheck, IconChevronUp, IconCross, IconPython, IconTerminal } from "./icons";
import { onHostMessage, post } from "./vscode";

type List = "commands" | "variables";

/** How a command stands on the list: running, ended well, stopped, or ended otherwise. */
type Phase = "running" | "ok" | "killed" | "ended";

type CommandRow = {
  command: KernelCommand;
  phase: Phase;
  /** It ended while on the list: a light runs through it. */
  lit: number;
  leaving: boolean;
};

type VariableRow = { variable: KernelVariable; lit: number; leaving: boolean };

type Line = { key: number; text: string; fresh: boolean };

/** A command's lines the panel shows. */
const TAIL_LINES = 7;
/** An ended command stays this long, then goes; a stopped one, less. */
const OK_STAYS_MS = 2800;
const KILLED_STAYS_MS = 900;
/** The row's own closing, as `.kernel-row-wrap.is-leaving` animates it. */
const LEAVE_MS = 460;
const TAIL_EVERY_MS = 1000;
const BUSY_RETRY_MS = 1500;

function phaseOf(command: KernelCommand, stopping: boolean): Phase {
  if (command.killed || stopping) return "killed";
  if (command.status === "running") return "running";
  return command.exitCode === 0 ? "ok" : "ended";
}

/** `12s`, `4m`, `1h05`. */
function duration(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  return m < 60 ? `${m}m` : `${Math.floor(m / 60)}h${String(m % 60).padStart(2, "0")}`;
}

function timeText(row: CommandRow, now: number): string {
  const { command } = row;
  switch (row.phase) {
    case "running":
      return duration(now - Date.parse(command.startedAt));
    case "ok":
    case "ended":
      return duration(command.durationMs ?? 0);
    case "killed":
      return "stopped";
  }
}

const UNITS: Record<NonNullable<KernelVariable["unit"]>, [string, string]> = {
  items: ["item", "items"],
  keys: ["key", "keys"],
  chars: ["char", "chars"],
  bytes: ["byte", "bytes"],
};

/** What a variable holds, in a few characters: its value when short, else its signature or size. */
function metaOf(variable: KernelVariable): string {
  if (variable.value !== undefined) return variable.value;
  if (variable.signature !== undefined) return variable.signature;
  if (variable.shape) {
    const shape = variable.shape.map((n) => n.toLocaleString()).join(" × ");
    return variable.dtype ? `${shape} ${variable.dtype}` : shape;
  }
  if (variable.len !== undefined && variable.unit) {
    const [one, many] = UNITS[variable.unit];
    return `${variable.len.toLocaleString()} ${variable.len === 1 ? one : many}`;
  }
  return "";
}

/** Lines that came since `before`: the new tail is the old one moved up. */
function nextLines(before: Line[], text: string, seq: { current: number }): Line[] {
  if (text === "") return [];
  const texts = text.split("\n").slice(-TAIL_LINES);
  let shift = 0;
  while (shift < before.length && !before.slice(shift).every((line, i) => texts[i] === line.text)) shift++;
  const kept = before.length - shift;
  return texts.map((line, i) =>
    i < kept ? { ...before[shift + i], fresh: false } : { key: seq.current++, text: line, fresh: before.length > 0 },
  );
}

/**
 * Over the message box: a pill for the commands the agent left running in
 * the background, one for the variables of its REPL. A pill opens its list
 * above; a row opens a command's last lines or a value's start. Only the
 * commands that serve, listening on a port, are listed, while they run: one
 * that ends well lights up and goes, any other goes at once.
 */
export function KernelTray({ kernel }: { kernel: KernelState }) {
  const [open, setOpen] = useState<List | null>(null);
  const openRef = useRef(open);
  openRef.current = open;
  const [, render] = useReducer((n: number) => n + 1, 0);
  const commands = useRef(new Map<string, CommandRow>());
  const variables = useRef(new Map<string, VariableRow>());
  const [flash, setFlash] = useState({ commands: 0, variables: 0 });
  const timers = useRef(new Set<number>());
  const later = (ms: number, run: () => void) => {
    const timer = window.setTimeout(() => {
      timers.current.delete(timer);
      run();
    }, ms);
    timers.current.add(timer);
  };
  useEffect(() => () => timers.current.forEach((timer) => clearTimeout(timer)), []);

  /** The lists the panel already had: what it opens on is not news. */
  const primed = useRef({ commands: false, variables: false });
  const flashPill = (list: List) => {
    if (primed.current[list] && openRef.current !== list) {
      setFlash((flash) => ({ ...flash, [list]: flash[list] + 1 }));
    }
  };

  const leave = (rows: Map<string, { leaving: boolean }>, key: string) => {
    const row = rows.get(key);
    if (!row || row.leaving) return;
    row.leaving = true;
    render();
    later(LEAVE_MS, () => {
      if (rows.get(key) === row) rows.delete(key);
      render();
    });
  };

  const dismiss = (id: string) => {
    post({ type: "kernelDismiss", id });
    leave(commands.current, id);
  };

  /** Shown stopped at once; the agent's list then says it ended. */
  const stop = (row: CommandRow) => {
    row.phase = "killed";
    render();
    post({ type: "kernelKill", id: row.command.id });
  };

  // The agent's list against the rows: new ones grow in, ended ones say how
  // they ended, the ones it dropped leave.
  useEffect(() => {
    const rows = commands.current;
    for (const command of kernel.commands) {
      const row = rows.get(command.id);
      if (!row) {
        // Ended before the panel saw it serve: nothing to show.
        if (command.status !== "running") {
          post({ type: "kernelDismiss", id: command.id });
          continue;
        }
        // Not serving (yet): a build or a test run ends on its own.
        if (command.ports.length === 0) continue;
        rows.set(command.id, { command, phase: phaseOf(command, false), lit: 0, leaving: false });
        flashPill("commands");
        continue;
      }
      const ended = row.command.status === "running" && command.status === "finished";
      row.command = command;
      row.phase = phaseOf(command, row.phase === "killed");
      if (!ended || row.leaving) continue;
      if (row.phase === "ok") {
        row.lit++;
        flashPill("commands");
        later(OK_STAYS_MS, () => dismiss(command.id));
      } else if (row.phase === "killed") {
        later(KILLED_STAYS_MS, () => dismiss(command.id));
      } else {
        dismiss(command.id);
      }
    }
    for (const id of rows.keys()) if (!kernel.commands.some((command) => command.id === id)) leave(rows, id);
    primed.current.commands = true;
    render();
  }, [kernel.commands]);

  useEffect(() => {
    const rows = variables.current;
    for (const variable of kernel.variables) {
      const row = rows.get(variable.name);
      if (!row || row.leaving) {
        const lit = primed.current.variables && openRef.current === "variables" ? 1 : 0;
        rows.set(variable.name, { variable, lit, leaving: false });
        flashPill("variables");
        continue;
      }
      const changed = row.variable.rev !== variable.rev || metaOf(row.variable) !== metaOf(variable);
      row.variable = variable;
      if (changed) {
        row.lit++;
        flashPill("variables");
      }
    }
    for (const name of rows.keys()) if (!kernel.variables.some((variable) => variable.name === name)) leave(rows, name);
    primed.current.variables = true;
    render();
  }, [kernel.variables]);

  const commandRows = [...commands.current.values()];
  const variableRows = [...variables.current.values()];
  const listed = commandRows.filter((row) => !row.leaving);
  const running = listed.filter((row) => row.phase === "running").length;
  const variableCount = variableRows.filter((row) => !row.leaving).length;

  // A list that empties closes.
  useEffect(() => {
    if ((open === "commands" && !listed.length) || (open === "variables" && !variableCount)) setOpen(null);
  }, [open, listed.length, variableCount]);

  const toggle = (list: List) => setOpen(open === list ? null : list);
  const shown = listed.length > 0 || variableCount > 0;
  const status = running ? "st-running" : "st-ok";

  return (
    <div className={shown ? "kernel-collapse is-shown" : "kernel-collapse"} aria-hidden={!shown}>
      <div>
        <div className="kernel-tray">
          <div className={open === "commands" ? "kernel-collapse is-shown" : "kernel-collapse"}>
            <div>
              <div className="kernel-stack">
                <div className="kernel-title">Running servers</div>
                {commandRows.map((row) => (
                  <CommandLine
                    key={row.command.id}
                    row={row}
                    active={open === "commands"}
                    onStop={() => stop(row)}
                    onDismiss={dismiss}
                  />
                ))}
              </div>
            </div>
          </div>
          <div className={open === "variables" ? "kernel-collapse is-shown" : "kernel-collapse"}>
            <div>
              <div className="kernel-stack">
                <div className="kernel-title">Agent's REPL variables</div>
                {variableRows.map((row) => (
                  <VariableLine key={row.variable.name} row={row} active={open === "variables"} />
                ))}
              </div>
            </div>
          </div>
          <div className="kernel-pills">
            {listed.length > 0 && (
              <button
                className={`kernel-pill ${status}${open === "commands" ? " is-open" : ""}`}
                type="button"
                aria-expanded={open === "commands"}
                aria-label={`${listed.length} running ${listed.length === 1 ? "server" : "servers"}`}
                onClick={() => toggle("commands")}
              >
                <IconTerminal className="kernel-logo" />
                {running > 0 ? (
                  <Lit className="kernel-count" text={String(running)} lit={flash.commands} back />
                ) : (
                  <IconCheck className="kernel-done" />
                )}
                <IconChevronUp className="kernel-chev" />
              </button>
            )}
            {variableCount > 0 && (
              <button
                className={`kernel-pill st-running${open === "variables" ? " is-open" : ""}`}
                type="button"
                aria-expanded={open === "variables"}
                aria-label={`${variableCount} REPL ${variableCount === 1 ? "variable" : "variables"}`}
                onClick={() => toggle("variables")}
              >
                <IconPython className="kernel-logo" />
                <Lit className="kernel-count" text={String(variableCount)} lit={flash.variables} back />
                <IconChevronUp className="kernel-chev" />
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

/** A text a light runs through, letter after letter, each time `lit` grows; `back` ends it grey again. */
function Lit({ text, lit, back = false, className }: { text: string; lit: number; back?: boolean; className: string }) {
  if (!lit) return <span className={className}>{text}</span>;
  return (
    <span className={className} key={lit}>
      {[...text].map((char, i) => (
        <span key={i} className={back ? "kernel-ch is-back" : "kernel-ch"} style={{ "--i": i } as CSSProperties}>
          {char}
        </span>
      ))}
    </span>
  );
}

/** A command: how it ended, its text, its ports, its time, its stop; open, its last lines. */
function CommandLine({
  row,
  active,
  onStop,
  onDismiss,
}: {
  row: CommandRow;
  active: boolean;
  onStop: () => void;
  onDismiss: (id: string) => void;
}) {
  const { command, phase } = row;
  const [open, setOpen] = useState(false);
  const [lines, setLines] = useState<Line[]>([]);
  const seq = useRef(0);
  const [now, setNow] = useState(Date.now);
  const watching = open && active;

  // The time counts while the list shows it.
  useEffect(() => {
    if (!active || phase !== "running") return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [active, phase]);

  useEffect(
    () =>
      onHostMessage((message) => {
        if (message.type !== "kernelTail" || message.id !== command.id || message.tail === null) return;
        const { tail } = message;
        setLines((before) => nextLines(before, tail, seq));
      }),
    [command.id],
  );

  // Open, the lines follow the command while it runs, and once more when it ends.
  useEffect(() => {
    if (!watching) return;
    post({ type: "kernelTail", id: command.id });
    if (command.status !== "running") return;
    const timer = setInterval(() => post({ type: "kernelTail", id: command.id }), TAIL_EVERY_MS);
    return () => clearInterval(timer);
  }, [watching, command.id, command.status]);

  const stop = () => (phase === "running" ? onStop() : onDismiss(command.id));
  // Stopped from here or by the agent: the interrupt closes its lines.
  const shown = phase === "killed" ? [...lines, { key: -1, text: "^C", fresh: true }].slice(-TAIL_LINES) : lines;

  return (
    <div className={`kernel-row-wrap st-${phase}${row.leaving ? " is-leaving" : ""}`}>
      <div className={open ? "kernel-row-inner is-open" : "kernel-row-inner"}>
        <div className="kernel-row" onClick={() => setOpen(!open)}>
          <span className="kernel-mark" aria-hidden="true">
            <IconCheck className="kernel-mark-ok" />
          </span>
          <Lit className="kernel-cmd" text={command.command} lit={row.lit} />
          {phase === "running" &&
            command.ports.slice(0, 2).map((port) => (
              <a
                className="kernel-port"
                key={port}
                href={`http://localhost:${port}/`}
                title={`Open localhost:${port}`}
                onClick={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                  post({ type: "openLink", url: `http://localhost:${port}/` });
                }}
              >
                :{port}
              </a>
            ))}
          <span className="kernel-time">{timeText(row, now)}</span>
          <button
            className="kernel-x"
            type="button"
            title={phase === "running" ? "Stop" : "Dismiss"}
            aria-label={phase === "running" ? "Stop" : "Dismiss"}
            disabled={phase === "killed"}
            onClick={(event) => {
              event.stopPropagation();
              stop();
            }}
          >
            <IconCross />
          </button>
        </div>
        <div className="kernel-tail-wrap">
          <div>
            <div className="kernel-tail">
              {shown.map((line) => (
                <div key={line.key} className={line.fresh ? "is-new" : undefined}>
                  {line.text || " "}
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

/** A variable: its name, type, and size or short value; open, the start of its value. */
function VariableLine({ row, active }: { row: VariableRow; active: boolean }) {
  const { variable } = row;
  const [open, setOpen] = useState(false);
  const [preview, setPreview] = useState<VariablePreview | null>(null);
  /** Bumped to ask again: the value was not read while a cell ran. */
  const [retry, setRetry] = useState(0);
  const meta = metaOf(variable);

  useEffect(
    () =>
      onHostMessage((message) => {
        if (message.type !== "kernelPreview" || message.name !== variable.name) return;
        const next = message.preview;
        const busy = "error" in next && next.error === "busy";
        // A cell runs: the value shown before stays, and it is asked again soon.
        setPreview((before) => (busy && before && "text" in before ? before : next));
        if (busy) setTimeout(() => setRetry((retry) => retry + 1), BUSY_RETRY_MS);
      }),
    [variable.name],
  );

  // Asked again each time it opens, and when the cell changes it.
  useEffect(() => {
    if (open && active) post({ type: "kernelPreview", name: variable.name });
  }, [open, active, variable.name, variable.rev, meta, retry]);

  let body: { text: string; style: string };
  if (!preview) body = { text: "…", style: "is-quiet" };
  else if ("error" in preview)
    body = {
      text: preview.error === "busy" ? "Shown once the agent's code is done" : preview.error,
      style: "is-quiet",
    };
  else body = { text: preview.truncated ? `${preview.text}\n…` : preview.text, style: `is-${preview.style}` };

  return (
    <div className={row.leaving ? "kernel-row-wrap is-leaving" : "kernel-row-wrap"}>
      <div className={open ? "kernel-row-inner is-open" : "kernel-row-inner"}>
        <div className="kernel-row is-var" onClick={() => setOpen(!open)}>
          <Lit className="kernel-vname" text={variable.name} lit={row.lit} back />
          <span className="kernel-vtype">{variable.type}</span>
          <span className="kernel-vmeta" title={meta}>
            {meta}
          </span>
        </div>
        <div className="kernel-tail-wrap">
          <div>
            <div className={`kernel-tail kernel-preview ${body.style}`}>
              {body.text.split("\n").map((line, i) => (
                <div key={i}>{line || " "}</div>
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

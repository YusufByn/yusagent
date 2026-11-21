// yusAgent (YA-10, ticket 24, PR 3) : ce que la carte d'une cellule montre en plus de celle
// d'Unharnessed, d'après les données de notre pont (`src/ya/cells.ts`) :
// - C6 : une ligne par `bash()` lu (commande, code de sortie, durée), sortie repliée ;
// - C7 : les vrais diffs, un bloc par fichier, à la place du diff deviné du code.
// Visuel provisoire (passe de design plus tard).
import { useState } from "react";

import type { BashRun, FileEdit } from "../src/protocol";

/** Pour l'en-tête : `exit 1` du premier `bash()` sorti en erreur, sinon undefined. */
export function exitMeta(runs: BashRun[] | undefined): string | undefined {
  const failed = runs?.find((run) => run.exitCode !== null && run.exitCode !== 0);
  return failed ? `exit ${failed.exitCode}` : undefined;
}

export function BashRuns({ runs }: { runs: BashRun[] }) {
  return (
    <div className="call-part is-sunk ya-runs">
      {runs.map((run, at) => (
        <BashRunLine key={at} run={run} />
      ))}
    </div>
  );
}

function BashRunLine({ run }: { run: BashRun }) {
  const [open, setOpen] = useState(false);
  const output = run.output.replace(/\n+$/, "");
  const failed = run.exitCode !== null && run.exitCode !== 0;
  const head = (
    <>
      <span className="ya-run-command">$ {run.command.trim()}</span>
      <span className="call-meta">
        <span className={failed ? "ya-run-exit is-error" : "ya-run-exit"}>exit {run.exitCode ?? "?"}</span>
        {run.durationMs !== undefined && (
          <>
            <span className="sep">·</span>
            {duration(run.durationMs)}
          </>
        )}
        {!output && (
          <>
            <span className="sep">·</span>
            no output
          </>
        )}
      </span>
    </>
  );
  return (
    <div className={`ya-run${open ? " is-open" : ""}`}>
      {output ? (
        <button className="ya-run-head" type="button" data-no-press aria-expanded={open} onClick={() => setOpen(!open)}>
          {head}
        </button>
      ) : (
        <div className="ya-run-head">{head}</div>
      )}
      {open && output && (
        <div className="call-lines is-output">
          {output.split("\n").map((line, at) => (
            <div key={at} className="call-line">
              {line || " "}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export function FileEdits({ edits }: { edits: FileEdit[] }) {
  return (
    <>
      {edits.map((edit) => (
        <div key={edit.path} className="is-sunk ya-edit">
          <div className="call-diff-head">
            <span title={edit.path}>{edit.shown}</span>
            <span className="call-meta">
              {edit.created && (
                <>
                  created<span className="sep">·</span>
                </>
              )}
              <span className="add">+{edit.added}</span>
              {!edit.created && (
                <>
                  {" "}
                  <span className="del">−{edit.removed}</span>
                </>
              )}
            </span>
          </div>
          <div className="call-lines">
            {edit.lines.map(([sign, text], at) => (
              <div key={at} className={sign === "+" ? "call-line is-add" : sign === "-" ? "call-line is-del" : "call-line"}>
                <span className="gutter">{sign === "+" ? "+" : sign === "-" ? "−" : ""}</span>
                {sign === "…" ? "…" : text || " "}
              </div>
            ))}
          </div>
        </div>
      ))}
    </>
  );
}

/** `12 ms`, `1.4 s`. */
function duration(ms: number): string {
  return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`;
}

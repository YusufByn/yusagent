// yusAgent (YA-10, ticket 24, PR 3) : ce que la carte d'une cellule montre en plus de celle
// d'Unharnessed, d'après les données de notre pont (`src/ya/cells.ts`) :
// - C6 : une ligne par `bash()` lu (commande, code de sortie, durée), sortie repliée ;
// - C7 : les vrais diffs, un bloc par fichier, à la place du diff deviné du code ;
// - étape 4 (E6 a) : le nom du fichier ouvre l'avant et l'après de la cellule dans `vscode.diff`
//   (C10) ; « Revert » restaure le fichier, grisé pendant un tour (garde-fou 1), avec la raison
//   d'un refus ou d'une impossibilité.
// Visuel provisoire (passe de design plus tard).
import { createContext, useContext, useState } from "react";

import type { BashRun, FileEdit, RevertStatus } from "../src/protocol";
import { post } from "./vscode";

/** Un tour tourne (le chat est occupé) : « Revert » attend (garde-fou 1). Fourni par `Status.tsx`. */
export const YaBusyContext = createContext(false);

/**
 * Séparateur avec ses espaces, écrits ici : sans eux (feuille provisoire), `exit 0` puis `32 ms`
 * se lisaient « exit 0·32 ms » (contrôle de Yusuf).
 */
const SEP = " · ";

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
            <span className="sep">{SEP}</span>
            {duration(run.durationMs)}
          </>
        )}
        {!output && (
          <>
            <span className="sep">{SEP}</span>
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

export function FileEdits({ toolId, edits }: { toolId: string; edits: FileEdit[] }) {
  return (
    <>
      {edits.map((edit) => (
        <div key={edit.path} className="is-sunk ya-edit">
          <div className="call-diff-head">
            {opensDiff(edit.revert) ? (
              <button
                className="ya-edit-file"
                type="button"
                title={`${edit.path}: open before and after the cell`}
                onClick={() => post({ type: "yaOpenDiff", toolId, path: edit.path })}
              >
                {edit.shown}
              </button>
            ) : (
              <span title={edit.path}>{edit.shown}</span>
            )}
            <span className="call-meta">
              {edit.created && (
                <>
                  created<span className="sep">{SEP}</span>
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
            <Revert toolId={toolId} edit={edit} />
          </div>
          <RevertNote status={edit.revert} />
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

/** « Revert », ou ce qu'il en est (rien tant que l'après de la cellule n'est pas lu). */
function Revert({ toolId, edit }: { toolId: string; edit: FileEdit }) {
  const busy = useContext(YaBusyContext);
  const status = edit.revert;
  if (status?.state === "reverted") return <span className="call-meta ya-reverted">Reverted</span>;
  if (status?.state !== "ready") return null;
  return (
    <button
      className="button is-secondary ya-revert"
      type="button"
      disabled={busy}
      title={busy ? "Available once no turn is running" : "Restore the file as it was before this cell"}
      onClick={() => post({ type: "yaRevert", toolId, path: edit.path })}
    >
      Revert
    </button>
  );
}

function RevertNote({ status }: { status: RevertStatus | undefined }) {
  if (status?.state === "blocked") return <p className="note ya-revert-note">Revert unavailable: {status.reason}.</p>;
  if (status?.state === "ready" && status.refused) return <p className="note ya-revert-note is-warning">{status.refused}</p>;
  if (status?.state === "reverted") return <p className="note ya-revert-note">The agent learns it with your next message.</p>;
  return null;
}

/** L'avant de la cellule a été retrouvé : le diff s'ouvre. */
function opensDiff(status: RevertStatus | undefined): boolean {
  return status?.state === "ready" || status?.state === "reverted";
}

/** `12 ms`, `1.4 s`. */
function duration(ms: number): string {
  return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`;
}

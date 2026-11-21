import { useEffect, useRef, useState } from "react";
import type { Usage } from "../src/protocol";
import { tokens } from "./models";
import { Popover } from "./Popover";

/** The ring on a 20 grid: its radius, and its circumference. */
const R = 7.25;
const C = 2 * Math.PI * R;
/** From this share of the compaction's threshold on, the ring tells that one comes. */
const NEAR = 0.9;
/** How long the ripple of a compaction that landed takes (styles.css, .ring-ripple). */
const RIPPLE_MS = 800;

/**
 * The context gauge beside send: a ring that fills as the conversation
 * takes up the model's window. Dotted while no reply has measured the
 * context, an orbit while a compaction runs, cyan when a compaction comes;
 * a ripple of cyan leaves it when one lands. Hovered or pressed, a card tells how full the context is and how much the
 * provider's cache took; not while the conversation is empty.
 */
export function UsageRing({ usage, empty }: { usage: Usage; empty: boolean }) {
  const [hover, setHover] = useState(false);
  const [pinned, setPinned] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  // A short wait both ways: passing over the ring opens nothing, and the
  // pointer crosses the gap to the card without closing it.
  const hold = (on: boolean) => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setHover(on), on ? 140 : 160);
  };
  const open = !empty && (hover || pinned);
  const ripple = useRipple(usage);

  const gauge = gaugeOf(usage);
  return (
    <div
      className="usage"
      onPointerEnter={() => !empty && hold(true)}
      onPointerLeave={() => hold(false)}
    >
      {open && (
        <Popover
          className="usage-card"
          trigger="usage"
          onClose={() => {
            setPinned(false);
            setHover(false);
          }}
        >
          <Context usage={usage} gauge={gauge} />
        </Popover>
      )}
      <button
        className={`usage-ring ${gauge.state}`}
        type="button"
        aria-label="Context and cost"
        aria-expanded={open}
        disabled={empty}
        data-menu-trigger="usage"
        onClick={() => setPinned((on) => !on)}
      >
        <svg className="ring" viewBox="0 0 20 20" width={20} height={20} aria-hidden="true">
          <circle className="ring-track" cx="10" cy="10" r={R} />
          <circle
            className={gauge.share > 0 ? "ring-fill" : "ring-fill is-empty"}
            cx="10"
            cy="10"
            r={R}
            strokeDasharray={C}
            strokeDashoffset={C * (1 - gauge.share)}
            transform="rotate(-90 10 10)"
          />
          <circle className="ring-orbit" cx="10" cy="10" r={R} strokeDasharray={`${C * 0.26} ${C}`} />
        </svg>
      </button>
      {ripple > 0 && <span key={ripple} className="ring-ripple" aria-hidden="true" />}
    </div>
  );
}

/**
 * A count bumped each time a compaction lands (it ran: the context is no
 * longer measured), 0 once its ripple is over.
 */
function useRipple({ compacting, tokens: used }: Usage): number {
  const [ripple, setRipple] = useState(0);
  const was = useRef(compacting);
  useEffect(() => {
    const landed = was.current && !compacting && used === undefined;
    was.current = compacting;
    if (landed) setRipple((n) => n + 1);
  }, [compacting, used]);
  useEffect(() => {
    if (ripple === 0) return;
    const timer = window.setTimeout(() => setRipple(0), RIPPLE_MS);
    return () => window.clearTimeout(timer);
  }, [ripple]);
  return ripple;
}

type Gauge = {
  /** How much of the window the context takes, 0 to 1. */
  share: number;
  state: "" | "is-near" | "is-unmeasured" | "is-compacting";
};

function gaugeOf({ tokens: used, window, compactAt, compacting }: Usage): Gauge {
  const share = used !== undefined && window > 0 ? Math.min(1, used / window) : 0;
  const near = used !== undefined && compactAt !== undefined && used >= compactAt * NEAR;
  const state = compacting ? "is-compacting" : used === undefined ? "is-unmeasured" : near ? "is-near" : "";
  return { share, state };
}

/** The context: how full, its bar, then its tokens over the window. */
function Context({ usage, gauge }: { usage: Usage; gauge: Gauge }) {
  const { tokens: used, window, compacting } = usage;
  const percent = used === undefined || window === 0 ? null : gauge.share * 100;
  return (
    <>
      <div className="usage-head">
        <span className="usage-title">Context</span>
        <span className="usage-figure">
          {percent === null ? (
            "—"
          ) : (
            <>
              {percent > 0 && percent < 1 ? "<1" : Math.round(percent)}%
            </>
          )}
        </span>
      </div>
      <div className={`usage-bar ${gauge.state}`}>
        <span className="usage-bar-fill" style={{ width: `${gauge.share * 100}%` }} />
      </div>
      <p className="usage-tokens">
        {compacting ? (
          "Compacting…"
        ) : (
          <>
            {used === undefined ? "—" : count(used)}
            {window > 0 ? ` of ${tokens(window)} tokens` : " tokens"}
          </>
        )}
      </p>
      {usage.cacheReusable > 0 && (
        <p className="usage-cache">
          <span>Cache {Math.round((usage.cacheReused / usage.cacheReusable) * 100)}%</span>
          <span>−{dollars(usage.cacheSaved)}</span>
        </p>
      )}
    </>
  );
}

/** 3.4 → `$3.40`, 0.0213 → `$0.021`: cents, or a tenth of a cent below a dime. */
function dollars(n: number): string {
  return `$${n.toFixed(n < 0.1 ? 3 : 2)}`;
}

/** 842 → `842`, 84213 → `84.2K`, 184213 → `184K`, 1250000 → `1.25M`. */
export function count(n: number): string {
  if (n < 1000) return String(n);
  if (n < 100_000) return `${(n / 1000).toFixed(1).replace(/\.0$/, "")}K`;
  if (n < 1_000_000) return `${Math.round(n / 1000)}K`;
  return `${(n / 1_000_000).toFixed(2).replace(/\.?0+$/, "")}M`;
}

import { useEffect, useRef, useState, type CSSProperties, type RefObject } from "react";
import type { Goal } from "../src/protocol";
import { IconCheck, IconCross, IconFlag } from "./icons";
import { reducedMotion } from "./motion";
import { post } from "./vscode";

/** Where the goal stands, as the box shows it. */
export type GoalPhase = "running" | "paused" | "done" | "failed";

export function goalPhase(goal: Goal | null): GoalPhase | null {
  switch (goal?.status) {
    case "active":
      return "running";
    case "paused":
    case "blocked":
    case "budget_limited":
      return "paused";
    case "complete":
      return "done";
    case "error":
      return "failed";
    default:
      return null;
  }
}

const STATES: Record<Goal["status"], string> = {
  active: "Goal",
  paused: "Paused",
  blocked: "Blocked",
  budget_limited: "Budget reached",
  complete: "Done",
  error: "Failed",
};

/**
 * On top of the box while there is a goal: where it stands and what it is
 * after, and the cross that ends it. It opens like a drawer; `lit` bumps on
 * a start or a play, and the flag goes up its pole.
 */
export function GoalHead({ goal, lit }: { goal: Goal | null; lit: number }) {
  // The last goal stays in while the drawer closes.
  const last = useRef(goal);
  if (goal) last.current = goal;
  const shown = last.current;
  const over = shown?.status === "complete" || shown?.status === "error";
  const end = over ? "Dismiss" : "End the goal";
  // A blocked goal says why where it says what it is after.
  const reason = shown?.status === "blocked" ? shown.lastReason : undefined;
  return (
    <div className="goal-head-wrap" aria-hidden={goal === null}>
      <div>
        {shown && (
          <div className="goal-head">
            <span className="goal-mark">
              <span className={lit > 0 ? "mark-run is-arriving" : "mark-run"} key={lit}>
                <IconFlag />
              </span>
              <span className="mark-pause">
                <i />
                <i />
              </span>
              <IconCheck className="mark-done" width={14} height={14} />
            </span>
            <span className="goal-state">{STATES[shown.status]}</span>
            <span className="goal-objective" title={reason ? `${reason}\n\n${shown.objective}` : shown.objective}>
              {reason ?? shown.objective}
            </span>
            <button
              className="icon-button goal-end"
              type="button"
              aria-label={end}
              data-tip={end}
              data-tip-align="end"
              tabIndex={goal ? undefined : -1}
              onClick={() => post({ type: "goal", action: "end" })}
            >
              <IconCross />
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

/**
 * The goal's timer and its pause / play, in place of the stop disc. Pause is
 * the stop square: the running turn stops and the goal waits. Paused, it
 * turns into the cyan play. Reached, a check and the time it took.
 */
export function GoalPill({ goal, phase, lit }: { goal: Goal; phase: GoalPhase; lit: number }) {
  const seconds = useGoalSeconds(goal, phase === "running");
  // One click per change: the next waits for the goal to move.
  const [pending, setPending] = useState(false);
  useEffect(() => setPending(false), [goal.status]);
  useEffect(() => {
    if (!pending) return;
    const timer = setTimeout(() => setPending(false), 5000);
    return () => clearTimeout(timer);
  }, [pending]);
  const tip = phase === "paused" ? "Resume the goal" : "Pause the goal";
  return (
    <div className="goal-pill">
      {seconds !== null && (
        <span className="goal-time-wrap">
          <span>
            <Clock seconds={seconds} lit={lit} />
          </span>
        </span>
      )}
      {phase === "done" && <IconCheck className="goal-check" width={15} height={15} />}
      {(phase === "running" || phase === "paused") && (
        <button
          className="send goal-toggle"
          type="button"
          aria-label={tip}
          data-tip={tip}
          data-tip-align="end"
          onClick={() => {
            if (pending) return;
            setPending(true);
            post({ type: "goal", action: phase === "paused" ? "resume" : "pause" });
          }}
        >
          {/* The stop square and the play triangle share the disc: one turns into the other. */}
          <svg viewBox="0 0 24 24" width={16} height={16} aria-hidden="true">
            <rect className="i-pause" x="7.5" y="7.5" width="9" height="9" rx="2" />
            <path className="i-play" d="M9.6 7.3v9.4l7.3-4.7z" />
          </svg>
        </button>
      )}
    </div>
  );
}

/** `4:07`, `1:04:07`: one slot per character, so the pill never jitters. A start or a play lights it up. */
function Clock({ seconds, lit }: { seconds: number; lit: number }) {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const pad = (n: number) => String(n).padStart(2, "0");
  const text = h ? `${h}:${pad(m)}:${pad(seconds % 60)}` : `${m}:${pad(seconds % 60)}`;
  return (
    <span className={lit > 0 ? "goal-time is-lit" : "goal-time"} key={lit}>
      {[...text].map((char, i) => (
        <span key={i} className={char === ":" ? "sep" : undefined} style={{ "--i": i } as CSSProperties}>
          {char}
        </span>
      ))}
    </span>
  );
}

/**
 * The goal's time, in seconds, from when it was set: to now while it runs,
 * to its last change once paused or over; `null` when the agent did not say.
 */
function useGoalSeconds(goal: Goal, running: boolean): number | null {
  const [now, setNow] = useState(Date.now);
  const { createdAt, updatedAt } = goal;
  useEffect(() => {
    if (!running || createdAt === undefined) return;
    let timer: ReturnType<typeof setTimeout>;
    // Each tick on the goal's own second.
    const tick = () => {
      setNow(Date.now());
      timer = setTimeout(tick, 1000 - ((Date.now() - createdAt) % 1000));
    };
    tick();
    return () => clearTimeout(timer);
  }, [running, createdAt]);
  if (createdAt === undefined) return null;
  const at = running ? now : (updatedAt ?? now);
  return Math.max(0, Math.floor((at - createdAt) / 1000));
}

/**
 * The light around the box while there is a goal, driven frame by frame: it
 * keeps circling while the goal runs, runs down on a pause, and a start, a
 * play or the end kick it round faster before it settles. After a pause or
 * the end, hover lights it again only once the pointer has left the box (or
 * the user types). Without a goal, the stylesheet's hover light is back.
 * Returns what bumps on a start or a play.
 */
export function useGoalLight(
  shell: RefObject<HTMLElement | null>,
  glow: RefObject<HTMLElement | null>,
  phase: GoalPhase | null,
): number {
  const [lit, setLit] = useState(0);
  const light = useRef({ angle: 0, speed: 0, opacity: 0, rate: 4, hover: false, focused: false, hushed: false });
  const running = useRef(false);
  running.current = phase === "running";
  const wake = useRef(() => {});
  const before = useRef(phase);

  useEffect(() => {
    const was = before.current;
    before.current = phase;
    if (was === phase) return;
    const l = light.current;
    const kick = (speed: number, opacity: number, rate: number) => {
      l.speed = speed;
      l.opacity = Math.max(l.opacity, opacity);
      l.rate = rate;
    };
    if (phase === "running") {
      if (was === "paused") kick(560, 0.75, 2.4);
      else kick(320, 0.55, 2.2);
      setLit((n) => n + 1);
    } else if (phase === "done") {
      kick(620, 0.8, 1.8);
      l.hushed = true;
    } else if (phase !== null) {
      l.rate = 1.6;
      l.hushed = true;
    }
    wake.current();
  }, [phase]);

  const driven = phase !== null;
  useEffect(() => {
    const box = shell.current;
    const el = glow.current;
    if (!driven || !box || !el) return;
    const l = light.current;
    l.hover = box.matches(":hover");
    l.focused = box.matches(":focus-within");
    l.opacity = Math.max(l.opacity, Number(getComputedStyle(el).opacity) || 0);
    const still = reducedMotion();
    let frame = 0;
    let then = 0;
    const step = (now: number) => {
      const dt = then ? Math.min(0.05, (now - then) / 1000) : 0;
      then = now;
      const on = running.current || ((l.hover || l.focused) && !l.hushed);
      const k = 1 - Math.exp(-dt * l.rate);
      l.speed += ((on ? 40 : 0) - l.speed) * k;
      l.opacity += ((on ? 0.3 : 0) - l.opacity) * k;
      if (!still) l.angle = (l.angle + l.speed * dt) % 360;
      el.style.setProperty("--ya-glow-angle", `${l.angle}deg`);
      el.style.opacity = l.opacity.toFixed(3);
      // Out and still: it sleeps until something wakes it.
      if (!on && l.opacity < 0.002 && l.speed < 0.5) {
        l.opacity = l.speed = 0;
        el.style.opacity = "0";
        frame = 0;
        return;
      }
      frame = requestAnimationFrame(step);
    };
    wake.current = () => {
      if (frame) return;
      then = 0;
      frame = requestAnimationFrame(step);
    };
    const events: [string, () => void][] = [
      ["mouseenter", () => (l.hover = true)],
      ["mouseleave", () => ((l.hover = false), (l.hushed = false))],
      ["focusin", () => (l.focused = true)],
      // Focus moving within the box comes back with its focusin.
      ["focusout", () => (l.focused = false)],
      ["input", () => (l.hushed = false)],
    ];
    const handlers = events.map(([name, listener]) => {
      const handler = () => {
        listener();
        wake.current();
      };
      box.addEventListener(name, handler);
      return [name, handler] as const;
    });
    el.classList.add("is-driven");
    wake.current();
    return () => {
      cancelAnimationFrame(frame);
      wake.current = () => {};
      for (const [name, handler] of handlers) box.removeEventListener(name, handler);
      el.classList.remove("is-driven");
      el.style.removeProperty("--ya-glow-angle");
      el.style.removeProperty("opacity");
    };
  }, [driven, shell, glow]);

  return lit;
}

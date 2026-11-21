import { useEffect, useLayoutEffect, useRef } from "react";
import type { SessionConfig } from "../src/protocol";
import { reducedMotion } from "./motion";
import { Popover } from "./Popover";
import { post } from "./vscode";

type Effort = NonNullable<SessionConfig["effort"]>;

const EFFORT_LABELS: Record<string, string> = {
  off: "Off",
  minimal: "Minimal",
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra high",
  max: "Max",
};

const effortLabel = (level: string) => EFFORT_LABELS[level] ?? level;

/** The chip has less room than the slider: its long names are shortened. */
const chipLabel = (level: string) => (level === "xhigh" ? "XHigh" : effortLabel(level));

const SOFT = "cubic-bezier(0.22, 1, 0.36, 1)";

/** The reasoning effort as rising bars, one per level, lit up to `at`. */
function Gauge({ count, at }: { count: number; at: number }) {
  return (
    <span className="gauge" aria-hidden="true">
      {Array.from({ length: count }, (_, i) => (
        <span
          key={i}
          className={i <= at ? "gauge-bar is-lit" : "gauge-bar"}
          // From a quarter of the height for the lowest level to full for the highest.
          style={{ height: `${count > 1 ? 25 + (75 * i) / (count - 1) : 100}%` }}
        />
      ))}
    </span>
  );
}

/** Whether there is a level to choose: a model with a single one (`high` alone) has its effort fixed. */
export const pickable = (effort: Effort) => effort.levels.length > 1;

/** The reasoning effort's chip in the composer and, above it while open, its slider. */
export function EffortPicker({
  effort,
  open,
  onToggle,
  onClose,
}: {
  effort: Effort;
  open: boolean;
  onToggle: () => void;
  onClose: () => void;
}) {
  // A single level is nothing to pick: the chip names it and opens no slider.
  if (!pickable(effort)) {
    return (
      <div className="picker effort-picker">
        <span className="chip is-idle" data-tip="Reasoning effort: fixed for this model">
          <Gauge count={effort.levels.length} at={effort.levels.indexOf(effort.current)} />
          <span className="chip-label">{chipLabel(effort.current)}</span>
        </span>
      </div>
    );
  }
  return (
    <div className="picker effort-picker">
      {open && (
        <Popover className="menu effort-menu" trigger="effort" onClose={onClose}>
          {/* A model with other levels gets a new slider. */}
          <EffortSlider key={effort.levels.join(" ")} effort={effort} />
        </Popover>
      )}
      <button
        className={open ? "chip is-open" : "chip"}
        type="button"
        data-tip="Reasoning effort"
        aria-haspopup="dialog"
        aria-expanded={open}
        data-menu-trigger="effort"
        onClick={onToggle}
      >
        <Gauge count={effort.levels.length} at={effort.levels.indexOf(effort.current)} />
        <span className="chip-label">{chipLabel(effort.current)}</span>
      </button>
    </div>
  );
}

/**
 * A pill track with a notch per level; the cyan fill is the handle, a white
 * ball on its round end. Dragged, a comet rides the ball; let go, the fill
 * springs onto the nearest notch and the level goes to the agent. The
 * level's name above streams in with its own light. The moving parts are
 * driven by `Slider`, outside React.
 */
function EffortSlider({ effort }: { effort: Effort }) {
  const root = useRef<HTMLDivElement>(null);
  const slider = useRef<Slider | null>(null);

  useLayoutEffect(() => {
    const element = root.current!;
    place(element.closest<HTMLElement>(".menu")!);
    const created = new Slider(element, effort.levels, Math.max(0, effort.levels.indexOf(effort.current)), (level) =>
      post({ type: "setConfig", configId: "thought_level", value: level }),
    );
    slider.current = created;
    created.track.focus({ preventScroll: true });
    return () => created.destroy();
    // Once: the slider keeps its own level from here; the effect below follows the agent's.
  }, []);

  // The agent's word on the level: a refused change puts the old one back.
  useEffect(() => slider.current?.goTo(effort.levels.indexOf(effort.current)), [effort]);

  const notches = (
    <div className="effort-notches">
      {effort.levels.map((level) => (
        <span key={level} className="effort-notch" />
      ))}
    </div>
  );
  return (
    <div ref={root}>
      <div className="effort-head">
        <span className="effort-title">Reasoning effort</span>
        <span className="effort-value" />
      </div>
      <div className="effort-slide">
        <span className="effort-halo" />
        <div
          className="effort-track"
          role="slider"
          tabIndex={0}
          aria-label="Reasoning effort"
          aria-valuemin={0}
          aria-valuemax={effort.levels.length - 1}
        >
          {notches}
          <div className="effort-fill">
            {notches}
            <span className="effort-surge" />
          </div>
          <span className="effort-tail" />
          <span className="effort-glow" />
          <span className="effort-thumb" />
        </div>
      </div>
    </div>
  );
}

/** The menu is wider than the chip: it moves left, and narrows if it must, to stay inside the composer. */
function place(menu: HTMLElement) {
  const composer = menu.closest(".composer")?.getBoundingClientRect();
  if (!composer) return;
  const picker = menu.parentElement!.getBoundingClientRect();
  const room = composer.width - 16;
  if (menu.offsetWidth > room) menu.style.width = `${room}px`;
  const left = -Math.max(0, picker.left + menu.offsetWidth - (composer.right - 8));
  menu.style.left = `${left}px`;
  menu.style.transformOrigin = `${picker.width / 2 - left}px 100%`;
}

/**
 * The slider's moving parts. The fill's end (`p`, the centre of its round
 * end) is a spring: held, it follows the pointer closely; let go, it springs
 * onto its notch with a small overshoot. While held, a comet rides it: the
 * ball turns into a light's core, a halo spills past the track, a tail
 * stretches behind with the speed, and the notches it crosses spark.
 */
class Slider {
  readonly track: HTMLElement;
  private readonly fill: HTMLElement;
  private readonly thumb: HTMLElement;
  private readonly halo: HTMLElement;
  private readonly glow: HTMLElement;
  private readonly tail: HTMLElement;
  private readonly surge: HTMLElement;
  private readonly valueBox: HTMLElement;
  private readonly notchRows: HTMLElement[][];
  private readonly W: number;
  private readonly H: number;
  /** Where each level's notch is: the centre of the fill's round end on it. */
  private readonly centers: number[];
  /** The level sent last. */
  private value: number;
  private p: number;
  private v = 0;
  private target: number;
  private dragging = false;
  /** The level the fill is nearest, as the name shows it. */
  private near = -1;
  /** Where the fill's end was drawn last, for the notches it crosses. */
  private drawn: number;
  /** The comet's light (0 to 1), its tail's length and which way it trails. */
  private light = 0;
  private tailLength = 0;
  private heading = 1;
  /** When each notch last sparked. */
  private readonly sparked: number[] = [];
  private raf = 0;
  private last = 0;

  constructor(
    root: HTMLElement,
    private readonly levels: string[],
    value: number,
    private readonly onCommit: (level: string) => void,
  ) {
    const find = (selector: string) => root.querySelector<HTMLElement>(selector)!;
    this.track = find(".effort-track");
    this.fill = find(".effort-fill");
    this.thumb = find(".effort-thumb");
    this.halo = find(".effort-halo");
    this.glow = find(".effort-glow");
    this.tail = find(".effort-tail");
    this.surge = find(".effort-surge");
    this.valueBox = find(".effort-value");
    this.notchRows = [...root.querySelectorAll(".effort-notches")].map((row) => [...row.children] as HTMLElement[]);

    this.W = this.track.offsetWidth;
    this.H = this.track.offsetHeight;
    this.track.style.setProperty("--track-width", `${this.W}px`);
    const span = Math.max(1, levels.length - 1);
    this.centers = levels.map((_, i) => this.H / 2 + ((this.W - this.H) * i) / span);
    this.notchRows.forEach((row) => row.forEach((notch, i) => (notch.style.left = `${this.centers[i]}px`)));
    this.value = value;
    this.p = this.target = this.drawn = this.centers[value];
    this.render();
    this.listen();
  }

  destroy() {
    cancelAnimationFrame(this.raf);
    this.raf = 0;
  }

  /** Moves to a level the agent settled on, without sending it back. */
  goTo(i: number) {
    if (i < 0 || i === this.value || this.dragging) return;
    this.value = i;
    this.target = this.centers[i];
    this.kick();
  }

  private listen() {
    const top = this.levels.length - 1;
    const toP = (event: PointerEvent) =>
      clamp(event.clientX - this.track.getBoundingClientRect().left, this.H / 2, this.W - this.H / 2);
    this.track.addEventListener("pointerdown", (event) => {
      if (event.button !== 0) return;
      this.dragging = true;
      this.track.setPointerCapture(event.pointerId);
      this.track.classList.add("is-dragging");
      this.hover(-1);
      this.target = toP(event);
      this.kick();
    });
    this.track.addEventListener("pointermove", (event) => {
      if (!this.dragging) return this.hover(this.nearest(toP(event)));
      this.target = toP(event);
      this.kick();
    });
    this.track.addEventListener("pointerleave", () => !this.dragging && this.hover(-1));
    const end = () => {
      if (!this.dragging) return;
      this.dragging = false;
      this.track.classList.remove("is-dragging");
      this.settle(this.nearest(this.target));
    };
    this.track.addEventListener("pointerup", end);
    this.track.addEventListener("pointercancel", end);
    this.track.addEventListener("keydown", (event) => {
      const step = ({ ArrowRight: 1, ArrowUp: 1, ArrowLeft: -1, ArrowDown: -1 } as Record<string, number>)[event.key];
      const i = event.key === "Home" ? 0 : event.key === "End" ? top : step === undefined ? null : clamp(this.value + step, 0, top);
      if (i === null) return;
      event.preventDefault();
      this.settle(i);
    });
  }

  private nearest(p: number) {
    const top = this.levels.length - 1;
    return clamp(Math.round(((p - this.H / 2) / (this.W - this.H)) * top), 0, top);
  }

  /** Springs onto a level's notch and sends the level, when it changed. */
  private settle(i: number) {
    this.target = this.centers[i];
    this.kick();
    if (i === this.value) return;
    this.value = i;
    this.onCommit(this.levels[i]);
  }

  private hover(i: number) {
    this.notchRows.forEach((row) => row.forEach((notch, j) => notch.classList.toggle("is-hover", j === i)));
  }

  private kick() {
    if (this.raf) return;
    this.last = performance.now();
    this.raf = requestAnimationFrame(this.tick);
  }

  private tick = (now: number) => {
    const dt = Math.min(0.032, (now - this.last) / 1000);
    this.last = now;
    if (reducedMotion()) {
      this.p = this.target;
      this.v = this.light = this.tailLength = 0;
    } else {
      const k = this.dragging ? 1400 : 420;
      const c = this.dragging ? 75 : 25;
      for (let s = 0; s < 2; s++) {
        const h = dt / 2;
        this.v += (k * (this.target - this.p) - c * this.v) * h;
        this.p += this.v * h;
      }
      // The light comes up fast when held, brighter the faster it goes, and
      // dies down slower once let go; the tail follows the speed, smoothed.
      const speed = Math.min(1, Math.abs(this.v) / 1200);
      const goal = this.dragging ? 0.45 + 0.55 * speed : 0;
      this.light += (goal - this.light) * (1 - Math.exp(-dt * (goal > this.light ? 12 : 4)));
      this.tailLength += (Math.min(80, Math.abs(this.v) * 0.08) - this.tailLength) * (1 - Math.exp(-dt * 12));
      if (Math.abs(this.v) > 20) this.heading = Math.sign(this.v);
    }
    const resting = Math.abs(this.v) < 1 && Math.abs(this.target - this.p) < 0.2;
    if (!this.dragging && resting && this.light < 0.01 && this.tailLength < 0.5) {
      this.p = this.target;
      this.v = this.light = this.tailLength = 0;
      this.raf = 0;
    } else {
      this.raf = requestAnimationFrame(this.tick);
    }
    this.render();
  };

  private render() {
    // Never thinner than its own round end; past the last notch the track clips it.
    const p = clamp(this.p, this.H / 2, this.W - this.H / 2 + 6);
    this.fill.style.width = `${p + this.H / 2}px`;
    this.thumb.style.translate = `${p}px 0`;

    this.halo.style.transform = this.glow.style.transform = `translateX(${p}px)`;
    this.halo.style.opacity = (this.light * 0.55).toFixed(3);
    this.glow.style.opacity = (this.light * 0.9).toFixed(3);
    // Anchored on the ball, the tail reaches back the way it came.
    this.tail.style.transform = `translateX(${p - 100}px) scaleX(${(this.heading * this.tailLength) / 100})`;
    this.tail.style.opacity = (this.light * Math.min(1, this.tailLength / 12)).toFixed(3);
    if (this.light > 0.15) {
      this.centers.forEach((center, j) => {
        if ((this.drawn - center) * (p - center) <= 0 && this.drawn !== p) this.spark(j);
      });
    }
    this.drawn = p;

    const near = this.nearest(p);
    if (near === this.near) return;
    const first = this.near < 0;
    this.near = near;
    this.showName(near, !first);
    this.track.setAttribute("aria-valuenow", String(near));
    this.track.setAttribute("aria-valuetext", effortLabel(this.levels[near]));
  }

  /** A notch the comet crosses flashes white and settles back; once per crossing, not on the spring's way back. */
  private spark(j: number) {
    const now = performance.now();
    if (now - (this.sparked[j] ?? 0) < 300) return;
    this.sparked[j] = now;
    this.notchRows.forEach((row) =>
      row[j].animate([{ background: "#ffffff", scale: "2", boxShadow: "0 0 8px 1px rgb(255 255 255 / 0.8)" }, {}], {
        duration: 650,
        easing: SOFT,
      }),
    );
  }

  /** Reaching the top level: a light runs along the fill, then the ball kicks and the halo flashes. */
  private charge() {
    if (reducedMotion()) return;
    this.surge.animate([{ transform: "translateX(-70px)", opacity: 1 }, { transform: `translateX(${this.W}px)`, opacity: 1 }], {
      duration: 300,
      easing: "cubic-bezier(0.5, 0, 0.75, 0)",
    });
    this.thumb.animate([{ scale: "1" }, { scale: "1.5", offset: 0.35 }, { scale: "1" }], { duration: 560, delay: 230, easing: SOFT });
    this.halo.animate([{ opacity: 0.8 }, { opacity: 0 }], { duration: 700, delay: 230, easing: SOFT });
  }

  /**
   * The level's name: the old one fades out, the new one streams in letter
   * by letter (styles.css). The lowest rests dim, the top level in cyan
   * after a charge along the fill.
   */
  private showName(i: number, animate: boolean) {
    const top = this.levels.length - 1;
    const old = this.valueBox.querySelector(":scope > span:not(.is-out)");
    const name = document.createElement("span");
    name.className = i === top ? "is-top" : i === 0 ? "is-bottom" : "is-mid";
    for (const [k, char] of [...effortLabel(this.levels[i])].entries()) {
      const letter = document.createElement("span");
      letter.className = "effort-letter";
      letter.style.setProperty("--i", String(k));
      letter.textContent = char === " " ? " " : char;
      name.append(letter);
    }
    // Up the levels, the light lingers longer in the letters.
    name.style.setProperty("--duration", `${Math.round(420 + (300 * i) / Math.max(1, top))}ms`);
    if (animate) name.classList.add("is-arriving");
    if (animate && i === top) this.charge();
    this.valueBox.append(name);
    if (!old) return;
    if (!animate) return old.remove();
    old.classList.add("is-out");
    old.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 120, easing: SOFT, fill: "forwards" }).finished.then(
      () => old.remove(),
      () => old.remove(),
    );
  }
}

function clamp(value: number, low: number, high: number) {
  return Math.min(high, Math.max(low, value));
}

// The press: a pressed control gives a little and comes straight back, and a
// thin cyan ring opens from under the pointer, a few sparks shooting out of
// it. Sharp, nothing blurred; it stays inside the panel, smaller near its
// edges. A control that is only words (the reasoning's "Thought for 6s")
// opts out with `data-no-press`.
import { reducedMotion } from "./motion";

const OUT = "cubic-bezier(0.2, 0.8, 0.2, 1)";
const SPARKS = 6;
/** The ring at its widest. */
const RING = 28;
/** How far a spark shoots, past the ring's start. */
const REACH = 18;
/** A spark's length. */
const SPARK = 5;

const pressing = new WeakMap<Element, Animation>();

document.addEventListener("pointerdown", (event) => {
  const control = (event.target as Element | null)?.closest<HTMLElement>("button:not(:disabled):not([data-no-press])");
  if (!control || event.button !== 0) return;
  // Wide rows barely give; small pills and discs give more.
  const depth = control.offsetWidth > 120 ? 0.99 : 0.96;
  pressing.get(control)?.cancel();
  pressing.set(
    control,
    control.animate([{ scale: "1" }, { scale: String(depth) }], { duration: 60, easing: "ease-out", fill: "forwards" }),
  );
  const release = () => {
    control.removeEventListener("pointerup", release);
    control.removeEventListener("pointerleave", release);
    pressing.get(control)?.cancel();
    pressing.set(
      control,
      control.animate([{ scale: String(depth) }, { scale: "1" }], {
        duration: reducedMotion() ? 1 : 120,
        easing: OUT,
      }),
    );
  };
  control.addEventListener("pointerup", release);
  control.addEventListener("pointerleave", release);
  if (!reducedMotion()) spark(event.clientX, event.clientY);
});

function spark(x: number, y: number) {
  // The room to the panel's nearest edge: nothing is cut by it.
  const room = Math.min(x, y, window.innerWidth - x, window.innerHeight - y) - 1;
  const size = Math.min(RING, 2 * room);
  if (size < 8) return;

  const ring = document.createElement("span");
  ring.className = "puff";
  ring.style.left = `${x}px`;
  ring.style.top = `${y}px`;
  ring.style.width = ring.style.height = `${size}px`;
  ring.style.margin = `${-size / 2}px 0 0 ${-size / 2}px`;
  document.body.append(ring);
  ring
    .animate([{ transform: "scale(0.15)", opacity: 1 }, { transform: "scale(1)", opacity: 0 }], {
      duration: 220,
      easing: OUT,
    })
    .finished.then(() => ring.remove(), () => ring.remove());

  const style = getComputedStyle(document.body);
  const accent = style.getPropertyValue("--uh-accent-rgb").trim();
  const ink = style.getPropertyValue("--uh-ink-rgb").trim();
  const start = 4;
  const reach = Math.min(REACH, room - start - SPARK);
  if (reach < 4) return;
  const turn = Math.random() * 360;
  for (let i = 0; i < SPARKS; i++) {
    // Evenly round, a little off each.
    const angle = turn + (i * 360) / SPARKS + (Math.random() - 0.5) * 24;
    const grain = document.createElement("span");
    grain.className = "grain";
    grain.style.left = `${x - 0.75}px`;
    grain.style.top = `${y - SPARK / 2}px`;
    grain.style.height = `${SPARK}px`;
    grain.style.background = `rgb(${i % 3 === 0 ? ink : accent} / 0.95)`;
    document.body.append(grain);
    grain
      .animate(
        [
          { transform: `rotate(${angle}deg) translateY(${-start}px)`, opacity: 1 },
          { transform: `rotate(${angle}deg) translateY(${-start - reach}px) scaleY(0.4)`, opacity: 0 },
        ],
        { duration: 260, easing: OUT },
      )
      .finished.then(() => grain.remove(), () => grain.remove());
  }
}

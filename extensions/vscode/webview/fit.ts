import { useLayoutEffect, useRef, type RefObject } from "react";

/** Cut shorter than this, a label says too little: the row counts as short of room. */
const LABEL_MIN_PX = 40;

/**
 * Keeps a row of controls on one line as the panel narrows: what `steps`
 * name leaves the row one after another, in that order, until the rest
 * fits. `data-short` on the row lists what left; the stylesheet hides each.
 * Measured, not set at fixed widths: what the row holds changes (the model's
 * name, the effort's level, a goal's time, stop beside send). It fits again
 * after each render, and when the row or one of its controls changes size.
 */
export function useFit(row: RefObject<HTMLElement | null>, steps: readonly string[]) {
  const fit = useRef(() => {});
  useLayoutEffect(() => {
    const el = row.current;
    if (!el) return;
    /** The row's width and its controls', as the last fit left them: unchanged, there is nothing to fit. */
    let fitted = "";
    let frame = 0;
    const widths = () => [el.clientWidth, ...[...el.children].map((child) => (child as HTMLElement).offsetWidth)].join();
    // A control that changed size on its own (a font landing, the goal's time growing in) is fitted on the
    // next frame: fitting here would resize its neighbours under the observer.
    const sizes = new ResizeObserver((entries) => {
      if (entries.some((entry) => entry.target === el)) fit.current();
      else frame ||= requestAnimationFrame(() => fit.current());
    });
    fit.current = () => {
      frame = 0;
      if (widths() === fitted) return;
      for (let gone = 0; gone <= steps.length; gone++) {
        el.dataset.short = steps.slice(0, gone).join(" ");
        if (fits(el)) break;
      }
      fitted = widths();
      for (const child of el.children) sizes.observe(child);
    };
    sizes.observe(el);
    return () => {
      sizes.disconnect();
      cancelAnimationFrame(frame);
      fit.current = () => {};
    };
  }, [row, steps]);
  // What the row holds may have changed.
  useLayoutEffect(() => fit.current());
}

/** The row's last control ends inside it, and its label (a chip's) is whole or cut no shorter than the least. */
function fits(row: HTMLElement): boolean {
  const end = row.getBoundingClientRect().right - parseFloat(getComputedStyle(row).paddingRight);
  if ((row.lastElementChild?.getBoundingClientRect().right ?? 0) > end + 0.5) return false;
  const label = row.querySelector<HTMLElement>(":scope > .chip > .chip-label");
  return !label || label.clientWidth >= Math.min(label.scrollWidth, LABEL_MIN_PX);
}

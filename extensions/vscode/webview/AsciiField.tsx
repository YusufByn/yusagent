import { useEffect, useRef } from "react";
import { reducedMotion } from "./motion";

/** Brightness ramp, blank first. ASCII only, so every glyph has the mono advance. */
const RAMP = " .:-=+*#%";
const FONT_PX = 11;
const LINE_PX = 14;
/** Seconds for every cell to have appeared. */
const INTRO_S = 0.9;
/** How far the whole field drifts with the pointer, in px. */
const PARALLAX_PX = 36;
/** Radius of the swell under the pointer, in px. */
const SWELL_PX = 80;
/** Swell strength above which a cell turns cyan. */
const HOT_SWELL = 0.3;

/**
 * A full-bleed canvas of characters: slow interference waves, drifting with
 * the pointer and swelling under it, quieter in the band behind the wordmark.
 * The field is faint ink; only the swell and the intro flash are cyan.
 */
export function AsciiField() {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;

    const still = reducedMotion();
    const style = getComputedStyle(canvas);
    const ink = style.getPropertyValue("--uh-ink-rgb").trim();
    const accent = style.getPropertyValue("--uh-accent-rgb").trim();
    const font = `${FONT_PX}px ${style.getPropertyValue("--uh-mono")}`;
    const levels = RAMP.length;
    const ramp = (rgb: string, floor: number, span: number) =>
      Array.from({ length: levels }, (_, level) => {
        const alpha = floor + Math.pow(level / (levels - 1), 1.4) * span;
        return `rgb(${rgb} / ${alpha.toFixed(3)})`;
      });
    // grid values: level for ink cells, levels + level for cyan cells.
    const fills = [...ramp(ink, 0.04, 0.3), ...ramp(accent, 0.2, 0.7)];

    let width = 0;
    let height = 0;
    let cols = 0;
    let rows = 0;
    let cellW = 0;
    let seeds = new Float32Array(0);
    let grid = new Uint8Array(0);

    const pointer = { x: 0.5, y: 0.5, tx: 0.5, ty: 0.5, on: 0, ton: 0 };
    const onMove = (event: PointerEvent) => {
      pointer.tx = event.clientX / width;
      pointer.ty = event.clientY / height;
      pointer.ton = 1;
    };
    const onLeave = () => {
      pointer.tx = 0.5;
      pointer.ty = 0.5;
      pointer.ton = 0;
    };

    const resize = () => {
      const dpr = window.devicePixelRatio || 1;
      width = canvas.clientWidth;
      height = canvas.clientHeight;
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(height * dpr);
      // Resizing the canvas resets the context state.
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.font = font;
      ctx.textBaseline = "top";
      cellW = ctx.measureText("M").width;
      cols = Math.ceil(width / cellW) + 1;
      rows = Math.ceil(height / LINE_PX) + 1;
      seeds = Float32Array.from({ length: cols * rows }, () => Math.random());
      grid = new Uint8Array(cols * rows);
    };

    const start = performance.now();
    let frame = 0;

    const draw = (now: number) => {
      const t = still ? 0 : (now - start) / 1000;
      const intro = still ? 1 : Math.min(1, t / INTRO_S);
      pointer.x += (pointer.tx - pointer.x) * 0.08;
      pointer.y += (pointer.ty - pointer.y) * 0.08;
      pointer.on += (pointer.ton - pointer.on) * 0.05;

      const mx = pointer.x * width;
      const my = pointer.y * height;
      const ox = (pointer.x - 0.5) * PARALLAX_PX;
      const oy = (pointer.y - 0.5) * PARALLAX_PX;
      const cx = width / 2;
      const cy = height / 2;
      // The band behind the wordmark, about one wordmark tall.
      const bandH = Math.min(width * 0.14, 72);

      for (let r = 0; r < rows; r++) {
        const y = r * LINE_PX;
        const Y = y + oy;
        const by = (y + LINE_PX / 2 - cy) / bandH;
        const quiet = 1 - 0.8 * Math.exp(-by * by);
        for (let c = 0; c < cols; c++) {
          const i = r * cols + c;
          const x = c * cellW;
          const X = x + ox;
          let v =
            0.42 +
            0.22 * Math.sin(X * 0.021 + t * 0.6) * Math.cos(Y * 0.017 - t * 0.45) +
            0.18 * Math.sin((X * 0.7 - Y) * 0.011 + t * 0.32) +
            0.1 * Math.sin(Math.hypot(X - cx, Y - cy) * 0.03 - t * 0.9);
          const dx = x - mx;
          const dy = y - my;
          const d2 = dx * dx + dy * dy;
          const swell = Math.exp(-d2 / (2 * SWELL_PX * SWELL_PX)) * pointer.on;
          v += swell * (0.4 + 0.18 * Math.sin(Math.sqrt(d2) * 0.08 - t * 4));
          v *= quiet;

          let level = Math.max(0, Math.min(levels - 1, Math.floor(v * levels)));
          let hot = swell > HOT_SWELL;
          if (intro < 1) {
            // Cells appear in a random order and flash cyan first.
            const at = seeds[i] * 0.85;
            if (intro < at) level = 0;
            else if (intro - at < 0.06) {
              level = levels - 1;
              hot = true;
            }
          }
          grid[i] = level && hot ? levels + level : level;
        }
      }

      ctx.clearRect(0, 0, width, height);
      const row: string[] = new Array(cols);
      for (let value = 1; value < fills.length; value++) {
        if (value === levels) continue; // a blank cyan cell
        ctx.fillStyle = fills[value];
        const glyph = RAMP[value % levels];
        for (let r = 0; r < rows; r++) {
          let any = false;
          for (let c = 0; c < cols; c++) {
            const hit = grid[r * cols + c] === value;
            row[c] = hit ? glyph : " ";
            any ||= hit;
          }
          if (any) ctx.fillText(row.join(""), 0, r * LINE_PX);
        }
      }

      if (!still) frame = requestAnimationFrame(draw);
    };

    resize();
    const observer = new ResizeObserver(() => {
      resize();
      if (still) draw(start);
    });
    observer.observe(canvas);
    if (!still) {
      window.addEventListener("pointermove", onMove);
      document.documentElement.addEventListener("pointerleave", onLeave);
    }
    frame = requestAnimationFrame(draw);

    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
      window.removeEventListener("pointermove", onMove);
      document.documentElement.removeEventListener("pointerleave", onLeave);
    };
  }, []);

  return <canvas ref={canvasRef} className="field" aria-hidden="true" />;
}

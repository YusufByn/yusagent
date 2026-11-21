// The front that brings a streaming text's words in (Markdown.tsx, styles.css .md-token).

/** How far apart the text's updates come, until the stream says otherwise; the most a pause counts for. */
const WAVE_GAP_MS = 60;
const WAVE_GAP_MAX_MS = 250;
/** How long after a word arrives the front has crossed it, at the most; the least time it takes over an update's words. */
const WAVE_MAX_LAG_MS = 150;
const WAVE_MIN_SPAN_MS = 30;

/**
 * Brings the streaming words in as one front moving through the text at the
 * pace it arrives: the words of each update are spread, in order and by
 * length, over the time until the next update is expected (how far apart
 * the last ones came). Behind the front the words fade in from the
 * background (styles.css, .md-token). A word is on the page when it arrives; the
 * front has crossed it at most WAVE_MAX_LAG_MS later, catching up when the
 * stream speeds up. The effect follows the stream, never the reverse.
 */
export class Wave {
  private queue: HTMLElement[] = [];
  /** When the words scheduled so far are all in. */
  private front = 0;
  /** When the last update came, and how far apart they come (smoothed). */
  private last = 0;
  private gap = WAVE_GAP_MS;

  add(word: HTMLElement): void {
    this.queue.push(word);
  }

  flush(): void {
    if (this.queue.length === 0) return;
    const now = performance.now();
    if (this.last) this.gap = this.gap * 0.6 + Math.min(Math.max(now - this.last, 16), WAVE_GAP_MAX_MS) * 0.4;
    this.last = now;
    // A word and the space after it.
    const lengths = this.queue.map((word) => (word.textContent?.length ?? 1) + 1);
    const total = lengths.reduce((sum, length) => sum + length, 0);
    const from = Math.max(now, this.front);
    const end = Math.min(Math.max(now + this.gap, from + WAVE_MIN_SPAN_MS), now + WAVE_MAX_LAG_MS);
    // Behind: this update's words go faster, from sooner.
    const start = Math.min(from, end - WAVE_MIN_SPAN_MS);
    let at = start;
    this.queue.forEach((word, index) => {
      const span = ((end - start) * lengths[index]) / total;
      word.style.setProperty("--wave-at", `${Math.round(at - now)}ms`);
      // A number (ms): the styles compute with it.
      word.style.setProperty("--wave-span", `${Math.max(1, Math.round(span))}`);
      at += span;
    });
    this.front = end;
    this.queue = [];
  }
}

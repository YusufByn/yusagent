/** The agent logo cropped to its strokes, so it can stand in for a letter. */
export function LogoGlyph({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="25 16 50 68"
      fill="none"
      stroke="currentColor"
      strokeWidth={10}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M30 21 V59 A20 20 0 0 0 70 59 V53" pathLength={1} />
      <path d="M70 21 V29" pathLength={1} />
    </svg>
  );
}

/**
 * "Unharnessed" with the logo as its U, set at weight 460, tracking −0.025 em.
 * The glyph is sized to the font's U (0.6585 em tall, baseline −0.008 em), so
 * its stroke (10 in a 68-unit glyph) matches the font's stem at that weight.
 */
export function Wordmark({ animated = false }: { animated?: boolean }) {
  return (
    <span className={animated ? "wordmark is-animated" : "wordmark"} role="img" aria-label="Unharnessed">
      <LogoGlyph className="wordmark-u" />
      <span className="wordmark-rest">nharnessed</span>
    </span>
  );
}

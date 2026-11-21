// yusAgent (YA-10, ticket 24, PR I) : remplace le paquet `thinking-orbs` (animation) qu'importe
// Chat.tsx d'Unharnessed (157dcdc1d) ; le visuel de yusAgent viendra plus tard.

export function ThinkingOrb({ state }: { state: string; size?: number; theme?: string; color?: string }) {
  return <span className="orb" data-state={state} aria-hidden="true" />;
}

// yusAgent (YA-10, ticket 24, PR I) : bouchon visuel. Le fichier d'Unharnessed du même nom
// (157dcdc1d, vscode/webview/) dessine ce composant ; le visuel de yusAgent viendra plus tard.
// Mêmes exports, rien de dessiné.

export function LogoGlyph(_: { className?: string }) {
  return null;
}

export function Wordmark(_: { animated?: boolean }) {
  return <span className="wordmark">yusAgent</span>;
}

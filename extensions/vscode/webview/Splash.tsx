// yusAgent (YA-10, ticket 24, PR I) : bouchon visuel. Le fichier d'Unharnessed du même nom
// (157dcdc1d, vscode/webview/) dessine ce composant ; le visuel de yusAgent viendra plus tard.
// Mêmes exports, rien de dessiné.
// Pas d'écran d'accueil : le panneau passe tout de suite à l'écran principal.

import { useEffect } from "react";
import type { AgentState } from "../src/protocol";

export function Splash({ onDone }: { agent: AgentState; onDone: () => void }) {
  useEffect(() => onDone(), [onDone]);
  return null;
}

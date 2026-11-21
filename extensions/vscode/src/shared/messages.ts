// Messages entre la webview et l'hôte de l'extension.

import type { ChatState } from "./state";

export type ToHost =
  | { type: "ready" }
  | { type: "send"; text: string }
  | { type: "stop" }
  | { type: "setConfig"; configId: string; value: string }
  | { type: "openSetting" }
  | { type: "retry" }
  | { type: "probeResult"; probe: Probe };

/** Ce qu'affiche la webview : texte visible et sélecteurs (tests e2e). */
export interface Probe {
  text: string;
  selects: { label: string | null; value: string; disabled: boolean; options: string[] }[];
}

/** `probe` : la webview renvoie son texte affiché (vérification du rendu par les tests e2e). */
export type ToWebview = { type: "state"; state: ChatState } | { type: "probe" };

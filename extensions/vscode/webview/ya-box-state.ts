// yusAgent (YA-10, ticket 31, X3) : l'état de la boîte de permission ouverte, pour la sonde des
// tests e2e (`webview/ya-probe.ts`). La boîte (`webview/ya-permission.tsx`) le pose à son ouverture
// et le retire à sa fermeture ; rien d'autre ne le lit. Les actions demandées par la sonde « à la
// prochaine ouverture » (`ProbeInput.atBoxOpen`) passent par `onBoxOpen` : sans sonde, aucune.

import type { QuietKeys } from "../src/ya/permission-keys";

export type BoxKey = { key: string; armed: boolean; result: string };

export type BoxState = {
  keys: QuietKeys;
  openedAt: number;
  /** Chaque touche reçue par la boîte : si les touches comptaient à cet instant, et l'issue. */
  log: BoxKey[];
};

let current: BoxState | null = null;
const openers = new Set<(state: BoxState) => void>();

export function boxOpened(state: BoxState): void {
  current = state;
  for (const opener of [...openers]) opener(state);
}

export function boxClosed(state: BoxState): void {
  if (current === state) current = null;
}

export function currentBox(): BoxState | null {
  return current;
}

/** Une fois, à la prochaine ouverture d'une boîte. */
export function onNextBoxOpen(run: (state: BoxState) => void): void {
  const once = (state: BoxState) => {
    openers.delete(once);
    run(state);
  };
  openers.add(once);
}

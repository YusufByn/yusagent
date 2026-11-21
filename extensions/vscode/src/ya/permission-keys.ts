// yusAgent (YA-10, ticket 24, PR 3, C3) : les touches de la boîte de permission, logique pure (sans
// DOM ni VS Code, testée par `node --test`, utilisée par `webview/ya-permission.tsx`).
//
// Mêmes touches que le TUI (`crates/pa-tui/src/ya_permission_panel.rs:7-13`) : `y` autorise une
// fois, `n` ou `Esc` refuse une fois. **Fenêtre de calme** de 1 s (`QUIET`, comme le TUI, `:29`) :
// une touche ne compte qu'après 1 s sans frappe depuis l'ouverture de la boîte (ou l'arrivée de la
// demande suivante) ; chaque frappe pendant ce temps, ou toute autre touche ensuite, la relance.
// **Jamais Entrée.** Le composant n'appelle `key` que si la boîte a le focus (décision de Yusuf).

export const QUIET_MS = 1000;

export type KeyResult = "allow" | "deny" | "ignored";

/** Une touche, telle que `KeyboardEvent` la donne. */
export type Key = { key: string; ctrlKey?: boolean; metaKey?: boolean; altKey?: boolean };

export class QuietKeys {
  private since: number;

  constructor(now: number) {
    this.since = now;
  }

  /** La boîte s'ouvre sur une nouvelle demande : la fenêtre repart. */
  reset(now: number): void {
    this.since = now;
  }

  /** Si les touches de réponse comptent à `now`. */
  armed(now: number): boolean {
    return now - this.since >= QUIET_MS;
  }

  key({ key, ctrlKey, metaKey, altKey }: Key, now: number): KeyResult {
    const armed = this.armed(now);
    const plain = !ctrlKey && !metaKey && !altKey;
    if (armed && plain && key === "y") return "allow";
    if (armed && plain && (key === "n" || key === "Escape")) return "deny";
    // Entrée, une frappe trop tôt, une autre touche : rien, et la fenêtre repart.
    this.since = now;
    return "ignored";
  }
}

/**
 * Le code tel qu'il tournera : les caractères de contrôle (sauf retour à la ligne et tabulation) et
 * les caractères de mise en forme invisibles (largeur nulle, sens d'écriture, trait d'union
 * conditionnel, BOM) écrits `\u{…}`, comme le TUI (décision T2 du ticket 20).
 */
export function visibleCode(code: string): string {
  // eslint-disable-next-line no-control-regex
  return code.replace(/[\u0000-\u0008\u000b-\u001f\u007f-\u009f\u00ad\u061c\u180e\u200b-\u200f\u202a-\u202e\u2060-\u2064\u2066-\u206f\ufeff\ufff9-\ufffb]/g, (char) =>
    `\\u{${char.codePointAt(0)!.toString(16).padStart(4, "0")}}`,
  );
}

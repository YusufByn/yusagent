// yusAgent (YA-10, ticket 24, tests e2e) : sur `probe`, la webview renvoie son texte visible et ses
// sélecteurs à l'hôte (`src/view.ts`). Rien ne sort de VS Code.
// PR 3 : `probeInput` agit comme un utilisateur (texte tapé dans la zone de message, focus, touches,
// clic sur un bouton de la boîte de permission), puis renvoie la même sonde.
import type { Probe, ProbeInput } from "../src/protocol";
import { onHostMessage, post } from "./vscode";

const BOX = ".ya-permission";
const COMPOSER = "textarea.composer-input";

function probe(): Probe {
  const active = document.activeElement;
  return {
    text: document.body.innerText,
    selects: Array.from(document.querySelectorAll("select")).map((select) => ({
      label: select.getAttribute("aria-label"),
      value: select.value,
      disabled: select.disabled,
      options: Array.from(select.options).map((option) => option.text),
    })),
    input: document.querySelector(COMPOSER)?.getAttribute("aria-label") ?? null,
    focused: active?.matches(BOX) ? "box" : active?.matches(COMPOSER) ? "composer" : "other",
    boxButtons: Array.from(document.querySelectorAll<HTMLButtonElement>(`${BOX} .ya-permission-actions button`)).map((button) => ({
      name: button.textContent ?? "",
      disabled: button.disabled,
    })),
  };
}

function act({ type, focus, keys, click }: ProbeInput): void {
  if (type !== undefined) {
    // Comme une frappe dans un champ contrôlé par React : la valeur, puis l'événement `input`.
    const composer = document.querySelector<HTMLTextAreaElement>(COMPOSER);
    if (composer) {
      composer.focus();
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set?.call(composer, type);
      composer.dispatchEvent(new Event("input", { bubbles: true }));
    }
  }
  if (focus === "box") document.querySelector<HTMLElement>(BOX)?.focus();
  if (focus === "none") (document.activeElement as HTMLElement | null)?.blur();
  const target = (document.activeElement as HTMLElement | null) ?? document.body;
  for (const key of keys ?? []) target.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
  if (click !== undefined) {
    // `click()` sur un bouton désactivé ne fait rien, comme un vrai clic.
    const button = Array.from(document.querySelectorAll<HTMLButtonElement>(`${BOX} .ya-permission-actions button`)).find(
      (candidate) => candidate.textContent === click,
    );
    button?.click();
  }
}

onHostMessage((message) => {
  if (message.type === "probe") post({ type: "probeResult", probe: probe() });
  if (message.type === "probeInput") {
    act(message.input);
    // Laisse React traiter l'action avant de lire la page.
    setTimeout(() => post({ type: "probeResult", probe: probe() }), 50);
  }
});

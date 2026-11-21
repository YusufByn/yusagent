// yusAgent (YA-10, ticket 24, tests e2e) : sur `probe`, la webview renvoie son texte visible et ses
// sélecteurs à l'hôte (`src/view.ts`). Rien ne sort de VS Code.
import type { Probe } from "../src/protocol";
import { onHostMessage, post } from "./vscode";

function probe(): Probe {
  return {
    text: document.body.innerText,
    selects: Array.from(document.querySelectorAll("select")).map((select) => ({
      label: select.getAttribute("aria-label"),
      value: select.value,
      disabled: select.disabled,
      options: Array.from(select.options).map((option) => option.text),
    })),
    input: document.querySelector("textarea.composer-input")?.getAttribute("aria-label") ?? null,
  };
}

onHostMessage((message) => {
  if (message.type === "probe") post({ type: "probeResult", probe: probe() });
  // PR 3 : touches vers la boîte de permission, avec ou sans le focus, comme un clavier.
  if (message.type === "probeKeys") {
    const box = document.querySelector<HTMLElement>(".ya-permission");
    if (message.focus) box?.focus();
    else (document.activeElement as HTMLElement | null)?.blur();
    const target = (document.activeElement as HTMLElement | null) ?? document.body;
    for (const key of message.keys) target.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
    // Laisse React traiter les touches avant de lire la page.
    setTimeout(() => post({ type: "probeResult", probe: probe() }), 50);
  }
});

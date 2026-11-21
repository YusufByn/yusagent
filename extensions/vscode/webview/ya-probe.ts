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
  };
}

onHostMessage((message) => {
  if (message.type === "probe") post({ type: "probeResult", probe: probe() });
});

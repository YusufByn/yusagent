// yusAgent (YA-10, ticket 24, tests e2e) : sur `probe`, la webview renvoie son texte visible et ses
// sélecteurs à l'hôte (`src/view.ts`). Rien ne sort de VS Code.
// PR 3 : `probeInput` agit comme un utilisateur (texte tapé dans la zone de message, focus, touches,
// clic sur un bouton de la boîte de permission), puis renvoie la même sonde.
// Ticket 31 : l'état de la boîte (touches et boutons prêts, touches reçues) et des actions jouées à
// l'ouverture de la prochaine boîte, pour vérifier ses délais sans l'horloge du test.
import type { Probe, ProbeInput } from "../src/protocol";
import { buttonsReady, BUTTONS_DELAY_MS } from "../src/ya/permission-keys";
import { currentBox, onNextBoxOpen } from "./ya-box-state";
import { onHostMessage, post } from "./vscode";

const BOX = ".ya-permission";
const COMPOSER = "textarea.composer-input";

let clicked: Probe["clicked"];
/** Ticket 31 : le dernier clic de la sonde sur un bouton de la boîte, avec son état à cet instant. */
let boxClick: { name: string; disabled: boolean } | undefined;

/** Ticket 31 : la boîte ouverte, lue avec les fonctions de `src/ya/permission-keys.ts`. */
function boxProbe(): Probe["box"] {
  const box = currentBox();
  if (!box) return null;
  const now = Date.now();
  return {
    keysReady: box.keys.armed(now),
    buttonsReady: buttonsReady(box.openedAt, now),
    keysReadyAt: box.keys.readyAt(),
    buttonsReadyAt: box.openedAt + BUTTONS_DELAY_MS,
    keys: [...box.log],
    ...(boxClick && { click: boxClick }),
  };
}

/** Un clic sur un bouton de la boîte par son nom ; `click()` sur un bouton désactivé ne fait rien, comme un vrai clic. */
function clickBox(name: string): void {
  const button = Array.from(document.querySelectorAll<HTMLButtonElement>(`${BOX} .ya-permission-actions button`)).find(
    (candidate) => candidate.textContent === name,
  );
  if (!button) return;
  boxClick = { name, disabled: button.disabled };
  button.click();
}

/** Des touches envoyées à l'élément qui a le focus. */
function press(keys: string[]): void {
  const target = (document.activeElement as HTMLElement | null) ?? document.body;
  for (const key of keys) target.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
}

function probe(): Probe {
  const active = document.activeElement;
  return {
    clicked,
    visibleInert: Array.from(document.querySelectorAll<HTMLElement>("[inert]")).filter((element) => {
      const rect = element.getBoundingClientRect();
      return rect.width > 0 && rect.height > 0;
    }).length,
    text: document.body.innerText,
    toggles: Array.from(document.querySelectorAll<HTMLElement>("button[aria-expanded]")).map((button) => {
      const rect = button.getBoundingClientRect();
      return {
        text: button.textContent?.trim() ?? "",
        expanded: button.getAttribute("aria-expanded") === "true",
        inert: button.closest("[inert]") !== null,
        visible: rect.width > 0 && rect.height > 0,
      };
    }),
    selects: Array.from(document.querySelectorAll("select")).map((select) => ({
      label: select.getAttribute("aria-label"),
      value: select.value,
      disabled: select.disabled,
      options: Array.from(select.options).map((option) => option.text),
    })),
    input: document.querySelector(COMPOSER)?.getAttribute("aria-label") ?? null,
    focused: active?.matches(BOX) ? "box" : active?.matches(COMPOSER) ? "composer" : "other",
    box: boxProbe(),
    switches: Array.from(document.querySelectorAll<HTMLElement>('[role="switch"]')).map((element) => {
      const rect = element.getBoundingClientRect();
      const knob = element.querySelector<HTMLElement>(".switch-knob")?.getBoundingClientRect();
      return {
        label: element.getAttribute("aria-label"),
        width: rect.width,
        height: rect.height,
        checked: element.getAttribute("aria-checked") === "true",
        background: getComputedStyle(element).backgroundColor,
        knobLeft: knob ? knob.left - rect.left : null,
      };
    }),
    boxButtons: Array.from(document.querySelectorAll<HTMLButtonElement>(`${BOX} .ya-permission-actions button`)).map((button) => ({
      name: button.textContent ?? "",
      disabled: button.disabled,
    })),
  };
}

/** Un clic comme celui d'un utilisateur (voir `ProbeInput.userClick`). */
function userClick(text: string): Probe["clicked"] {
  // Texte exact d'abord, sinon contenu ; le dernier de la page (le plus récent de la conversation).
  // Ticket 28 : un bouton sans texte (icône) par son nom, `aria-label` exact.
  const buttons = Array.from(document.querySelectorAll<HTMLElement>("button"));
  const exact = buttons.filter(
    (candidate) => candidate.textContent?.trim() === text || (!candidate.textContent?.trim() && candidate.getAttribute("aria-label") === text),
  );
  const button = (exact.length > 0 ? exact : buttons.filter((candidate) => candidate.textContent?.includes(text))).at(-1);
  if (!button) return "missing";
  button.scrollIntoView({ block: "center" });
  const rect = button.getBoundingClientRect();
  if (rect.width === 0 || rect.height === 0) return "covered";
  const x = rect.left + rect.width / 2;
  const y = rect.top + rect.height / 2;
  const hit = document.elementFromPoint(x, y);
  if (!hit) return "covered";
  if (hit.closest("[inert]")) return "inert";
  if (!(hit === button || button.contains(hit))) return "covered";
  const init = { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0 };
  hit.dispatchEvent(new PointerEvent("pointerdown", init));
  hit.dispatchEvent(new MouseEvent("mousedown", init));
  hit.dispatchEvent(new PointerEvent("pointerup", init));
  hit.dispatchEvent(new MouseEvent("mouseup", init));
  hit.dispatchEvent(new MouseEvent("click", init));
  return "clicked";
}

function act({ type, focus, keys, click, userClick: clickText, atBoxOpen }: ProbeInput): void {
  // Ticket 31 : jouées dans l'appel qui ouvre la prochaine boîte (son `useEffect`), après son focus.
  if (atBoxOpen) {
    onNextBoxOpen(() => {
      if (atBoxOpen.click !== undefined) clickBox(atBoxOpen.click);
      press(atBoxOpen.keys ?? []);
    });
  }
  if (clickText !== undefined) clicked = userClick(clickText);
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
  press(keys ?? []);
  if (click !== undefined) clickBox(click);
}

onHostMessage((message) => {
  if (message.type === "probe") post({ type: "probeResult", probe: probe() });
  if (message.type === "probeInput") {
    act(message.input);
    // Laisse React traiter l'action avant de lire la page.
    setTimeout(() => post({ type: "probeResult", probe: probe() }), 50);
  }
});

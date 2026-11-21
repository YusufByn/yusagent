// yusAgent (PR 3, étape 3, C3) : touches de la boîte de permission (mêmes que le TUI, fenêtre de
// calme de 1 s, jamais Entrée) et code montré tel qu'il tournera.

import assert from "node:assert/strict";
import { test } from "node:test";

import { QUIET_MS, QuietKeys, visibleCode } from "../../src/ya/permission-keys";

test("fenêtre de calme : rien avant 1 s, puis y autorise, n et Esc refusent", () => {
  assert.equal(QUIET_MS, 1000);
  const keys = new QuietKeys(0);
  assert.equal(keys.key({ key: "y" }, 999), "ignored", "trop tôt");
  // La frappe trop tôt a relancé la fenêtre : 1 s depuis 999.
  assert.equal(keys.key({ key: "y" }, 1500), "ignored", "fenêtre relancée par la frappe");
  // Et celle-ci aussi : 1 s depuis 1500.
  assert.equal(keys.key({ key: "y" }, 2499), "ignored");
  assert.equal(keys.key({ key: "y" }, 3499), "allow");
  assert.equal(new QuietKeys(0).key({ key: "n" }, 1000), "deny");
  assert.equal(new QuietKeys(0).key({ key: "Escape" }, 1000), "deny");
});

test("jamais Entrée ; une autre touche ou une touche avec modificateur relance la fenêtre", () => {
  const keys = new QuietKeys(0);
  assert.equal(keys.key({ key: "Enter" }, 5000), "ignored");
  assert.equal(keys.key({ key: "y" }, 5500), "ignored", "Entrée a relancé la fenêtre");
  assert.equal(keys.key({ key: "y" }, 6500), "allow", "1 s après la dernière frappe");
  const other = new QuietKeys(0);
  assert.equal(other.key({ key: "a" }, 5000), "ignored");
  assert.equal(other.key({ key: "y" }, 5999), "ignored");
  for (const modifier of ["ctrlKey", "metaKey", "altKey"] as const) {
    assert.equal(new QuietKeys(0).key({ key: "y", [modifier]: true }, 5000), "ignored", modifier);
  }
  assert.equal(new QuietKeys(0).key({ key: "Y" }, 5000), "ignored", "Y majuscule : pas une réponse");
});

test("reset : une nouvelle demande repart avec sa propre fenêtre", () => {
  const keys = new QuietKeys(0);
  keys.reset(10_000);
  assert.equal(keys.armed(10_500), false);
  assert.equal(keys.key({ key: "y" }, 10_500), "ignored");
  assert.equal(keys.armed(11_500), true);
});

test("visibleCode : contrôles et caractères invisibles échappés, retours et tabulations gardés", () => {
  assert.equal(visibleCode("a\tb\nc"), "a\tb\nc");
  assert.equal(visibleCode("x\u200by"), "x\\u{200b}y");
  assert.equal(visibleCode("rm\u202e -rf"), "rm\\u{202e} -rf");
  assert.equal(visibleCode("\u001b[2J\r"), "\\u{001b}[2J\\u{000d}");
  assert.equal(visibleCode("\ufeffprint(1)"), "\\u{feff}print(1)");
  assert.equal(visibleCode("é ü 漢"), "é ü 漢");
});

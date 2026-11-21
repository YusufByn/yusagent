// yusAgent (YA-10, ticket 24) : paramètres d'`initialize` de l'extension. Capacités client toutes
// à false et pas de `clientInfo` : rien de variable dans les trames d'ouverture. Le fixture
// `test/fixtures/acp-frames.json` fige les trames de l'ouverture et d'un prompt (preuve 1, §5.4).

import { PROTOCOL_VERSION } from "./types";

export const INITIALIZE_PARAMS = {
  protocolVersion: PROTOCOL_VERSION,
  clientCapabilities: { fs: { readTextFile: false, writeTextFile: false }, terminal: false },
};

// Tests unitaires : le module `vscode` n'existe pas hors de VS Code. Le code repris d'Unharnessed
// l'importe à l'exécution dans quelques fichiers (`files.ts`, `providers.ts`…), sans s'en servir au
// chargement : un bouchon qui répond à tout suffit (chargé par `node --require`).
import Module from "node:module";
import { join } from "node:path";

const stub: unknown = new Proxy(function stub() {}, {
  get: (_target, property) => (property === "then" ? undefined : stub),
  apply: () => stub,
  construct: () => stub as object,
});

const internals = Module as unknown as {
  _resolveFilename(request: string, ...rest: unknown[]): string;
  _cache: Record<string, unknown>;
};
const STUB_PATH = join(__dirname, "vscode-stub.js");
const resolve = internals._resolveFilename;
internals._resolveFilename = function (request: string, ...rest: unknown[]) {
  if (request === "vscode") return STUB_PATH;
  return resolve.call(this, request, ...rest);
};
internals._cache[STUB_PATH] = { id: STUB_PATH, filename: STUB_PATH, loaded: true, exports: stub };

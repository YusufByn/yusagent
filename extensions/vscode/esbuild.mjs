// Groupage (E3) : l'hôte de l'extension (CommonJS, Node) dans dist/extension.js, et la webview
// React (IIFE + CSS, navigateur) dans dist/webview/main.js et main.css. Minifiés, sans source map.
// Repris d'Unharnessed (157dcdc1d, vscode/esbuild.mjs), sans le mode --watch ni les polices.
import { build } from "esbuild";

const common = { bundle: true, minify: true, sourcemap: false, logLevel: "info", legalComments: "none" };

await Promise.all([
  build({
    ...common,
    entryPoints: ["src/extension.ts"],
    outfile: "dist/extension.js",
    platform: "node",
    format: "cjs",
    target: "node20",
    external: ["vscode"],
  }),
  build({
    ...common,
    entryPoints: { main: "webview/main.tsx" },
    outdir: "dist/webview",
    platform: "browser",
    format: "iife",
    target: "chrome120",
    jsx: "automatic",
    define: { "process.env.NODE_ENV": '"production"' },
  }),
]);

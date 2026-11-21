// Groupage (E3) : l'hôte de l'extension (CommonJS, Node) et la webview (IIFE + CSS, navigateur),
// minifiés, sans source map dans le .vsix.
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
    entryPoints: ["webview/index.tsx"],
    outfile: "dist/webview.js",
    platform: "browser",
    format: "iife",
    target: "es2022",
    jsx: "automatic",
    define: { "process.env.NODE_ENV": '"production"' },
  }),
]);

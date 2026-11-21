import js from "@eslint/js";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist/**", "out/**", "node_modules/**", ".vscode-test/**", "*.vsix"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["test/fake-agent/**/*.mjs", "scripts/**/*.mjs", "esbuild.mjs"],
    languageOptions: { globals: { process: "readonly", console: "readonly", setTimeout: "readonly" } },
  },
);

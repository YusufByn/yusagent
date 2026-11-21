import js from "@eslint/js";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist/**", "out/**", "node_modules/**", ".vscode-test/**", "*.vsix"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    // Le code repris d'Unharnessed nomme `_` ce qu'il ignore exprès.
    rules: {
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_", destructuredArrayIgnorePattern: "^_", ignoreRestSiblings: true },
      ],
    },
  },
  {
    files: ["test/fake-agent/**/*.mjs", "scripts/**/*.mjs", "esbuild.mjs"],
    languageOptions: { globals: { process: "readonly", console: "readonly", setTimeout: "readonly" } },
  },
);

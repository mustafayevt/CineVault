import { createRequire } from "node:module";
import js from "@eslint/js";
import globals from "globals";
import jsxA11y from "eslint-plugin-jsx-a11y";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import prettier from "eslint-config-prettier";

// typescript-eslint cannot load the TypeScript 7 compiler API yet, so point its
// `require("typescript")` at the side-by-side TypeScript 6 package. The compiler
// used by `bun run typecheck` and the build is still TypeScript 7.
const require = createRequire(import.meta.url);
const Module = require("node:module");
const resolveFilename = Module._resolveFilename;
Module._resolveFilename = function (request, ...rest) {
  return resolveFilename.call(
    this,
    request === "typescript" ? "@typescript/typescript6" : request,
    ...rest,
  );
};
const { default: tseslint } = await import("typescript-eslint");

export default tseslint.config(
  { ignores: ["dist/", "node_modules/", "src/bindings.ts"] },
  js.configs.recommended,
  ...tseslint.configs.strict,
  jsxA11y.flatConfigs.recommended,
  reactHooks.configs.flat.recommended,
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      ecmaVersion: 2023,
      globals: { ...globals.browser },
    },
    plugins: { "react-refresh": reactRefresh },
    rules: {
      "@typescript-eslint/no-explicit-any": "error",
      "@typescript-eslint/consistent-type-imports": "error",
      "react-refresh/only-export-components": ["warn", { allowConstantExport: true }],
    },
  },
  {
    files: ["**/*.test.{ts,tsx}", "src/test/**"],
    languageOptions: { globals: { ...globals.browser, ...globals.vitest } },
  },
  {
    files: ["*.config.{js,ts}", "scripts/**/*.mjs"],
    languageOptions: { globals: { ...globals.node } },
  },
  prettier,
);

import js from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import tseslint from "typescript-eslint";

const nodeGlobals = { console: "readonly", process: "readonly", Buffer: "readonly", URL: "readonly", structuredClone: "readonly", setTimeout: "readonly", clearTimeout: "readonly", AbortSignal: "readonly", fetch: "readonly", globalThis: "readonly", TextDecoder: "readonly", TextEncoder: "readonly", AbortController: "readonly", setInterval: "readonly", clearInterval: "readonly", queueMicrotask: "readonly", URLSearchParams: "readonly", Response: "readonly", Headers: "readonly" };

export default tseslint.config(
  { ignores: ["dist/**", "out/**", "node_modules/**", "src/renderer/model/contract/*.generated.ts", "../../packages/design-system/node_modules/**"] },
  js.configs.recommended,
  tseslint.configs.recommended,
  {
    rules: {
      // Intentional: validators reject control characters in user text.
      "no-control-regex": "off",
      "no-useless-assignment": "off",
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_", varsIgnorePattern: "^_", caughtErrors: "none", ignoreRestSiblings: true }],
    },
  },
  { files: ["**/*.mjs"], languageOptions: { globals: nodeGlobals } },
  { files: ["scripts/ui-shots.mjs"], languageOptions: { globals: { window: "readonly" } } },
  {
    files: ["src/**/*.{ts,tsx}", "../../packages/design-system/src/**/*.{ts,tsx}", "vite.renderer.config.ts"],
    plugins: { "react-hooks": reactHooks, "react-refresh": reactRefresh },
    rules: {
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "error",
      "react-refresh/only-export-components": "off",
    },
  },
);

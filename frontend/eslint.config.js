// Flat ESLint config. Like the backend's ruff setup, this is a small
// high-signal set: the type-aware rules that catch real mistakes, plus the
// React Hooks rules, which are the ones that actually bite in this codebase
// (since react-hooks 7 that includes the React Compiler's rules of React).
import js from "@eslint/js";
import { defineConfig } from "eslint/config";
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

export default defineConfig(
  { ignores: ["dist/**", "node_modules/**", "src/data/**", "playwright-report/**", "test-results/**"] },
  js.configs.recommended,
  tseslint.configs.recommended,
  {
    files: ["**/*.{ts,tsx}"],
    extends: [reactHooks.configs.flat.recommended],
    rules: {
      // Underscore-prefixed args are the codebase's "deliberately unused" mark.
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
    },
  },
  {
    // Build scripts are plain Node modules, not part of the app's TS project.
    files: ["scripts/**/*.mjs", "e2e/**/*.mjs"],
    languageOptions: { globals: { process: "readonly", console: "readonly" } },
  },
);

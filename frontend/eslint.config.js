// Flat ESLint config. Like the backend's ruff setup, this is a small
// high-signal set: the type-aware rules that catch real mistakes, plus the
// React Hooks rules, which are the ones that actually bite in this codebase
// (since react-hooks 7 that includes the React Compiler's rules of React).
import js from "@eslint/js";
import { defineConfig } from "eslint/config";
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

/** A hex colour (#rgb, #rgba, #rrggbb, #rrggbbaa) in a string or template
 *  literal: a class like bg-[#e5f5eb], a style value, a var() fallback.
 *  Not a link's #anchor after a page name ("page.html#abc"), nor an HTML
 *  entity (&#123;). */
function hexColourRules() {
  const hex = "/(^|[^\\w&#])#([0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})(?![\\w-])/i";
  const message = "Use a theme colour token, var(--ss-…) from index.css, not a hex colour (GUI-14).";
  return [
    { selector: `Literal[value=${hex}]`, message },
    { selector: `TemplateElement[value.raw=${hex}]`, message },
  ];
}

export default defineConfig(
  {
    ignores: [
      "dist/**", "node_modules/**", "src/data/**", "playwright-report/**", "test-results/**",
      // the built help pages (scripts/build-docs.mjs)
      "public/help/**",
    ],
  },
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
    // GUI-14: colours come from the theme's tokens in index.css (written
    // var(--ss-…) in a class or a style), which have a light and a dark value
    // and are checked for contrast in contrast.test.ts. A hex colour in a
    // component is the same in both themes: that is how faint badges and
    // pins crept into the dark theme. Add a token instead.
    files: ["src/**/*.{ts,tsx}"],
    ignores: [
      "src/**/*.test.{ts,tsx}",
      // the chart palette: series colours drawn on a canvas, which cannot
      // read CSS variables (each is checked in contrast.test.ts)
      "src/components/panels/chartUtils.ts",
      // TODO(GUI-14): these still hold hex colours and were being changed by
      // other 0.3 work when the rule came in; move their colours to tokens
      // and take them off this list
      "src/reports.ts",
      "src/components/panels/ResultsPanel.tsx",
      "src/components/panels/EnergyView.tsx",
      "src/components/ImportTableDialog.tsx",
    ],
    rules: {
      "no-restricted-syntax": ["error", ...hexColourRules()],
    },
  },
  {
    // Build scripts are plain Node modules, not part of the app's TS project.
    files: ["scripts/**/*.mjs", "e2e/**/*.mjs"],
    languageOptions: { globals: { process: "readonly", console: "readonly" } },
  },
  {
    // The help pages' own script: plain browser code, not part of the app.
    files: ["scripts/help-assets/*.js"],
    languageOptions: { globals: { window: "readonly", document: "readonly" } },
  },
);

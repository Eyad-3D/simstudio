// Unit tests (Vitest). Kept apart from vite.config.ts: the tests exercise
// stores and helpers, so they need a DOM (jsdom: localStorage, document) but
// none of the app build's plugins. The browser tests in e2e/ run under
// Playwright instead (see playwright.config.ts).
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
    restoreMocks: true,
  },
});

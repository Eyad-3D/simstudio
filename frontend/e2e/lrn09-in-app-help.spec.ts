// LRN-09: the help inside the app. F1 on a part, a panel or a parameter, the
// "?" in each panel's header and the Help menu open the matching page in the
// Help panel, with no network; links out of the help go to the browser; the
// release notes show once after an update; the Results summary explains its
// rows.
import { readFileSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { openApp, ribbonTab, showPanel } from "./app";

const VERSION = readFileSync(new URL("../../VERSION", import.meta.url), "utf8").trim();
const helpTitle = (page: Page) => page.frameLocator("iframe[title='Help page']").getByRole("heading", { level: 1 });

test("LRN-09: F1 opens the right page for every part in the library, offline, within 1 s", async ({ page, context, baseURL }) => {
  await context.route((url) => !url.href.startsWith(baseURL!), (route) => route.abort());
  await openApp(page);
  const lib = await (await page.request.get("/api/library")).json();
  const parts = lib.components as { id: string; name: string }[];
  expect(parts.length).toBeGreaterThanOrEqual(32);
  // each part's page is where F1 sends it (the store's selection), and opens fast
  for (const c of parts) {
    const t0 = Date.now();
    await page.evaluate((page) => window.lightsimHelp!(page), `reference/components/${c.id}.html`);
    await expect(helpTitle(page)).toHaveText(c.name);
    expect(Date.now() - t0).toBeLessThan(1000);
  }
  // and the F1 key itself, on a part of the example
  await page.getByRole("button", { name: "Close the help" }).click();
  await page.locator(".react-flow__node", { hasText: "E-Motor" }).first().click();
  await page.keyboard.press("F1");
  await expect(helpTitle(page)).toHaveText("E-Motor");
  // Esc in the panel closes it
  await page.getByRole("complementary", { name: "Help" }).getByRole("button", { name: "Back" }).focus();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("complementary", { name: "Help" })).toHaveCount(0);
});

test("LRN-09: each panel's ? opens its how-to page; F1 inside a panel does too", async ({ page }) => {
  await openApp(page);
  await showPanel(page, "Data Bus Connections");
  await page.getByRole("button", { name: "Help on Data Bus Connections" }).click();
  await expect(helpTitle(page)).toHaveText("Wire control signals");
  await showPanel(page, "Problems");
  await page.getByRole("button", { name: "Help on Problems" }).click();
  await expect(helpTitle(page)).toHaveText("Find and fix problems in a model");
  // F1 with the focus in the Cases panel and nothing selected
  await page.locator(".react-flow__pane").click();
  await showPanel(page, "Cases & Parameters");
  await page.locator("[data-help-panel='how-to/parameter-sweep.html'] select").first().focus();
  await page.keyboard.press("F1");
  await expect(helpTitle(page)).toHaveText("Run a parameter sweep");
});

test("LRN-09: the Help menu lists the help's main pages, and links out of the help go to the browser", async ({ page, context }) => {
  await openApp(page);
  await page.getByRole("button", { name: "Help", exact: true }).click();
  const items = await page.getByRole("menu", { name: "Help" }).getByRole("menuitem").allTextContents();
  expect(items).toEqual(expect.arrayContaining(["Documentation", "Known limits", "What is validated", "Release notes",
    "Keyboard shortcuts", "Examples guide", "Report a problem…", "Show the first-steps tour"]));
  await page.getByRole("menuitem", { name: "Known limits" }).click();
  await expect(helpTitle(page)).toHaveText(/known issues/);
  // a link to the repo (GitHub) opens outside the panel
  const frame = page.frameLocator("iframe[title='Help page']");
  const out = frame.locator("main a[href^='https://github.com']").first();
  if (await out.count()) {
    // answered here, so the test needs no network
    await context.route("https://github.com/**", (r) =>
      r.fulfill({ status: 200, contentType: "text/html", body: "<title>GitHub</title>" }),
    );
    const [popup] = await Promise.all([context.waitForEvent("page"), out.click()]);
    expect(popup.url()).toMatch(/^https:\/\/github\.com\//);
    await popup.close();
    await expect(helpTitle(page)).toHaveText(/known issues/);
  }
  // Open in browser hands the page shown to the browser
  const [tab] = await Promise.all([context.waitForEvent("page"), page.getByRole("button", { name: "Open in browser" }).click()]);
  await expect(tab).toHaveURL(/\/help\/known-limits\.html$/);
});

test("LRN-09: What's new shows the release notes once after an update, never on a first start", async ({ page }) => {
  await page.addInitScript(() => {
    if (!sessionStorage.getItem("seeded")) {
      localStorage.setItem("lightsim-last-version", "0.0.1");
      sessionStorage.setItem("seeded", "1");
    }
  });
  await openApp(page);
  await expect(helpTitle(page)).toHaveText("Release notes");
  expect(await page.evaluate(() => localStorage.getItem("lightsim-last-version"))).toBe(VERSION);
  await page.reload();
  await expect(page.locator(".react-flow__node").first()).toBeVisible();
  await page.waitForTimeout(500);
  await expect(page.getByRole("complementary", { name: "Help" })).toHaveCount(0);
});

test("LRN-10: each Results summary row says what it means, and links to the definitions", async ({ page }) => {
  await openApp(page);
  await page.getByRole("button", { name: /^Run/, exact: false }).filter({ hasText: "Run" }).first().click();
  await ribbonTab(page, "Results").click();
  await expect(page.getByText(/^success$/).first()).toBeVisible({ timeout: 60_000 });
  await page.getByText(/^All summary values/).click();
  const row = page.getByRole("region", { name: "Summary" }).getByRole("cell", { name: "Consumption", exact: true });
  await expect(row).toHaveAttribute("title", /per 100 km/);
  const rows = page.getByRole("region", { name: "Summary" }).locator("tbody tr td:first-child");
  for (const cell of await rows.all()) await expect(cell).toHaveAttribute("title", /\S/);
  await page.getByRole("button", { name: "what they mean" }).click();
  await expect(helpTitle(page)).toHaveText("Results summary values");
});

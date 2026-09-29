// LRN-04: the help pages, served by the engine, with no network, and the
// ways into them from the app (F1, the header's Help button).
import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";
import { openApp } from "./app";

const VERSION = readFileSync(new URL("../../VERSION", import.meta.url), "utf8").trim();

test("LRN-04: the help works offline, has every section, searches fast and carries the app's version", async ({
  page,
  context,
  baseURL,
}) => {
  const outside: string[] = [];
  await context.route(
    (url) => !url.href.startsWith(baseURL!),
    (route) => {
      outside.push(route.request().url());
      return route.abort();
    },
  );
  await page.goto("/help/index.html");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("LightSim help");
  await expect(page.locator("footer")).toHaveText(`LightSim ${VERSION}`);
  const { pages, sections, broken } = await page.evaluate(async () => {
    const index = (window as unknown as { HELP_INDEX: { u: string; s: string }[] }).HELP_INDEX;
    const status = await Promise.all(index.map((p) => fetch(p.u).then((r) => r.status)));
    return {
      pages: index.length,
      sections: [...new Set(index.map((p) => p.s))],
      broken: index.filter((_, i) => status[i] !== 200).map((p) => p.u),
    };
  });
  expect(pages).toBeGreaterThanOrEqual(30);
  expect(sections.sort()).toEqual(
    [
      "Get started", "Tutorials", "How-to guides", "Examples", "Reference", "Theory",
      "Validation", "Known issues", "Release notes", "Data sources", "Glossary",
    ].sort(),
  );
  expect(broken).toEqual([]);
  // the pictures the quick start shows are served too
  await page.goto("/help/quick-start.html");
  for (const img of await page.locator("main img").all()) {
    expect(await img.evaluate((i: HTMLImageElement) => i.complete && i.naturalWidth > 0)).toBe(true);
  }
  const [ms, first] = await page.evaluate(() => {
    const q = document.getElementById("q") as HTMLInputElement;
    const t = performance.now();
    q.value = "state of charge";
    q.dispatchEvent(new Event("input"));
    return [performance.now() - t, document.querySelector("#hits a")?.textContent];
  });
  expect(first).toBeTruthy();
  expect(ms).toBeLessThan(200);
  await page.getByLabel("Search the help").fill("pick drive cycle");
  await page.locator("#hits a").first().click();
  await expect(page).toHaveURL(/\/help\/how-to\/pick-a-drive-cycle\.html$/);
  expect(outside).toEqual([]);
});

test("LRN-04: F1 opens the selected part's page, the Help button the front page", async ({ page, context }) => {
  await openApp(page);
  await page.locator(".react-flow__node", { hasText: "Vehicle Task" }).first().click();
  const [part] = await Promise.all([context.waitForEvent("page"), page.keyboard.press("F1")]);
  await expect(part).toHaveURL(/\/help\/reference\/components\/signal\.driving_task\.html$/);
  await expect(part.getByRole("heading", { level: 1 })).toHaveText("Driving Task");
  // each parameter has an anchor, for links straight to it
  await expect(part.locator("#cycle")).toHaveCount(1);
  const [front] = await Promise.all([
    context.waitForEvent("page"),
    page.getByRole("button", { name: "Help (F1)" }).click(),
  ]);
  await expect(front).toHaveURL(/\/help\/index\.html$/);
});

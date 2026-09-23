// STU-03: each parameter sweep is saved with its project as a study with its
// results table; a second sweep adds a study instead of replacing the first,
// and both survive a reload and a save.
import { expect, test, type Page } from "@playwright/test";
import { expectProject, openApp, ribbonTab, showPanel } from "./app";
import { importProject } from "./ui-helpers";

/** Sweep the Vehicle's mass over `steps` values and wait for the Results page. */
async function sweepVehicleMass(page: Page, steps: number): Promise<void> {
  await ribbonTab(page, "Home").click();
  await showPanel(page, "Cases & Parameters");
  const sweepElement = page.locator("select", { has: page.locator("option", { hasText: "Element…" }) }).nth(1);
  await sweepElement.selectOption({ label: "Vehicle" });
  await page.locator("input[type=number][max='16']").fill(String(steps));
  await page.getByRole("button", { name: `Run sweep (${steps})` }).click();
  await expect(page.getByPlaceholder("Search channels…")).toBeVisible({ timeout: 60_000 });
}

/** The saved studies, newest first, as the Cases panel lists them. */
async function studies(page: Page) {
  await ribbonTab(page, "Home").click();
  await showPanel(page, "Cases & Parameters");
  return page.getByRole("region", { name: "Saved studies" }).locator("[aria-expanded]");
}

/** Open a study's card (if closed) and read its table's body rows. */
async function tableOf(page: Page, index: number): Promise<string[][]> {
  const card = (await studies(page)).nth(index);
  if ((await card.getAttribute("aria-expanded")) !== "true") await card.click();
  const table = page.getByRole("region", { name: "Saved studies" }).getByRole("table").nth(index);
  return table.locator("tbody tr").evaluateAll((rows) =>
    rows.map((r) => [...r.querySelectorAll("td")].map((td) => td.textContent!.trim())),
  );
}

test("STU-03: two sweeps give two saved studies that survive a reload and a save", async ({ page }) => {
  const n = Date.now() % 100000;
  const id = `e2e-stu03-${n}`;
  const name = `E2E studies ${n}`;
  await openApp(page);
  const example = await (await page.request.get("/api/examples/bev-car")).json();
  example.cases[0].duration = 20;
  await importProject(page, { ...example, id, name });
  // the sweep form seeds 50-150 % of the example's own mass
  const mass: number = example.systems
    .flatMap((s: { elements: { label: string; parameterOverrides: Record<string, number> }[] }) => s.elements)
    .find((e: { label: string }) => e.label === "Vehicle").parameterOverrides.mass_kg;
  const [lo, mid, hi] = [mass * 0.5, mass, mass * 1.5].map(String);

  await sweepVehicleMass(page, 2);
  await expect(await studies(page)).toHaveCount(1);
  const first = await tableOf(page, 0);
  expect(first.map((row) => [row[0], row[2]])).toEqual([
    [lo, "complete"],
    [hi, "complete"],
  ]);
  expect(first.every((row) => /^[\d.,-]+$/.test(row[1]))).toBe(true); // a result per point

  await sweepVehicleMass(page, 3);
  await expect(await studies(page)).toHaveCount(2);
  expect((await tableOf(page, 0)).map((row) => row[0])).toEqual([lo, mid, hi]);
  expect(await tableOf(page, 1), "the first study's table is intact").toEqual(first);

  // unsaved: the recovery draft brings the studies back after a reload
  await page.reload();
  await expectProject(page, name, { unsaved: true });
  await expect(await studies(page)).toHaveCount(2);
  expect(await tableOf(page, 1)).toEqual(first);

  // saved: they are in the project file
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expectProject(page, name, { unsaved: false });
  const saved = await (await page.request.get(`/api/projects/${id}`)).json();
  expect(saved.studies.map((s: { points: unknown[] }) => s.points.length)).toEqual([2, 3]);
  expect(saved.studies[0].factors[0]).toMatchObject({ elementLabel: "Vehicle", paramLabel: "Vehicle Mass", values: [mass * 0.5, mass * 1.5] });
  expect(Object.keys(saved.studies[0].points[0].kpis).length).toBeGreaterThan(3);
});

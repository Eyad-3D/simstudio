// ENG-34: a Driving Task over distance shows a Laps field in the case
// settings, and the run ends after those laps. ENG-33: the case's Charge
// balance setting is saved with the case.
import { expect, test } from "@playwright/test";
import { expectProject, openApp, ribbonTab, runActiveCase, showPanel } from "./app";
import { importProject } from "./ui-helpers";

const LAP = "0:30; 200:30; 300:90; 600:90; 700:40; 800:40; 900:70; 1100:70; 1200:30";

test("ENG-34: a case over distance ends after its laps", async ({ page }) => {
  const n = Date.now() % 100000;
  const id = `e2e-eng34-${n}`;
  await openApp(page);
  const example = await (await page.request.get("/api/examples/bev-car")).json();
  const c = example.cases[0];
  c.duration = 300;
  c.parameterOverrides = { ...c.parameterOverrides, "el-task": { profile: LAP, cycle: "" } };
  await importProject(page, { ...example, id, name: `E2E laps ${n}` });

  await ribbonTab(page, "Home").click();
  await showPanel(page, "Cases & Parameters");
  const laps = page.locator("label", { hasText: /^Laps/ }).locator("input");
  await expect(laps, "no Laps over time").toHaveCount(0);

  // over distance, lap after lap (this case's own values): a Laps field
  const row = (key: string) => page.locator("li", { hasText: `Vehicle Task · ${key}` });
  await page.locator("select", { has: page.locator("option", { hasText: "Element…" }) }).first().selectOption({ label: "Vehicle Task" });
  await page.locator("select", { has: page.locator("option", { hasText: "Profile Axis" }) }).first().selectOption({ label: "Profile Axis" });
  await page.getByRole("button", { name: /Add override/ }).click();
  await row("Profile Axis").locator("select").selectOption("distance");
  await expect(laps).toHaveCount(1);
  await page.locator("select", { has: page.locator("option", { hasText: "Element…" }) }).first().selectOption({ label: "Vehicle Task" });
  await page.locator("select", { has: page.locator("option", { hasText: "Repeat Profile" }) }).first().selectOption({ label: "Repeat Profile" });
  await page.getByRole("button", { name: /Add override/ }).click();
  await row("Repeat Profile").locator("select").selectOption("true");
  await laps.fill("2");

  await runActiveCase(page);
  await expect(page.getByText("success").first()).toBeVisible({ timeout: 60_000 });
  // the stored run drove two 1,200 m laps and stopped there
  await expect
    .poll(async () => {
      const runs = await (await page.request.get(`/api/projects/${id}/runs`)).json();
      return runs[0]?.summary.find((s: { label: string }) => s.label === "Distance driven")?.value;
    })
    .toBeCloseTo(2.4, 3);
});

test("ENG-33: Charge balance is a case setting, Auto unless set", async ({ page }) => {
  const n = Date.now() % 100000;
  const id = `e2e-eng33-${n}`;
  const name = `E2E balance ${n}`;
  await openApp(page);
  const example = await (await page.request.get("/api/examples/bev-car")).json();
  await importProject(page, { ...example, id, name });
  await ribbonTab(page, "Home").click();
  await showPanel(page, "Cases & Parameters");
  const balance = page.locator("label", { hasText: /^Charge balance/ }).locator("select");
  await expect(balance).toHaveValue("auto");
  await balance.selectOption("off");
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expectProject(page, name, { unsaved: false });
  const saved = await (await page.request.get(`/api/projects/${id}`)).json();
  expect(saved.cases[0].chargeBalance).toBe(false);
});

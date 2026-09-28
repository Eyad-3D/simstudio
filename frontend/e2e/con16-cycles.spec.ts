// CON-16: pick a standard drive cycle from a list, with a preview.
import { expect, test } from "@playwright/test";
import { openApp, ribbonTab, showPanel } from "./app";

test("CON-16: WLTC in three clicks sets the case length and shows the trace's figures", async ({ page }) => {
  await openApp(page);
  await page.locator(".react-flow__node", { hasText: "Vehicle Task" }).first().click(); // 1
  await showPanel(page, "Properties");
  const field = page.getByRole("combobox", { name: "Drive Cycle" });
  await field.click(); // 2: opens the list
  await page.keyboard.press("Escape");
  await field.selectOption("wltc-3b"); // 3
  // published: 1,800 s and 23.266 km (UN GTR No. 15)
  await expect(page.getByText("WLTC class 3b: 1,800 s · 23.27 km · top 131.3 km/h")).toBeVisible();
  await expect(page.getByRole("img", { name: /^Speed over time, WLTC class 3b/ })).toBeVisible();
  // the typed profile's editor is hidden while a cycle drives the task
  await expect(page.getByRole("button", { name: /^Profile.*Edit…$/ })).toHaveCount(0);
  await ribbonTab(page, "Simulations").click();
  await expect(page.getByTitle("Solver settings for this case")).toHaveText(/^1800s/);

  // back to the typed profile: its editor and its own sketch return
  await field.selectOption("");
  await expect(page.getByText(/^Custom profile: 600 s · /)).toBeVisible();
  await expect(page.getByRole("button", { name: /^Profile.*Edit…$/ })).toBeVisible();
});

test("CON-16: a case picks its own cycle and takes its length", async ({ page }) => {
  await openApp(page);
  await showPanel(page, "Cases & Parameters");
  // any case list sets the active case
  const caseList = page.locator("select", { has: page.locator("option", { hasText: "WLTC Class 3b" }) });
  await caseList.first().selectOption({ label: "WLTC Class 3b" });
  const cycle = page.getByRole("combobox", { name: "Vehicle Task · Drive Cycle" });
  await expect(cycle).toHaveValue("wltc-3b");
  await cycle.selectOption("udds");
  await expect(page.getByLabel("Duration (s)")).toHaveValue("1369");
});

test("CON-16: the library search finds cycles and adds a Driving Task on one", async ({ page }) => {
  await openApp(page);
  await showPanel(page, "Components");
  const search = page.getByPlaceholder("Search components…");
  await search.fill("nedc"); // no NEDC trace ships, so nothing may promise one
  await expect(page.getByText("No components match “nedc”.")).toBeVisible();
  await search.fill("drive cycle");
  await expect(page.getByRole("button", { name: /^Add a Driving Task on / })).toHaveCount(3);
  await search.fill("hwfet");
  const before = await page.locator(".react-flow__node").count();
  const hwfet = page.getByRole("button", { name: /^Add a Driving Task on EPA highway \(HWFET\) · / });
  await hwfet.click();
  await expect(page.locator(".react-flow__node")).toHaveCount(before + 1);
  await hwfet.dblclick(); // a double-click adds one task, not two
  await expect(page.locator(".react-flow__node")).toHaveCount(before + 2);
  await showPanel(page, "Properties");
  await expect(page.getByRole("combobox", { name: "Drive Cycle" })).toHaveValue("hwfet");
});

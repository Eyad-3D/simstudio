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
  // published: 1,800 s and 23.266 km (EU 2017/1151); the source and the
  // reason LightSim may ship it show with it (CON-31)
  await expect(page.getByText("WLTC class 3b: 1,800 s · 23.27 km · top 131.3 km/h")).toBeVisible();
  await expect(page.getByTestId("cycle-source")).toContainText(
    "Source: Commission Regulation (EU) 2017/1151, Annex XXI, Sub-Annex 1",
  );
  await expect(page.getByTestId("cycle-source")).toContainText("Source: EUR-Lex, © European Union");
  await expect(page.getByRole("img", { name: /^Speed over time, WLTC class 3b/ })).toBeVisible();
  // the typed profile's editor is hidden while a cycle drives the task
  await expect(page.getByRole("button", { name: /^Profile.*Edit…$/ })).toHaveCount(0);
  await ribbonTab(page, "Simulations").click();
  await expect(page.getByTitle("Solver settings for this case")).toHaveText(/^1800s/);

  // back to the typed profile: its editor and its own sketch return
  await field.selectOption("");
  await expect(page.getByText(/^Custom profile \(not a standard cycle\): 600 s · /)).toBeVisible();
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
  await search.fill("nedc"); // NEDC ships since CON-04
  await expect(page.getByRole("button", { name: /^Add a Driving Task on NEDC \(withdrawn\) · 1,180 s/ })).toBeVisible();
  await search.fill("drive cycle");
  await expect(page.getByRole("button", { name: /^Add a Driving Task on / })).toHaveCount(27);
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

test("CON-11: a Road Profile takes its grade from a cycle that has one", async ({ page }) => {
  await openApp(page);
  await showPanel(page, "Components");
  await page.getByPlaceholder("Search components…").fill("road profile");
  await page.getByRole("button", { name: "Add Road Profile" }).dblclick();
  await showPanel(page, "Properties");
  const field = page.getByRole("combobox", { name: "Grade From Cycle" });
  // only the cycles with a grade column are offered
  await expect(field.locator("option")).toHaveText([
    "Custom profile (typed points)",
    /^Long-haul truck route \(804\.6 km, with grade\) · 83,042 s · 804\.62 km · with grade$/,
    /^Long-haul truck route, first 100 km \(with grade\) · 5,903 s · 100\.01 km · with grade$/,
    "Import a cycle from a file…", // a file of one's own with a grade (CON-11)
  ]);
  await expect(page.getByLabel("Profile Axis")).toBeVisible();
  await field.selectOption("long-haul-100km");
  // the typed grade and its axis are not used while a cycle sets the grade
  await expect(page.getByLabel("Profile Axis")).toHaveCount(0);
  await expect(page.getByTestId("cycle-source")).toContainText("longHaulDriveCycle.csv");
});

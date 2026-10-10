// LRN-07 / LRN-12: replay the tutorial "Your first electric car", part 2,
// in the app, the way a reader clicks it: a blank project, seven parts from
// the library, four wires drawn between their pins, three signals picked in
// Data Bus Connections, the wheel's load share, the car's values and the
// WLTC, then Run. Each step's **Check:** from the page is checked here; the
// numbers come from docs/help/checks.json, which the backend tests run too.
import { readFileSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { headlineTile, openApp, runActiveCase, showPanel } from "./app";
import { newProject } from "./ui-helpers";

test.use({ viewport: { width: 1600, height: 1000 } });

const page_ = readFileSync(new URL("../../docs/help/tutorials/first-electric-car.md", import.meta.url), "utf8");
const checks = JSON.parse(readFileSync(new URL("../../docs/help/checks.json", import.meta.url), "utf8")) as {
  facts: { page: string; text: string }[];
};
const quoted = (text: string) => {
  // the test checks only what the page still says
  expect(page_.replace(/\s+/g, " ")).toContain(text);
  expect(checks.facts.some((f) => f.page === "tutorials/first-electric-car.md" && f.text.includes(text))).toBe(true);
  return text;
};

const PARTS: [string, string][] = [
  ["vehicle.body", "Vehicle 1"],
  ["signal.driving_task", "Driving Task 1"],
  ["driver.driver", "Driver 1"],
  ["battery.generic", "HV Battery Pack 1"],
  ["motor.emotor", "E-Motor 1"],
  ["mech.final_drive", "Final Drive 1"],
  ["propulsion.wheel", "Wheel 1"],
];

const node = (page: Page, label: string) => page.locator(".react-flow__node", { hasText: label }).first();

/** Drag a wire from one part's pin to another's, as a reader does. */
async function wire(page: Page, from: string, fromPin: string, to: string, toPin: string) {
  const pin = (label: string, id: string) => node(page, label).locator(`.react-flow__handle.source[data-handleid="${id}"]`);
  const a = (await pin(from, fromPin).boundingBox())!;
  const b = (await pin(to, toPin).boundingBox())!;
  await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
  await page.mouse.down();
  await page.mouse.move((a.x + b.x) / 2, (a.y + b.y) / 2, { steps: 5 });
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2, { steps: 5 });
  await page.mouse.up();
}

async function problems(page: Page) {
  await showPanel(page, "Problems");
  return page.locator(".ss-panel-toolbar span", { hasText: /errors?, \d+ warnings?/ }).first();
}

async function setNumber(page: Page, label: string, field: string | RegExp, value: string) {
  await node(page, label).click();
  await showPanel(page, "Properties");
  const box = page.getByRole("spinbutton", { name: field }).first();
  await box.fill(value);
  await box.press("Enter");
}

test("LRN-07: the tutorial's own car builds, checks and runs as its page says", async ({ page }) => {
  test.setTimeout(180_000);
  await openApp(page);
  await newProject(page);

  // 3. the seven parts: Problems lists 5 errors
  for (const [id] of PARTS) await page.locator(`[data-component-id='${id}']`).dblclick();
  for (const [, label] of PARTS) await expect(node(page, label)).toBeVisible();
  quoted("lists 5 errors");
  await expect(await problems(page)).toHaveText(/^5 errors, /);
  await expect(page.getByText("Vehicle present but no connected wheels — it will not move").first()).toBeVisible();

  // 4. the power path: 2 errors, about signals
  // every part in view, so each pin can be reached
  await page.getByRole("combobox", { name: "Zoom" }).selectOption("fit");
  await page.waitForTimeout(400);
  await wire(page, "HV Battery Pack 1", "pos", "E-Motor 1", "pos");
  await wire(page, "HV Battery Pack 1", "neg", "E-Motor 1", "neg");
  await wire(page, "E-Motor 1", "shaft", "Final Drive 1", "flange_in");
  await wire(page, "Final Drive 1", "flange_out", "Wheel 1", "shaft");
  await expect(page.locator(".react-flow__edge")).toHaveCount(4);
  quoted("now lists 2 errors");
  await expect(await problems(page)).toHaveText(/^2 errors, /);

  // 5. the three signals: no errors, one warning
  await showPanel(page, "Data Bus Connections");
  for (const [to, from] of [
    ["Driver 1 · Target Speed", "Driving Task 1 · Target Speed"],
    ["Driver 1 · Actual Speed", "Vehicle 1 · Vehicle Speed"],
    ["E-Motor 1 · Traction Command", "Driver 1 · Traction Command"],
  ]) {
    await page.getByRole("combobox", { name: `Source of ${to}`, exact: true }).click();
    await page.getByRole("listbox").getByRole("option", { name: new RegExp(`^${from.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`) }).first().click();
  }
  quoted("has no errors left, and one warning: *Wheel load shares add up to 25 %, not 100 %*");
  await expect(await problems(page)).toHaveText("0 errors, 1 warning");
  await expect(page.getByText(/Wheel load shares add up to 25 %, not 100 %/).first()).toBeVisible();

  // 6. the wheel carries the car: all clear
  await setNumber(page, "Wheel 1", /^Vehicle Load Share/, "100");
  quoted("*Problems* says *All data checks passed*");
  await expect(await problems(page)).toHaveText("0 errors, 0 warnings");
  await expect(page.getByText(/All data checks passed/).first()).toBeVisible();

  // 7. the car's values and the WLTC, then Run
  await setNumber(page, "Vehicle 1", /^Vehicle Mass/, "1200");
  await setNumber(page, "Vehicle 1", /^Drag Coefficient/, "0.3");
  await setNumber(page, "Vehicle 1", /^Frontal Area/, "2.1");
  await setNumber(page, "HV Battery Pack 1", /^Usable Capacity/, "30");
  await node(page, "Driving Task 1").click();
  await showPanel(page, "Properties");
  await page.getByRole("combobox", { name: "Drive Cycle" }).selectOption("wltc-3b");
  await runActiveCase(page);
  await expect(page.getByText(/^success$/).first()).toBeVisible({ timeout: 90_000 });
  quoted("LightSim gives 10.46 kWh/100 km with the values above");
  await expect(headlineTile(page, "Consumption")).toContainText("10.459");
  await expect(headlineTile(page, "Distance driven")).toContainText("23.267");
});

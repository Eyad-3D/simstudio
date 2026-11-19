// UX-19: the diagram's keys work while the diagram has the keyboard (the
// focus is in it, or it was the last place clicked), not only with the
// pointer over it, and never while typing in a field. Ctrl+A, Ctrl+X, F2
// and Enter are new; Delete, the ribbon's Delete and Ctrl+X remove the whole
// selection in one undo step.
import { expect, test, type Page } from "@playwright/test";
import { openApp, ribbonButton } from "./ui-helpers";
import { showPanel } from "./app";

test.use({ viewport: { width: 1600, height: 1000 } });

const nodes = (page: Page) => page.locator(".react-flow__node");
const part = (page: Page, label: string) => nodes(page).filter({ hasText: label }).first();
const selected = (page: Page) => page.locator(".react-flow__node.selected");
const counts = (page: Page) =>
  page.evaluate(() => ({
    parts: document.querySelectorAll(".react-flow__node").length,
    wires: document.querySelectorAll(".react-flow__edge").length,
  }));
/** Move the pointer off the diagram, onto the ribbon's empty strip. */
const pointerAway = async (page: Page) => page.mouse.move(800, 3);

test.beforeEach(async ({ page }) => {
  await openApp(page);
});

test("Ctrl+A selects every part and Delete removes them, one Ctrl+Z brings all back, pointer elsewhere", async ({
  page,
}) => {
  const before = await counts(page);
  await part(page, "Final Drive").click();
  await pointerAway(page);
  await page.keyboard.press("Control+a");
  await expect(selected(page)).toHaveCount(before.parts);
  await page.keyboard.press("Delete");
  await expect(nodes(page)).toHaveCount(0);
  await page.keyboard.press("Control+z");
  await expect.poll(() => counts(page)).toEqual(before);
});

test("Backspace deletes a multi-selection; Ctrl+X cuts it and Ctrl+V pastes it back", async ({ page }) => {
  const before = await counts(page);
  await part(page, "Final Drive").click();
  await part(page, "Wheel RL").click({ modifiers: ["Control"] });
  await pointerAway(page);
  await page.keyboard.press("Backspace");
  await expect(nodes(page)).toHaveCount(before.parts - 2);
  await page.keyboard.press("Control+z");
  await expect.poll(() => counts(page)).toEqual(before);

  await part(page, "Final Drive").click();
  await part(page, "Wheel RL").click({ modifiers: ["Control"] });
  await pointerAway(page);
  await page.keyboard.press("Control+x");
  await expect(nodes(page)).toHaveCount(before.parts - 2);
  await expect(part(page, "Final Drive")).toHaveCount(0);
  await page.keyboard.press("Control+v");
  await expect(nodes(page)).toHaveCount(before.parts);
  await expect(selected(page)).toHaveCount(2);
  await expect(part(page, "Final Drive")).toBeVisible();
  // the cut was one step: undo the paste, then the cut
  await page.keyboard.press("Control+z");
  await page.keyboard.press("Control+z");
  await expect.poll(() => counts(page)).toEqual(before);
});

test("F2 renames the selected part; Enter puts the focus in its Properties", async ({ page }) => {
  await part(page, "Final Drive").click();
  await pointerAway(page);
  await page.keyboard.press("F2");
  const input = page.locator("div.fixed", { has: page.getByText("Rename element", { exact: true }) }).locator("input");
  await expect(input).toBeFocused();
  await expect(input).toHaveValue("Final Drive");
  await page.keyboard.type("Rear Axle Drive");
  await page.keyboard.press("Enter");
  await expect(input).toHaveCount(0);
  await expect(part(page, "Rear Axle Drive")).toBeVisible();

  // the Enter that closed the dialog did not also move the focus; this one does
  await part(page, "Rear Axle Drive").click();
  await pointerAway(page);
  await page.keyboard.press("Enter");
  const name = page.locator("[data-properties-panel]").getByRole("textbox", { name: "Name", exact: true });
  await expect(name).toBeFocused();
  await expect(name).toHaveValue("Rear Axle Drive");
});

test("the keys stay with a field being typed in, and with a panel clicked last", async ({ page }) => {
  const before = await counts(page);
  await part(page, "Final Drive").click();
  await showPanel(page, "Properties");
  const name = page.locator("[data-properties-panel]").getByRole("textbox", { name: "Name", exact: true });
  await name.click();
  await page.keyboard.press("Control+a");
  await page.keyboard.press("Delete");
  await expect(name).toHaveValue("");
  await expect(selected(page)).toHaveCount(1);
  expect((await counts(page)).parts).toBe(before.parts);
  await name.fill("Final Drive");

  // a click on a panel's text takes the keyboard from the diagram
  await part(page, "Final Drive").click();
  await page.locator("[data-properties-panel] .ss-panel-toolbar").click();
  await page.keyboard.press("Control+a");
  await page.keyboard.press("Delete");
  await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
  expect((await counts(page)).parts).toBe(before.parts);
});

test("the ribbon's Delete removes the whole selection in one undo step", async ({ page }) => {
  const before = await counts(page);
  await part(page, "Final Drive").click();
  await part(page, "Wheel RL").click({ modifiers: ["Control"] });
  await part(page, "Wheel RR").click({ modifiers: ["Control"] });
  await ribbonButton(page, "Delete");
  await expect(nodes(page)).toHaveCount(before.parts - 3);
  await page.keyboard.press("Control+z");
  await expect.poll(() => counts(page)).toEqual(before);
});

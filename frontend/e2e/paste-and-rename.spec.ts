// Duplicate, paste and rename on the diagram. Duplicate and paste select the
// new parts in the canvas's own handler, and the prompt seeds its text while
// rendering; both moved when react-hooks 7 flagged the effects that did it.
import { expect, test, type Page } from "@playwright/test";
import { openApp } from "./ui-helpers";

test.use({ viewport: { width: 1600, height: 1000 } });

const nodes = (page: Page) => page.locator(".react-flow__node");
const ids = (page: Page, css: string) =>
  page.locator(css).evaluateAll((els) => els.map((e) => e.getAttribute("data-id")!));
const selectedIds = (page: Page) => ids(page, ".react-flow__node.selected");

test("duplicate and paste select exactly the new parts", async ({ page }) => {
  await openApp(page);
  const before = await ids(page, ".react-flow__node");
  await nodes(page).nth(0).click();
  await nodes(page).nth(1).click({ modifiers: ["Control"] });
  const originals = await selectedIds(page);
  expect(originals).toHaveLength(2);

  await page.keyboard.press("Control+d");
  await expect(nodes(page)).toHaveCount(before.length + 2);
  const duplicates = await selectedIds(page);
  expect(duplicates).toHaveLength(2);
  expect(duplicates.filter((id) => before.includes(id))).toEqual([]);

  // copy the duplicates and paste them over empty space
  await page.keyboard.press("Control+c");
  const pane = (await page.locator(".react-flow__pane").boundingBox())!;
  await page.mouse.move(pane.x + pane.width - 150, pane.y + 80);
  await page.keyboard.press("Control+v");
  await expect(nodes(page)).toHaveCount(before.length + 4);
  const pasted = await selectedIds(page);
  expect(pasted).toHaveLength(2);
  expect(pasted.filter((id) => before.includes(id) || duplicates.includes(id))).toEqual([]);

  // and with the context menu's Paste
  await page.mouse.click(pane.x + 30, pane.y + pane.height - 30, { button: "right" });
  await page.getByRole("button", { name: /^Paste/ }).click();
  await expect(nodes(page)).toHaveCount(before.length + 6);
  const pastedAgain = await selectedIds(page);
  expect(pastedAgain).toHaveLength(2);
  expect(pastedAgain.filter((id) => pasted.includes(id))).toEqual([]);
});

test("Rename shows each part's own name, and Escape drops the typed text", async ({ page }) => {
  await openApp(page);
  const input = page.locator("div.fixed", { has: page.getByText("Rename element", { exact: true }) }).locator("input");
  const rename = async (part: number) => {
    await nodes(page).nth(part).click({ button: "right" });
    await page.getByRole("button", { name: /^Rename/ }).click();
    await expect(input).toBeFocused();
    return input.inputValue();
  };

  const first = await rename(0);
  expect(first).not.toBe("");
  await expect(nodes(page).nth(0)).toContainText(first);
  await input.fill("typed then cancelled");
  await page.keyboard.press("Escape");
  await expect(input).toHaveCount(0);
  await expect(nodes(page).nth(0)).not.toContainText("typed then cancelled");

  const second = await rename(2);
  expect(second).not.toBe(first);
  await expect(nodes(page).nth(2)).toContainText(second);
  // the whole name is selected, so typing replaces it
  await page.keyboard.type("Renamed part");
  await page.keyboard.press("Enter");
  await expect(nodes(page).nth(2)).toContainText("Renamed part");
});

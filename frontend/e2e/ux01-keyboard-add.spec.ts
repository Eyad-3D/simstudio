// UX-01: a model can be built without dragging. Every library part can be
// added from the keyboard, and each drag has a pointer alternative
// (double-click, or click the part and then the diagram).
import { expect, test, type Page } from "@playwright/test";
import { dockTab, newProject, nodeLayout, openApp, openExample } from "./ui-helpers";

test.use({ viewport: { width: 1600, height: 1000 } });

const row = (page: Page, id: string) => page.locator(`[data-component-id='${id}']`);
const nodes = (page: Page) => page.locator(".react-flow__node");

test("UX-01: every library part can be added with the keyboard alone", async ({ page }) => {
  await openApp(page);
  const focused = () =>
    page.evaluate(() => {
      const a = document.activeElement;
      return { text: a?.textContent?.trim() ?? "", comp: a?.getAttribute("data-component-id") ?? null };
    });

  // Tab to the ribbon's New and start an empty project
  let tabs = 0;
  while ((await focused()).text !== "New" && tabs++ < 80) await page.keyboard.press("Tab");
  expect(tabs, "Tab never reached New").toBeLessThan(80);
  await page.keyboard.press("Enter");
  await expect(nodes(page)).toHaveCount(0);

  // Tab on into the library
  tabs = 0;
  while (!(await focused()).comp && tabs++ < 80) await page.keyboard.press("Tab");
  expect(tabs, "Tab never reached a library part").toBeLessThan(80);

  // Enter on every part, moving down the list with the arrow keys
  const library = await page.locator("[data-component-id]").evaluateAll((rows) =>
    rows.map((r) => r.getAttribute("data-component-id")),
  );
  expect(library.length).toBeGreaterThanOrEqual(30);
  const added: string[] = [];
  for (let guard = 0; guard < 200 && added.length < library.length; guard++) {
    const { comp } = await focused();
    if (comp && !added.includes(comp)) {
      await page.keyboard.press("Enter");
      added.push(comp);
      await expect(nodes(page)).toHaveCount(added.length);
      // the new part is selected (and so shown in Properties)
      await expect(page.locator(".react-flow__node.selected")).toHaveCount(1);
    }
    await page.keyboard.press("ArrowDown");
  }
  expect(added).toHaveLength(library.length);
  expect((await nodeLayout(page)).overlaps, "overlapping parts").toBe(0);
  await expect(page.locator("[aria-live=polite]", { hasText: /^Added / })).toHaveCount(1);
});

test("UX-01: search matches descriptions and other names, at word starts", async ({ page }) => {
  await openApp(page);
  const search = page.getByPlaceholder("Search components…");
  const names = async (q: string) => {
    await search.fill(q);
    return page.locator("[data-component-id] .truncate").allTextContents();
  };
  expect(await names("inverter")).toContain("E-Motor");
  expect(await names("ICE")).toContain("Combustion Engine");
  expect(await names("accumulator")).toContain("HV Battery Pack");
  expect(await names("hv pack")).toContain("HV Battery Pack");
  expect(await names("device")).not.toContain("Combustion Engine");
});

test("UX-01: double-click adds a part; click then click the diagram places it", async ({ page }) => {
  await openApp(page);
  const before = await nodes(page).count();
  await row(page, "mech.brake").dblclick();
  await expect(nodes(page)).toHaveCount(before + 1);

  const pane = (await page.locator(".react-flow").boundingBox())!;
  await row(page, "signal.constant").click();
  await expect(row(page, "signal.constant")).toHaveAttribute("data-placing", "true");
  await expect(page.getByRole("status").filter({ hasText: "Click the diagram to place" })).toHaveCount(1);
  const target = { x: pane.x + 60, y: pane.y + 60 }; // empty space in the top-left corner
  await page.mouse.click(target.x, target.y);
  await expect(nodes(page)).toHaveCount(before + 2);
  const placed = (await page.locator(".react-flow__node.selected").boundingBox())!;
  expect(Math.abs(placed.x - target.x)).toBeLessThan(80);
  expect(Math.abs(placed.y - target.y)).toBeLessThan(80);

  // Esc cancels an armed part
  await row(page, "signal.constant").click();
  await expect(row(page, "signal.constant")).toHaveAttribute("data-placing", "true");
  await page.keyboard.press("Escape");
  await expect(row(page, "signal.constant")).not.toHaveAttribute("data-placing");
});

test("UX-01: an armed part is dropped when another project is opened", async ({ page }) => {
  await openApp(page);
  await row(page, "signal.constant").click();
  await expect(row(page, "signal.constant")).toHaveAttribute("data-placing", "true");
  await openExample(page, "Hybrid");
  await expect(row(page, "signal.constant")).not.toHaveAttribute("data-placing");
  await expect(page.getByRole("status").filter({ hasText: "Click the diagram to place" })).toHaveCount(0);
  // so a click on the other project's empty space adds nothing
  const count = await nodes(page).count();
  const pane = (await page.locator(".react-flow").boundingBox())!;
  await page.mouse.click(pane.x + 40, pane.y + 40);
  await page.waitForTimeout(300);
  await expect(nodes(page)).toHaveCount(count);
});

test("UX-01: Enter with the diagram behind another tab shows it and adds the part", async ({ page }) => {
  await openApp(page);
  await newProject(page);
  await dockTab(page, "Monitors").click();
  await expect(page.locator(".react-flow")).toHaveCount(0); // the hidden panel leaves the page
  await row(page, "mech.brake").focus();
  await page.keyboard.press("Enter");
  await expect(nodes(page)).toHaveCount(1);
  await expect(page.locator(".react-flow__node.selected")).toHaveCount(1);
  expect((await nodeLayout(page)).offscreen, "the new part is outside the diagram's view").toBe(0);
  await expect(page.locator("[aria-live=polite]", { hasText: /^Added Brake/ })).toHaveCount(1);
  await expect(row(page, "mech.brake")).toBeFocused();
});

// UX-10: a value outside a parameter's limits turns its field red as it is
// typed, with a line that says what is allowed, and Data Checks, which read
// the same limits from the catalogue, say the same. A case's own values are
// checked too. Hovering or focusing a parameter shows a card with its help
// texts (LRN-05); the expected texts are read from the catalogue, so a
// wording change needs no change here.
import AxeBuilder from "@axe-core/playwright";
import { readFileSync } from "node:fs";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { openApp, showPanel } from "./app";

type Param = { key: string; description: string; typical: string; whereToFind: string };
const library: { components: { id: string; parameters: Param[] }[] } = JSON.parse(
  readFileSync(new URL("../src/data/componentLibrary.json", import.meta.url), "utf8"),
);
/** The Help panel's page (LRN-09). */
const helpFrame = (page: Page) => page.locator("iframe[title='Help page']");
const param = (defId: string, key: string) =>
  library.components.find((c) => c.id === defId)!.parameters.find((p) => p.key === key)!;

/** Type `text` into a number field as a user would and return how long, in
 *  ms, the field took to turn red (aria-invalid), measured in the page. */
function msToRed(field: Locator, text: string): Promise<number> {
  return field.evaluate(
    (input: HTMLInputElement, typed) =>
      new Promise<number>((resolve) => {
        const t0 = performance.now();
        const seen = () => input.getAttribute("aria-invalid") === "true" && resolve(performance.now() - t0);
        new MutationObserver(seen).observe(input, { attributes: true });
        setTimeout(() => resolve(Infinity), 2000);
        input.focus();
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, typed);
        input.dispatchEvent(new Event("input", { bubbles: true }));
      }),
    text,
  );
}

test("UX-10: a value out of range turns red with its reason as it is typed, and Data Checks agree", async ({
  page,
}) => {
  await openApp(page);
  await page.locator(".react-flow__node", { hasText: "E-Motor" }).first().click();
  await showPanel(page, "Properties");
  const field = page.getByRole("spinbutton", { name: "Generator Torque Limit Scale" });
  const reason = page.getByRole("alert").filter({ hasText: "Generator Torque Limit Scale must be" });

  const ms = await msToRed(field, "250");
  test.info().annotations.push({ type: "ms to red", description: ms.toFixed(1) });
  expect(ms, "red within 0.5 s of typing").toBeLessThan(500);
  await expect(reason).toHaveText("Generator Torque Limit Scale must be at least 0 and at most 200 %.", {
    timeout: 500,
  });
  await expect(field).toHaveAccessibleDescription(/must be at least 0 and at most 200 %/);
  // a red field still shows that it has focus
  const ring = () => field.evaluate((el) => getComputedStyle(el).boxShadow);
  const focusedRing = await ring();
  await field.blur();
  expect(focusedRing).not.toBe(await ring());

  // the value is stored as typed, so the background Data Checks say the same
  await showPanel(page, "Problems");
  await expect(
    page.getByText("Generator Torque Limit Scale of 'E-Motor' must be at least 0 and at most 200 % — got 250."),
  ).toBeVisible({ timeout: 3000 });

  await showPanel(page, "Properties");
  await field.fill("100");
  await expect(field).not.toHaveAttribute("aria-invalid");
  await expect(reason).toHaveCount(0);
  await showPanel(page, "Problems");
  await expect(page.getByText(/Generator Torque Limit Scale of 'E-Motor'/)).toHaveCount(0, { timeout: 3000 });
});

test("UX-10: a case's own value is checked as it is typed and a cleared field keeps it", async ({ page }) => {
  await openApp(page);
  await showPanel(page, "Cases & Parameters");
  // the first Element… list is the add-override form's (the second, the sweep's)
  const pick = page.locator("select", { has: page.locator("option", { hasText: "Element…" }) }).first();
  await pick.selectOption({ label: "HV Battery Pack" });
  await pick.locator("xpath=following-sibling::select[1]").selectOption({ label: "Initial SOC (%)" });
  await page.getByRole("button", { name: "Add override" }).click();

  const field = page.getByRole("spinbutton", { name: "HV Battery Pack · Initial SOC" });
  await field.fill("150");
  await expect(field).toHaveAttribute("aria-invalid", "true", { timeout: 500 });
  await expect(page.getByRole("alert").filter({ hasText: "Initial SOC must be" })).toHaveText(
    "Initial SOC must be above 0 and at most 100 %.",
  );
  await showPanel(page, "Problems");
  await expect(
    page.getByText("Initial SOC of 'HV Battery Pack' in case 'City Cycle' must be above 0 and at most 100 % — got 150."),
  ).toBeVisible({ timeout: 3000 });

  // clearing the field no longer stores 0: leaving it puts 150 back
  await showPanel(page, "Cases & Parameters");
  await field.fill("");
  await field.blur();
  await expect(field).toHaveValue("150");
  await field.fill("80");
  await expect(field).not.toHaveAttribute("aria-invalid");
  await showPanel(page, "Problems");
  await expect(page.getByText(/in case 'City Cycle'/)).toHaveCount(0, { timeout: 3000 });
});

/** The E-Motor's Properties, with the help card's locator. */
async function emotorProperties(page: Page) {
  await openApp(page);
  await page.locator(".react-flow__node", { hasText: "E-Motor" }).first().click();
  await showPanel(page, "Properties");
  const field = page.getByRole("spinbutton", { name: "Generator Torque Limit Scale" });
  return { field, row: page.locator("tr", { has: field }), card: page.locator(".ss-help-card:popover-open") };
}

test("UX-10: a help card explains a parameter on hover and on focus, and Esc closes it", async ({ page }) => {
  const q4 = param("motor.emotor", "q4_torque_scale_pct");
  const { field, row, card } = await emotorProperties(page);

  // the pointer rests on the row: the card, beside it and inside the window
  await row.hover();
  await expect(card).toContainText(q4.description, { timeout: 1000 });
  await expect(card).toContainText(`Typical: ${q4.typical}`);
  await expect(card).toContainText(`Where to find it: ${q4.whereToFind}`);
  await expect(card).toContainText("Default 100 % · allowed: at least 0 and at most 200 %");
  const [c, r] = [(await card.boundingBox())!, (await row.boundingBox())!];
  expect(c.x + c.width, "left of the Properties row").toBeLessThanOrEqual(r.x + 1);
  expect(c.y).toBeGreaterThanOrEqual(0);
  expect(c.y + c.height).toBeLessThanOrEqual(page.viewportSize()!.height);
  // it stays while the pointer moves onto it, and links to the parameter's help
  await card.hover();
  await page.waitForTimeout(400);
  // (the help opens in its panel inside the app, LRN-09)
  await card.getByRole("button", { name: "More in the help (F1)" }).click();
  await expect(helpFrame(page)).toHaveAttribute("src", /\/help\/reference\/components\/motor\.emotor\.html#q4_torque_scale_pct$/);
  await page.getByRole("button", { name: "Close the help" }).click();
  await page.mouse.move(5, 5);
  await expect(card).toHaveCount(0);

  // keyboard: focus opens it at once; Esc closes it and the field keeps focus
  await field.focus();
  await expect(card).toContainText(q4.description, { timeout: 200 });
  await expect(field).toHaveAccessibleDescription(q4.description);
  await page.keyboard.press("Escape");
  await expect(card).toHaveCount(0);
  await expect(field).toBeFocused();
  // F1 on a parameter opens that parameter's help
  await page.keyboard.press("F1");
  await expect(helpFrame(page)).toHaveAttribute("src", /motor\.emotor\.html#q4_torque_scale_pct$/);
});

test("UX-10: a click into a field keeps its card, Tab past the card closes it, and F1 opens what it shows", async ({ page }) => {
  const q4 = param("motor.emotor", "q4_torque_scale_pct");
  const { field, row, card } = await emotorProperties(page);
  // a real click: the card opens with the focus and stays when the button
  // comes up (it once closed then), and while the pointer is elsewhere
  const f = (await field.boundingBox())!;
  await page.mouse.move(f.x + f.width / 2, f.y + f.height / 2);
  await page.mouse.down();
  await page.waitForTimeout(150);
  await page.mouse.up();
  await page.mouse.move(5, 5);
  await page.waitForTimeout(400);
  await expect(field).toBeFocused();
  await expect(card).toContainText(q4.description);
  // a click on the card's text keeps it too
  const c = (await card.boundingBox())!;
  await page.mouse.click(c.x + 20, c.y + 14);
  await page.waitForTimeout(400);
  await expect(card).toHaveCount(1);
  await page.mouse.move(5, 5);

  // from the last Edit… button Tab reaches the card's button, and the next
  // Tab leaves the card closed behind it
  await page.getByRole("button", { name: /Edit…$/ }).last().focus();
  await page.keyboard.press("Tab");
  await expect(card.getByRole("button", { name: "More in the help (F1)" })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(card).toHaveCount(0);

  // F1 with a card open from hovering opens the parameter it shows
  await row.hover();
  await expect(card).toContainText(q4.description, { timeout: 1000 });
  await page.keyboard.press("F1");
  await expect(helpFrame(page)).toHaveAttribute("src", /motor\.emotor\.html#q4_torque_scale_pct$/);
  await page.getByRole("button", { name: "Close the help" }).click();
  await page.mouse.move(5, 5);

  // a dialog opened with Enter leaves the focus behind it: no card there
  await page.getByRole("button", { name: /^Drag Torque \(unpowered\).*Edit…$/ }).focus();
  await page.keyboard.press("Enter");
  await expect(page.locator(".fixed.inset-0", { hasText: "— E-Motor" })).toBeVisible();
  await page.keyboard.press("Tab");
  await page.waitForTimeout(400);
  await expect(card).toHaveCount(0);
});

test("UX-10: in the parameter dialog Esc closes the card first, and tables carry their help", async ({ page }) => {
  const drag = param("motor.emotor", "drag_torque");
  const { card } = await emotorProperties(page);
  // the Edit… button of a table has the card too; opening the dialog closes it
  const edit = page.getByRole("button", { name: /^Drag Torque \(unpowered\).*Edit…$/ });
  await edit.hover();
  await expect(card).toContainText(drag.description, { timeout: 1000 });
  await edit.click();
  const dialog = page.locator(".fixed.inset-0", { hasText: "— E-Motor" });
  await expect(dialog).toBeVisible();
  await expect(card).toHaveCount(0);
  // the table's help line sits above its editor
  await expect(dialog.locator("[data-param='drag_torque'] .ss-param-help")).toContainText(drag.whereToFind);

  await dialog.getByRole("spinbutton", { name: "Generator Torque Limit Scale" }).focus();
  await expect(card).toHaveCount(1);
  await page.keyboard.press("Escape");
  await expect(card).toHaveCount(0);
  await expect(dialog).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
});

for (const theme of ["light", "dark"] as const) {
  test(`UX-10: the help card and the reason line pass axe in the ${theme} theme`, async ({ page }) => {
    await page.addInitScript((t) => localStorage.setItem("lightsim-theme", t), theme);
    const { field, card } = await emotorProperties(page);
    await field.fill("250");
    await field.focus();
    await expect(card).toHaveCount(1);
    // dockview's 1-px separator hides the panel text's colours from axe
    await page.addStyleTag({ content: ".dv-view::before { content: none !important; }" });
    const { violations } = await new AxeBuilder({ page })
      .include(".ss-help-card")
      .include(".ss-param-problem")
      .analyze();
    const blocking = violations.filter((v) => v.impact === "serious" || v.impact === "critical");
    expect(blocking.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual([]);
  });
}

// UX-10: a value outside a parameter's limits turns its field red as it is
// typed, with a line that says what is allowed, and Data Checks, which read
// the same limits from the catalogue, say the same. A case's own values are
// checked too.
import { expect, test, type Locator } from "@playwright/test";
import { openApp, showPanel } from "./app";

/** Type `text` into a number field as a user would and return how long, in
 *  ms, the field took to turn red (aria-invalid), measured in the page. */
function msToRed(field: Locator, text: string): Promise<number> {
  return field.evaluate(
    (input: HTMLInputElement, typed) =>
      new Promise<number>((resolve) => {
        const t0 = performance.now();
        const seen = () => input.getAttribute("aria-invalid") === "true" && resolve(performance.now() - t0);
        new MutationObserver(seen).observe(input, { attributes: true });
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

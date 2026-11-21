// MOD-48: typing a tyre code fills in the wheel; MOD-08: the battery's
// Defined By lists Cells, which this version does not run (NEW-CELLS), with
// the words for it and not to be picked (the Properties panel).
import { expect, test } from "@playwright/test";
import { openApp, selectElement } from "./app";
import { importProject } from "./ui-helpers";

test("MOD-48 and MOD-08: a tyre code fills the wheel, Cells is listed as not in this version yet", async ({ page }) => {
  const n = Date.now() % 100000;
  await openApp(page);
  const example = await (await page.request.get("/api/examples/bev-car")).json();
  await importProject(page, { ...example, id: `e2e-mod48-${n}`, name: `E2E tyre ${n}` });

  await selectElement(page, "Wheel FL");
  await page.getByRole("textbox", { name: "Tyre Code" }).fill("205/55 R16 91V");
  await expect(page.getByRole("spinbutton", { name: "Wheel Radius" })).toHaveValue("0.3065");
  await expect(page.getByTestId("tyre-summary")).toContainText("load index 91");
  await page.getByRole("combobox", { name: "Rolling Resistance Label Class" }).selectOption("B");
  await expect(page.getByRole("spinbutton", { name: "Rolling Resistance" })).toHaveValue("0.0072");

  await selectElement(page, "HV Battery Pack");
  await expect(page.getByRole("spinbutton", { name: "Cells in Series (s)" })).toHaveCount(0);
  const cells = page.getByRole("combobox", { name: "Defined By" }).locator("option", { hasText: "Cells" });
  await expect(cells).toHaveText("Cells (not in this version yet)");
  await expect(cells).toBeDisabled();
});

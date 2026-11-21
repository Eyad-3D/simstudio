// MOD-48: typing a tyre code fills in the wheel; MOD-08: the battery's
// Defined By switch shows the fields of the mode chosen (the Properties
// panel).
import { expect, test } from "@playwright/test";
import { openApp, selectElement } from "./app";
import { importProject } from "./ui-helpers";

test("MOD-48 and MOD-08: a tyre code fills the wheel, Cells shows the cell fields", async ({ page }) => {
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
  await page.getByRole("combobox", { name: "Defined By" }).selectOption("Cells");
  await expect(page.getByRole("spinbutton", { name: "Cells in Series (s)" })).toHaveValue("96");
  await expect(page.getByRole("spinbutton", { name: "Series Resistance R0" })).toHaveCount(0);
});

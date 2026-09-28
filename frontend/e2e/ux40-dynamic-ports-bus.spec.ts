// UX-40: a Monitor added from the library has no ports of its own until you
// add them in Properties. The Data Bus panel lists it with a hint, its new
// port can be wired there, and the Monitors panel reads the wired signal.
import { expect, test } from "@playwright/test";
import { openApp, ribbonTab, runActiveCase, showPanel } from "./app";

test("UX-40: a new Monitor is wired in the Data Bus panel and shows its signal", async ({ page }) => {
  await openApp(page);
  await page.locator("[data-component-id='signal.monitor']").dblclick();
  await showPanel(page, "Data Bus Connections");

  // the Monitor is listed before it has a port, and says where to add one
  await expect(page.getByText(/^Monitor 3: no ports yet\. Add one in Properties\.$/)).toBeVisible();
  await showPanel(page, "Properties");
  await page.getByRole("button", { name: "input", exact: true }).click();

  await showPanel(page, "Data Bus Connections");
  const source = page.getByRole("combobox", { name: "Source of Monitor 3 · in_1" });
  await source.click();
  await page.getByRole("option", { name: "Vehicle · Vehicle Speed [km/h]" }).click();
  await expect(source).toHaveValue("Vehicle · Vehicle Speed");

  await runActiveCase(page);
  await ribbonTab(page, "Home").click();
  await showPanel(page, "Monitors");
  const title = page.getByTitle("Select this monitor").filter({ hasText: "Monitor 3" });
  const card = page.locator("div.rounded", { has: title });
  await expect(card.getByText("in_1 — Vehicle · Vehicle Speed")).toBeVisible();
  await expect(card.locator(".font-mono")).toHaveText(/^-?[\d,.]+\s*km\/h$/);
});

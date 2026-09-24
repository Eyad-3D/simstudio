// UX-40: a Monitor added from the library has no ports of its own until you
// add them in Properties. It still appears in the Data Bus lists, its new
// port can be wired there, and the Monitors panel reads the wired signal.
import { expect, test } from "@playwright/test";
import { openApp, ribbonTab, runActiveCase, showPanel } from "./app";

test("UX-40: a new Monitor is wired in the Data Bus panel and shows its signal", async ({ page }) => {
  await openApp(page);
  await page.locator("[data-component-id='signal.monitor']").dblclick();
  await showPanel(page, "Data Bus Connections");
  const connect = page.getByRole("button", { name: "Connect", exact: true });
  const bus = page.locator(".dv-content-container", { has: connect });
  const element = (label: string) =>
    bus.locator("button.ss-tree-row").filter({ has: page.getByText(label, { exact: true }) });

  // the Monitor is listed before it has a port, and says where to add one
  await element("Monitor 3").nth(1).click();
  await expect(bus.getByText("No ports yet: add one in Properties.")).toBeVisible();
  await showPanel(page, "Properties");
  await page.getByRole("button", { name: "input", exact: true }).click();

  await showPanel(page, "Data Bus Connections");
  await element("Vehicle").first().click();
  await bus.locator("table").first().locator("tr", { hasText: "Vehicle Speed" }).click();
  await bus.locator("table").nth(1).locator("tr", { hasText: "in_1" }).click();
  await connect.click();
  await expect(bus.getByText("Monitor 3", { exact: true })).toHaveCount(3); // both lists and the new link

  await runActiveCase(page);
  await ribbonTab(page, "Home").click();
  await showPanel(page, "Monitors");
  const title = page.getByTitle("Select this monitor").filter({ hasText: "Monitor 3" });
  const card = page.locator("div.rounded", { has: title });
  await expect(card.getByText("in_1 — Vehicle · Vehicle Speed")).toBeVisible();
  await expect(card.locator(".font-mono")).toHaveText(/^-?[\d,.]+\s*km\/h$/);
});

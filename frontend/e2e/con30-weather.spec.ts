// CON-30: weather presets on the Ambient, and the electric car's winter case.
import { expect, test } from "@playwright/test";
import { openApp, showPanel } from "./app";

test("CON-30: a weather preset sets the Ambient's temperature and pressure in one step", async ({ page }) => {
  await openApp(page);
  await page.locator(".react-flow__node", { hasText: "Ambient" }).first().click();
  await showPanel(page, "Properties");
  const temperature = page.getByRole("spinbutton", { name: /^Temperature/ });
  await expect(temperature).toHaveValue("20");
  await page.getByRole("button", { name: "Apply preset: High altitude (1,500 m)" }).click();
  await expect(temperature).toHaveValue("5.25");
  await expect(page.getByRole("spinbutton", { name: /^Pressure/ })).toHaveValue("84.56");
  await page.getByRole("button", { name: "Apply preset: Cold day (−7 °C)" }).click();
  await expect(temperature).toHaveValue("-7");
});

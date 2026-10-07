// CON-18: start from a pre-wired template with a short form; save a model as a template.
import { expect, test } from "@playwright/test";
import { openApp, showPanel } from "./app";

test("CON-18: a project from a template takes the form's values, and a model becomes a template", async ({ page }) => {
  await openApp(page);
  await page.getByRole("button", { name: "Templates" }).click();
  const dialog = page.getByRole("dialog", { name: "Vehicle templates" });
  await dialog.getByRole("radio", { name: /Electric car, one motor/ }).check();
  await dialog.getByRole("textbox", { name: "Project name" }).fill("Template EV");
  await dialog.getByRole("textbox", { name: "Test mass" }).fill("1650");
  await dialog.getByRole("button", { name: "Create project" }).click();
  const discard = page.getByRole("button", { name: /Don.t save/ });
  if (await discard.isVisible().catch(() => false)) await discard.click();
  await expect(dialog).toHaveCount(0);
  await page.locator(".react-flow__node", { hasText: /^Vehicle$/ }).first().click();
  await showPanel(page, "Properties");
  await expect(page.getByRole("spinbutton", { name: /^Vehicle Mass/ })).toHaveValue("1650");

  // save the open model as a template that asks for its battery
  await page.getByRole("button", { name: "Templates" }).click();
  await dialog.getByRole("tab", { name: "Save this model as a template" }).click();
  await dialog.getByRole("textbox", { name: "Template name" }).fill("Battery study");
  await dialog.getByRole("checkbox", { name: /^HV Battery Pack · Usable Capacity/ }).check();
  await dialog.getByRole("button", { name: "Save as template" }).click();
  await expect(dialog.getByRole("radio", { name: /Battery study/ })).toBeVisible();
  await dialog.getByRole("radio", { name: /Battery study/ }).check();
  await expect(dialog.getByRole("textbox", { name: "HV Battery Pack · Usable Capacity" })).toHaveValue("62");
  await dialog.getByRole("button", { name: "Delete the template Battery study" }).click();
  await expect(dialog.getByRole("radio", { name: /Battery study/ })).toHaveCount(0);
});

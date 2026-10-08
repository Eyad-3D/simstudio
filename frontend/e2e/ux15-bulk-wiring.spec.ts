// UX-15 (follow-up): Data Bus Connections → Connect several. "Matching names"
// pairs each unconnected input with the output of the same name; "One output
// to every part of a type" links one output to, say, every Brake's Brake
// Command. Both list the links before making them, and one Undo takes them
// all back.
import { expect, test } from "@playwright/test";
import { openApp, showPanel } from "./app";
import { importProject } from "./ui-helpers";

test("UX-15: matching names rebuilds the BEV's links of the same name in one step", async ({ page }) => {
  await openApp(page);
  const bev = await (await page.request.get("/api/examples/bev-car")).json();
  bev.dataBusConnections = [];
  await importProject(page, bev);
  await showPanel(page, "Data Bus Connections");
  await expect(page.getByText(/^0 links · /)).toBeVisible();

  await page.getByRole("button", { name: "Connect several…" }).click();
  const bulk = page.getByRole("region", { name: "Connect several" });
  await bulk.getByRole("radio", { name: "Matching names" }).check();
  const preview = bulk.getByRole("list", { name: "Links to make" }).getByRole("listitem");
  await expect(preview).toHaveCount(8);
  await expect(preview.filter({ hasText: "Vehicle Task · Target Speed → Driver · Target Speed" })).toHaveCount(1);
  await expect(preview.filter({ hasText: "HV Battery Pack · SOC → BMS Monitor · soc" })).toHaveCount(1);
  await expect(preview.filter({ hasText: /^Driver · Brake Command → Brake (FL|FR|RL|RR) · Brake Command$/ })).toHaveCount(4);

  await bulk.getByRole("button", { name: "Connect 8 inputs" }).click();
  await expect(page.getByText(/^8 links · /)).toBeVisible();
  await expect(bulk.getByRole("status")).toHaveText("8 links connected. One Undo takes them all back.");
  await expect(page.getByRole("combobox", { name: "Source of Brake RL · Brake Command", exact: true })).toHaveValue(
    "Driver · Brake Command [-]",
  );
  await expect(bulk.getByRole("button", { name: "Connect 0 inputs" })).toBeDisabled();

  // one Undo takes all eight back
  await page.keyboard.press("Control+z");
  await expect(page.getByText(/^0 links · /)).toBeVisible();
  await page.keyboard.press("Control+y");
  await expect(page.getByText(/^8 links · /)).toBeVisible();
});

test("UX-15: one output to every Brake, with a preview of what it replaces", async ({ page }) => {
  await openApp(page);
  await showPanel(page, "Data Bus Connections");
  await expect(page.getByText(/^13 links · /)).toBeVisible();
  await page.getByRole("button", { name: "Connect several…" }).click();
  const bulk = page.getByRole("region", { name: "Connect several" });
  await expect(bulk.getByLabel("Inputs to connect").locator("option:checked")).toHaveText(
    "every Brake · Brake Command (4)",
  );
  await expect(bulk.getByRole("status")).toHaveText("Pick the output to connect.");

  // the source box offers the outputs whose names fit first
  const box = bulk.getByRole("combobox", { name: "Output to connect to every Brake · Brake Command" });
  await box.click();
  await expect(page.getByRole("listbox").getByRole("option").first()).toHaveText("Driver · Brake Command [-]");
  await page.keyboard.type("traction command");
  await page.keyboard.press("Enter");
  await expect(box).toHaveValue("Driver · Traction Command [-]");

  // every Brake has a source already: they keep it unless asked
  const preview = bulk.getByRole("list", { name: "Links to make" }).getByRole("listitem");
  await expect(preview).toHaveCount(4);
  await expect(preview.first()).toHaveText(
    "Not Brake FL · Brake Command: keeps its source, Driver · Brake Command.",
  );
  await expect(bulk.getByRole("button", { name: "Connect 0 inputs" })).toBeDisabled();
  await bulk.getByRole("checkbox", { name: "Replace the sources they have" }).check();
  await expect(preview.first()).toHaveText(
    "Driver · Traction Command → Brake FL · Brake Command (replaces its source)",
  );
  await bulk.getByRole("button", { name: "Connect 4 inputs" }).click();
  for (const b of ["FL", "FR", "RL", "RR"])
    await expect(page.getByRole("combobox", { name: `Source of Brake ${b} · Brake Command`, exact: true })).toHaveValue(
      "Driver · Traction Command [-]",
    );
  await expect(page.getByText(/^13 links · /)).toBeVisible();

  // one Undo puts every Brake back on the brake command
  await page.keyboard.press("Control+z");
  for (const b of ["FL", "FR", "RL", "RR"])
    await expect(page.getByRole("combobox", { name: `Source of Brake ${b} · Brake Command`, exact: true })).toHaveValue(
      "Driver · Brake Command [-]",
    );
  await bulk.getByRole("button", { name: "Close" }).click();
  await expect(bulk).toHaveCount(0);
});

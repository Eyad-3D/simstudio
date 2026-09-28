// UX-15: Data Bus Connections lists one row per signal input, and a link is
// two clicks: open the input's source box, pick an output. The box offers
// outputs only, so a link between two inputs cannot be made here (the store
// refuses one made elsewhere). The BEV's 13 links took 65 clicks with the
// old four-column picker.
import { expect, test } from "@playwright/test";
import { openApp, showPanel } from "./app";
import { importProject } from "./ui-helpers";

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

test("UX-15: every signal link of the BEV is rebuilt in two clicks", async ({ page }) => {
  await openApp(page);
  await showPanel(page, "Data Bus Connections");
  const boxes = page.getByRole("combobox", { name: /^Source of / });
  await expect(page.getByText("13 links", { exact: false })).toBeVisible();

  // the links as the panel shows them: input → the output feeding it
  const links = (await boxes.evaluateAll((els) =>
    els.map((el) => [el.getAttribute("aria-label")!.replace(/^Source of /, ""), (el as HTMLInputElement).value]),
  )).filter(([, from]) => from);
  expect(links).toHaveLength(13);
  expect(links).toContainEqual(["Driver · Actual Speed", "Vehicle · Vehicle Speed"]);
  await expect(page.locator("li", { hasText: "Driver · Actual Speed" })).toContainText(/→\s*Driver · Actual Speed/);

  // remove them all, then link each input again
  const remove = page.getByTitle("Remove connection");
  while ((await remove.count()) > 0) await remove.first().click();
  await expect(page.getByText(/^0 links · /)).toBeVisible();
  const list = page.getByRole("listbox");
  let clicks = 0;
  for (const [to, from] of links) {
    await page.getByRole("combobox", { name: `Source of ${to}`, exact: true }).click();
    clicks++;
    if (to === "Driver · Actual Speed") {
      // the list offers outputs only: no input can be picked as a source
      const offered = await list.getByRole("option").allInnerTexts();
      expect(offered.length).toBeGreaterThan(0);
      for (const [input] of links) expect(offered.some((o) => o.startsWith(`${input} [`))).toBe(false);
    }
    await list.getByRole("option", { name: new RegExp(`^${escape(from)} \\[`) }).click();
    clicks++;
  }
  await expect(page.getByText(/^13 links · /)).toBeVisible();
  expect(clicks).toBeLessThanOrEqual(30);
  await expect(page.getByRole("combobox", { name: "Source of Driver · Actual Speed", exact: true })).toHaveValue(
    "Vehicle · Vehicle Speed",
  );
  await showPanel(page, "Problems");
  await expect(page.getByText("0 errors, 0 warnings", { exact: true })).toBeVisible({ timeout: 2000 });

  // search and filters
  await showPanel(page, "Data Bus Connections");
  await page.getByLabel("Search signals").fill("brake");
  await expect(boxes).toHaveCount(4);
  await page.getByLabel("Search signals").fill("");

  // a part's menu lists its signals: the E-Motor takes one and feeds one
  await page.locator(".react-flow__node", { hasText: "E-Motor" }).first().click({ button: "right" });
  await page.getByRole("button", { name: "Signals…" }).click();
  await expect(page.getByRole("checkbox", { name: "Selected part" })).toBeChecked();
  await expect(boxes).toHaveCount(2);
  await expect(page.getByRole("combobox", { name: "Source of E-Motor · Traction Command", exact: true })).toHaveValue(
    "Driver · Traction Command",
  );
  await expect(page.getByRole("combobox", { name: "Source of Vehicle Monitor · motor_torque", exact: true })).toHaveValue(
    "E-Motor · Shaft Torque",
  );
});

test("UX-15: the source box works from the keyboard", async ({ page }) => {
  await openApp(page);
  await showPanel(page, "Data Bus Connections");
  const box = page.getByRole("combobox", { name: "Source of Vehicle · Road Grade", exact: true });
  await expect(box).toHaveValue("");
  await box.focus();
  await page.keyboard.type("vehicle task");
  await expect(page.getByRole("listbox").getByRole("option")).toHaveCount(1);
  await page.keyboard.press("Enter");
  await expect(box).toHaveValue("Vehicle Task · Target Speed");
  await expect(box).toHaveAttribute("aria-expanded", "false");
  // Escape closes the list and keeps the source
  await page.keyboard.press("ArrowDown");
  await expect(box).toHaveAttribute("aria-expanded", "true");
  await page.keyboard.press("Escape");
  await expect(box).toHaveAttribute("aria-expanded", "false");
  await expect(box).toHaveValue("Vehicle Task · Target Speed");
});

test("UX-15: a link between two inputs from an older project is shown and can be removed", async ({ page }) => {
  await openApp(page);
  const bev = await (await page.request.get("/api/examples/bev-car")).json();
  bev.dataBusConnections.push({
    id: "db-old",
    element1Id: "el-driver",
    port1Id: "sig_speed_in",
    element2Id: "el-motor",
    port2Id: "sig_demand_in",
  });
  await importProject(page, bev);
  await showPanel(page, "Data Bus Connections");
  const old = page.locator("li", { hasText: "both inputs, so no data flows" });
  await expect(old).toHaveText("E-Motor · Traction Command ↔ Driver · Actual Speed: both inputs, so no data flows. Remove it.");
  // both inputs keep their own rows and sources
  await expect(page.getByRole("combobox", { name: "Source of Driver · Actual Speed", exact: true })).toHaveValue(
    "Vehicle · Vehicle Speed",
  );
  await showPanel(page, "Problems");
  await expect(page.getByText(/^How to fix: Remove it in Data Bus Connections/)).toBeVisible();
  await showPanel(page, "Data Bus Connections");
  await old.getByTitle("Remove connection").click();
  await expect(old).toHaveCount(0);
  await expect(page.getByText(/^13 links · /)).toBeVisible();
});

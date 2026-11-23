// Hiding a port and a node's terminals, on the example: the HV Bus's unused
// terminal is hidden from its pin's right-click menu and shown again from
// the part's; a wired terminal cannot be hidden or deleted; Add a port gives
// the node a sixth terminal and Delete port takes it away, each one undo step.
import { expect, test, type Page } from "@playwright/test";
import { openApp } from "./ui-helpers";

test.use({ viewport: { width: 1600, height: 1000 } });

const bus = (page: Page) => page.locator(".react-flow__node", { hasText: "HV Bus" }).first();
const pin = (page: Page, id: string) => bus(page).locator(`.react-flow__handle.source[data-handleid="${id}"]`);
const item = (page: Page, name: string) => page.getByRole("button", { name, exact: true });

test("an unused port is hidden from its menu and shown again from the part's; a wired one cannot be", async ({ page }) => {
  await openApp(page);
  await expect(pin(page, "t5")).toHaveCount(1);

  // the wired battery terminal: hiding it is offered, greyed, with the reason
  await pin(page, "t1").click({ button: "right" });
  await expect(item(page, "Hide port")).toHaveAttribute("aria-disabled", "true");
  await expect(item(page, "Hide port")).toHaveAttribute("title", /has a wire or a signal link/);
  await item(page, "Hide port").click();
  await expect(pin(page, "t1")).toHaveCount(1);
  await page.keyboard.press("Escape");

  // the unused one goes
  await pin(page, "t5").click({ button: "right" });
  await item(page, "Hide port").click();
  await expect(pin(page, "t5")).toHaveCount(0);

  // the part's menu brings it back; one Undo hides it again
  await bus(page).click({ button: "right", position: { x: 46, y: 30 } });
  await item(page, "Show hidden ports").click();
  await expect(pin(page, "t5")).toHaveCount(1);
  await page.keyboard.press("Control+z");
  await expect(pin(page, "t5")).toHaveCount(0);
  await page.keyboard.press("Control+z");
  await expect(pin(page, "t5")).toHaveCount(1);
  // with nothing hidden the item is not there
  await bus(page).click({ button: "right", position: { x: 46, y: 30 } });
  await expect(item(page, "Show hidden ports")).toHaveCount(0);
});

test("a node gets a sixth terminal from Add a port and loses it from Delete port", async ({ page }) => {
  await openApp(page);
  await bus(page).click({ button: "right", position: { x: 46, y: 30 } });
  await item(page, "Add a port").click();
  await expect(pin(page, "t6")).toHaveCount(1);
  await expect(pin(page, "t6")).toHaveAttribute("title", /^Terminal 6/);

  // a wired terminal cannot be deleted
  await pin(page, "t3").click({ button: "right" });
  await expect(item(page, "Delete port")).toHaveAttribute("aria-disabled", "true");
  await page.keyboard.press("Escape");

  await pin(page, "t6").click({ button: "right" });
  await item(page, "Delete port").click();
  await expect(pin(page, "t6")).toHaveCount(0);
  await page.keyboard.press("Control+z");
  await expect(pin(page, "t6")).toHaveCount(1);

  // a part that is not a node: Hide only
  const battery = page.locator(".react-flow__node", { hasText: "Battery" }).first();
  await battery.click({ button: "right", position: { x: 46, y: 30 } });
  await expect(item(page, "Add a port")).toHaveCount(0);
});

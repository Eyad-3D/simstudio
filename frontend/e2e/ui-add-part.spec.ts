// Adding a part on the diagram itself: a double-click on empty space (or
// Shift+A) opens a search box there, Enter places the part found at that
// spot and Esc leaves nothing; a wire dragged from a port and let go over
// empty space opens the same box with only the parts it may connect to, and
// Enter places the part wired to its fitting port, in one undo step.
import { expect, test, type Page } from "@playwright/test";
import { newProject, openApp } from "./ui-helpers";

test.use({ viewport: { width: 1600, height: 1000 } });

const nodes = (page: Page) => page.locator(".react-flow__node");
const box = (page: Page) => page.locator("[data-add-part]");
const pane = (page: Page) => page.locator(".react-flow__pane");

/** A spot on the empty diagram, `dx`/`dy` from its middle, on the screen. */
async function spot(page: Page, dx = 0, dy = 0) {
  const b = (await pane(page).boundingBox())!;
  return { x: b.x + b.width / 2 + dx, y: b.y + b.height / 2 + dy };
}

test("a part added by typing on empty space goes where the box opened; Esc leaves nothing", async ({ page }) => {
  await openApp(page);
  await newProject(page);

  // Esc: the box goes, nothing is added
  const a = await spot(page, -200, 0);
  await page.mouse.dblclick(a.x, a.y);
  await expect(box(page)).toBeVisible();
  await page.keyboard.type("gear");
  await page.keyboard.press("Escape");
  await expect(box(page)).toHaveCount(0);
  await expect(nodes(page)).toHaveCount(0);

  // typing filters as the Components panel's search does; Enter places it there
  await page.mouse.dblclick(a.x, a.y);
  await page.keyboard.type("transmission");
  await expect(box(page).getByRole("option")).toHaveText(["Gearbox"]);
  await page.keyboard.press("Enter");
  await expect(box(page)).toHaveCount(0);
  await expect(nodes(page)).toHaveCount(1);
  const n = (await nodes(page).first().boundingBox())!;
  expect(Math.abs(n.x + n.width / 2 - a.x)).toBeLessThan(60);
  expect(Math.abs(n.y + n.height / 2 - a.y)).toBeLessThan(60);
  await expect(nodes(page).first()).toHaveClass(/selected/);

  // Shift+A opens the box at the pointer
  const b = await spot(page, 200, 0);
  await page.mouse.move(b.x, b.y);
  await page.mouse.click(b.x, b.y);
  await page.keyboard.press("Shift+A");
  await expect(box(page)).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(nodes(page)).toHaveCount(1);
});

test("a wire let go on empty space adds a part that fits, wired, in one undo step", async ({ page }) => {
  await openApp(page);
  await newProject(page);
  const a = await spot(page, -250, 0);
  await page.mouse.dblclick(a.x, a.y);
  await page.keyboard.type("HV Battery");
  await page.keyboard.press("Enter");
  await expect(nodes(page)).toHaveCount(1);

  // drag from the battery's negative terminal to empty space
  const pin = (await page.locator('.react-flow__node .react-flow__handle[data-handleid="neg"]').first().boundingBox())!;
  const b = await spot(page, 150, 60);
  await page.mouse.move(pin.x + pin.width / 2, pin.y + pin.height / 2);
  await page.mouse.down();
  await page.mouse.move(b.x, b.y, { steps: 12 });
  await page.mouse.up();
  await expect(box(page)).toBeVisible();
  // only parts with an electrical port: no wheel, no gearbox
  const options = box(page).getByRole("option");
  await expect(options.filter({ hasText: "E-Motor" })).toHaveCount(1);
  await expect(options.filter({ hasText: /^Wheel/ })).toHaveCount(0);
  await expect(options.filter({ hasText: /^Gearbox/ })).toHaveCount(0);

  // Esc: no part and no wire
  await page.keyboard.press("Escape");
  await expect(nodes(page)).toHaveCount(1);
  await expect(page.locator(".react-flow__edge")).toHaveCount(0);

  // again, and Enter: the motor, wired to its own negative terminal
  await page.mouse.move(pin.x + pin.width / 2, pin.y + pin.height / 2);
  await page.mouse.down();
  await page.mouse.move(b.x, b.y, { steps: 12 });
  await page.mouse.up();
  await page.keyboard.type("e-motor");
  await expect(options).toHaveText(["E-MotorNegative Terminal (−)"]);
  await page.keyboard.press("Enter");
  await expect(nodes(page)).toHaveCount(2);
  await expect(page.locator(".react-flow__edge")).toHaveCount(1);

  // one undo step takes both away
  await pane(page).click({ position: { x: 20, y: 400 } });
  await page.keyboard.press("Control+z");
  await expect(nodes(page)).toHaveCount(1);
  await expect(page.locator(".react-flow__edge")).toHaveCount(0);
});

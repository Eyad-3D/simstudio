// Layout fixes found by using the app (app-pass report): a value's help card
// never lies over its row's error (#12), a choice box shows its whole choice
// (#16), and a parameter's whole name shows in its part's window (#26;
// the docked panel keeps one line, GUI-34).
import { expect, test, type Locator } from "@playwright/test";
import { openApp, openParams, selectElement } from "./app";

/** Whether the element at the centre of `l` is `l` itself or inside it (nothing lies over it). */
const onTop = (l: Locator) =>
  l.evaluate((el) => {
    const r = el.getBoundingClientRect();
    const top = el.ownerDocument.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
    return !!top && (top === el || el.contains(top));
  });

test("#12: a value typed out of its limits shows its error, with no help card over it or the rows below", async ({ page }) => {
  await openApp(page);
  const win = await openParams(page, "E-Motor");
  const field = win.locator('tr[data-param="torque_scale_pct"] input');
  await field.click();
  await field.fill("-50");
  await field.press("Enter");
  await win.mouse.move(5, 5);
  const error = win.getByRole("alert").filter({ hasText: "Torque Scale must be above 0" });
  await expect(error).toBeVisible();
  await expect(win.locator(".ss-help-card:popover-open")).toHaveCount(0);
  expect(await onTop(error)).toBe(true);
  // the names of the rows below stay readable
  expect(await onTop(win.locator('tr[data-param="speed_scale_pct"] td').first())).toBe(true);
});

test("#16: a choice box is wide enough for its longest choice", async ({ page }) => {
  await openApp(page);
  const win = await openParams(page, "Differential");
  const type = win.getByRole("combobox", { name: "Type", exact: true });
  await type.selectOption("Limited-slip");
  // the box is at least as wide as its longest choice's text (and its arrow)
  const fits = await type.evaluate((el: HTMLSelectElement) => {
    const ctx = document.createElement("canvas").getContext("2d")!;
    const s = getComputedStyle(el);
    ctx.font = `${s.fontWeight} ${s.fontSize} ${s.fontFamily}`;
    const longest = Math.max(...[...el.options].map((o) => ctx.measureText(o.text).width));
    return el.getBoundingClientRect().width >= longest + 16;
  });
  expect(fits).toBe(true);
});

test.describe("at 1366 x 768", () => {
  test.use({ viewport: { width: 1366, height: 768 } });

  test("#26: the docked panel keeps a name on one line (GUI-34); the part's window shows it whole", async ({ page }) => {
    await openApp(page);
    await selectElement(page, "E-Motor");
    // docked: one line, cut with "…", the whole name in the help card or on hover
    const docked = page.locator('[data-properties-panel] tr[data-param="max_speed_rpm"] td').first().locator("span").first();
    await expect(docked).toHaveText("Maximum Speed (0 = full-load curve's last point)");
    expect(await docked.evaluate((el) => el.getBoundingClientRect().height < 1.5 * parseFloat(getComputedStyle(el).lineHeight))).toBe(true);
    const port = page.locator("[data-properties-panel]").getByTitle("Positive Terminal (+)");
    await expect(port).toHaveText("Positive Terminal (+)");
    // the window, which has the room: the whole name, nothing cut
    const win = await openParams(page, "E-Motor");
    const whole = win.locator('tr[data-param="max_speed_rpm"] td').first().locator("span").first();
    expect(await whole.evaluate((el) => el.scrollWidth <= el.clientWidth + 1 && el.scrollHeight <= el.clientHeight + 1)).toBe(true);
  });
});

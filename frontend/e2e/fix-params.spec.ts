// Layout fixes found by using the app (app-pass report): a value's help card
// never lies over its row's error (#12), a choice box shows its whole choice
// (#16), and Properties at 1366 wide shows a parameter's whole name (#26).
import { expect, test, type Locator } from "@playwright/test";
import { openApp, openFromMenu, openParams, selectElement, showPanel } from "./app";

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

  test("#26: Properties shows a parameter's whole name, so its value cannot mislead", async ({ page }) => {
    await openApp(page);
    await selectElement(page, "E-Motor");
    const name = page.locator('[data-properties-panel] tr[data-param="max_speed_rpm"] td').first().locator("span").first();
    await expect(name).toHaveText("Maximum Speed (0 = full-load curve's last point)");
    // all of it on screen: wrapped, not cut
    expect(await name.evaluate((el) => el.scrollWidth <= el.clientWidth + 1 && el.scrollHeight <= el.clientHeight + 1)).toBe(true);
    // a port's name is readable in full, or on hover
    const port = page.locator("[data-properties-panel]").getByTitle("Positive Terminal (+)");
    await expect(port).toHaveText("Positive Terminal (+)");
  });

  test("#26: in the docked panel no name breaks inside a word, for every part of the four examples", async ({ page }) => {
    test.setTimeout(300_000);
    await openApp(page);
    let badged = 0;
    const broken: string[] = [];
    for (const example of ["Efficient Electric Sedan", "Battery Electric Car", "FS Electric (generic)", "P2 Hybrid Car"]) {
      await openFromMenu(page, example);
      await showPanel(page, "Elements");
      const rows = page.locator("button.ss-tree-row");
      const n = await rows.count();
      for (let i = 0; i < n; i++) {
        await showPanel(page, "Elements");
        await rows.nth(i).click();
        await showPanel(page, "Properties");
        const found = await page.evaluate(() => {
          const panel = document.querySelector("[data-properties-panel]");
          if (!panel) return { bad: [] as string[], badges: 0 };
          panel.querySelectorAll("details").forEach((d) => ((d as HTMLDetailsElement).open = true));
          const bad: string[] = [];
          for (const el of panel.querySelectorAll<HTMLElement>(".ss-param-name")) {
            const text = el.firstChild;
            if (!text || text.nodeType !== Node.TEXT_NODE) continue;
            const s = text.textContent ?? "";
            // the room the name has: its whole cell (not the name itself, which a
            // badge beside it could squeeze)
            const box = el.parentElement!;
            const pad = getComputedStyle(box);
            const cell = box.clientWidth - parseFloat(pad.paddingLeft) - parseFloat(pad.paddingRight);
            const range = document.createRange();
            // a word, or the part of one up to its hyphen or slash (a line may end there)
            for (const m of s.matchAll(/[^\s\-–/]+[-–/]?/g)) {
              range.setStart(text, m.index!);
              range.setEnd(text, m.index! + m[0].length);
              const boxes = [...range.getClientRects()].filter((r) => r.width > 0);
              const lines = new Set(boxes.map((r) => Math.round(r.top)));
              const wide = boxes.reduce((w, r) => w + r.width, 0);
              // a word wider than the whole cell may break; no other
              if (lines.size > 1 && wide <= cell + 1) bad.push(`${s} → "${m[0]}"`);
            }
          }
          return { bad, badges: panel.querySelectorAll('[data-testid="source-badge"]').length };
        });
        badged += found.badges;
        broken.push(...found.bad.map((b) => `${example}: ${b}`));
      }
    }
    expect(badged).toBeGreaterThan(0); // parts with value-source badges were among them
    expect(broken).toEqual([]);
  });
});

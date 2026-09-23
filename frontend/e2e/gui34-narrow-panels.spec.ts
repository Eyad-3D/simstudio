// GUI-34: on a 1366-px laptop the side panels show numbers in full and keep
// their labels on one line. Properties showed the Driver's I Gain 0.08 as
// "0.0" and wrapped "Generator Torque Limit Scale" onto 4 lines; Cases &
// Parameters wrapped "Store every (steps)" onto 3 lines and "Run case" onto
// 2, pushing Stop out of the panel; and the side tabs read "Eleme" and
// "Cases & Parame".
import { expect, test, type Page } from "@playwright/test";
import { openApp, selectElement, showPanel } from "./app";

test.use({ viewport: { width: 1366, height: 768 } });

/** What does not fit in the right-hand panel on show: a field whose value is
 *  wider than the field, a control sticking out of the panel, and a label,
 *  table cell or button whose text takes more than one line. */
function misfits(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const group = [...document.querySelectorAll(".dv-groupview")].find((g) =>
      g.querySelector("[aria-label=Properties]"),
    )!;
    const content = group.querySelector(".dv-content-container")!;
    const panel = content.getBoundingClientRect();
    const shown = [...content.querySelectorAll("*")].filter((el) => el.getClientRects().length > 0);
    // the most lines one run of text in `el` wraps onto (a line per row of boxes)
    const lines = (el: Element) => {
      let most = 0;
      const walk = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
      for (let node = walk.nextNode(); node; node = walk.nextNode()) {
        const range = document.createRange();
        range.selectNodeContents(node);
        const rows = new Set([...range.getClientRects()].filter((r) => r.width > 0).map((r) => Math.round(r.top)));
        most = Math.max(most, rows.size);
      }
      return most;
    };
    const out: string[] = [];
    for (const el of shown) {
      const name = (el as HTMLInputElement).value || el.textContent!.trim();
      if (el.matches("input:not([type=checkbox])") && el.scrollWidth > el.clientWidth + 1)
        out.push(`"${name}" is clipped (${el.scrollWidth} px in a ${el.clientWidth} px field)`);
      const box = el.getBoundingClientRect();
      if (el.matches("input, select, button") && (box.left < panel.left - 1 || box.right > panel.right + 1))
        out.push(`"${name}" sticks out of the panel`);
      if (el.matches("label, td, th, button") && lines(el) > 1) out.push(`"${name}" takes ${lines(el)} lines`);
    }
    return out;
  });
}

/** Dock tabs not wholly in their tab strip (scrolled out of sight). */
function hiddenTabs(page: Page): Promise<string[]> {
  return page.evaluate(() =>
    [...document.querySelectorAll(".dv-tabs-container")].flatMap((strip) => {
      const box = strip.getBoundingClientRect();
      return [...strip.querySelectorAll(".dv-tab")]
        .filter((tab) => {
          const r = tab.getBoundingClientRect();
          return r.left < box.left - 1 || r.right > box.right + 1;
        })
        .map((tab) => tab.textContent!);
    }),
  );
}

// soft checks, so a failure lists what misfits on every screen, not the first
for (const theme of ["light", "dark"] as const) {
  test(`GUI-34: side panels fit a 1366x768 window (${theme})`, async ({ page }) => {
    await page.addInitScript((t) => localStorage.setItem("lightsim-theme", t), theme);
    await openApp(page);
    expect.soft(await hiddenTabs(page), "tabs out of sight").toEqual([]);

    await selectElement(page, "Driver");
    expect.soft(await misfits(page), "Driver's properties").toEqual([]);
    // a value with more digits than the field had room for is shown in full
    await page.locator("tr", { hasText: "I Gain" }).locator("input").fill("1234.5678");
    expect.soft(await misfits(page), "Driver's properties, a long value").toEqual([]);

    await selectElement(page, "E-Motor");
    expect.soft(await misfits(page), "E-Motor's properties").toEqual([]);

    await showPanel(page, "Cases & Parameters");
    expect.soft(await misfits(page), "Cases & Parameters").toEqual([]);
    expect.soft(await hiddenTabs(page), "tabs out of sight").toEqual([]);
  });
}

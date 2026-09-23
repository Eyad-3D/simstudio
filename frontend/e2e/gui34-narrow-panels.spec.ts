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
 *  wider than the field, a control sticking out of the panel, something
 *  sticking out of its table cell, a parameter label squeezed below 72 px, a
 *  label, table cell, button or span whose text takes more than one line, and
 *  a panel that scrolls sideways. */
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
      const cell = el.parentElement?.closest("td, th")?.getBoundingClientRect();
      if (cell && (box.left < cell.left - 1 || box.right > cell.right + 1)) out.push(`"${name}" sticks out of its cell`);
      if (el.matches("td:first-child") && box.width < 71) out.push(`"${name}" is squeezed to ${box.width} px`);
      if (el.matches("label, td, th, button, span") && lines(el) > 1) out.push(`"${name}" takes ${lines(el)} lines`);
      if (/auto|scroll/.test(getComputedStyle(el).overflowX) && el.scrollWidth > el.clientWidth + 1)
        out.push(`the panel scrolls sideways (${el.scrollWidth} px in ${el.clientWidth} px)`);
    }
    return out;
  });
}

/** Dock tabs not wholly in their tab strip (scrolled out of sight) or whose
 *  title is cut short. */
function unreadableTabs(page: Page): Promise<string[]> {
  return page.evaluate(() =>
    [...document.querySelectorAll(".dv-tabs-container")].flatMap((strip) => {
      const box = strip.getBoundingClientRect();
      return [...strip.querySelectorAll(".dv-tab")]
        .filter((tab) => {
          const r = tab.getBoundingClientRect();
          const title = tab.querySelector(".dv-default-tab-content")!;
          return r.left < box.left - 1 || r.right > box.right + 1 || title.scrollWidth > title.clientWidth + 1;
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
    expect.soft(await unreadableTabs(page), "tabs out of sight or cut short").toEqual([]);

    await selectElement(page, "Driver");
    expect.soft(await misfits(page), "Driver's properties").toEqual([]);
    // a value too long for the panel scrolls inside its field; the labels keep
    // their room and the panel does not scroll sideways
    await page.locator("tr", { hasText: "I Gain" }).locator("input").fill("0.30000000000000004");
    const longValue = (await misfits(page)).filter((m) => !m.includes("is clipped"));
    expect.soft(longValue, "Driver's properties, a 17-digit value").toEqual([]);

    await selectElement(page, "E-Motor");
    expect.soft(await misfits(page), "E-Motor's properties").toEqual([]);
    // a value with more digits than the field had room for is shown in full
    await page.locator("tr", { hasText: "Rotor Inertia" }).locator("input").fill("1234.5678");
    expect.soft(await misfits(page), "E-Motor's properties, a long value").toEqual([]);

    await showPanel(page, "Cases & Parameters");
    expect.soft(await misfits(page), "Cases & Parameters").toEqual([]);
    expect.soft(await unreadableTabs(page), "tabs out of sight or cut short").toEqual([]);

    // during a paced run the Type row gains a LIVE badge and structural
    // parameters a "(next run)" hint
    await page.locator("label", { hasText: "Pacing" }).locator("select").selectOption("1");
    await page.getByRole("button", { name: "Run case" }).click();
    await selectElement(page, "E-Motor");
    await expect(page.getByText("LIVE", { exact: true })).toBeVisible();
    await expect(page.getByText("(next run)").first()).toBeVisible();
    expect.soft(await misfits(page), "E-Motor's properties during a run").toEqual([]);
    await page.getByTitle("Stop the running simulation", { exact: true }).click();
  });
}

// A part's parameter window, easier to read (U-4.9, U-4.10): its sections
// (the rarely used last one folded), one search field and one Changed only
// box at the top, a dot on each value that is not the library's (a click puts
// the library's back), and a line where the active case runs with its own
// value, which goes to that value in Cases & Parameters.
import { expect, test, type Page } from "@playwright/test";
import { openApp, openParams, selectElement, showPanel } from "./app";

const section = (win: Page, name: string) => win.locator("details.ss-param-section", { has: win.locator("summary", { hasText: name }) });
const rows = (win: Page) => win.locator("tr[data-param]");

test("the battery's window: sections, a search, Changed only and a reset", async ({ page }) => {
  await openApp(page);
  const win = await openParams(page, "HV Battery Pack");

  // headings, the last one folded with its heading in view
  await expect(win.locator("details.ss-param-section > summary")).toHaveText([
    "Capacity and charge",
    "Voltage and resistance",
    "Limits",
    "Advanced",
  ]);
  await expect(section(win, "Advanced")).not.toHaveAttribute("open");
  await expect(win.getByRole("spinbutton", { name: "Charger Efficiency" })).toBeHidden();

  // the search filters as it is typed, opening the section with a match
  const search = win.getByRole("searchbox", { name: "Search parameters" });
  await search.fill("charger");
  await expect(rows(win)).toHaveCount(1);
  await expect(win.getByRole("spinbutton", { name: "Charger Efficiency" })).toBeVisible();
  await search.fill("no such words");
  await expect(win.getByRole("status")).toHaveText("No parameter matches “no such words”.");
  await search.fill("");
  await expect(section(win, "Advanced")).not.toHaveAttribute("open");

  // Changed only: the example's own capacity and minimum charge
  const changed = win.getByRole("checkbox", { name: "Changed only" });
  await changed.check();
  await expect(win.locator("tr[data-param='capacity_kWh']")).toBeVisible();
  await expect(win.locator("tr[data-param='min_soc_pct']")).toBeVisible();
  await expect(win.locator("tr[data-param='initial_soc_pct']")).toHaveCount(0);

  // the dot puts the library's value back, and the main window's Properties follow
  await selectElement(page, "HV Battery Pack");
  await win.getByRole("button", { name: "Put Minimum SOC back to the library's 10 %" }).click();
  await expect(win.locator("tr[data-param='min_soc_pct']")).toHaveCount(0); // (no longer changed)
  await changed.uncheck();
  await expect(win.getByRole("spinbutton", { name: "Minimum SOC" })).toHaveValue("10");
  await expect(page.getByRole("spinbutton", { name: "Minimum SOC" })).toHaveValue("10");
  await expect(win.getByRole("button", { name: /^Put Minimum SOC back/ })).toHaveCount(0);
  // one undo step brings the example's value back
  await win.locator("h1").click();
  await win.keyboard.press("Control+z");
  await expect(win.getByRole("spinbutton", { name: "Minimum SOC" })).toHaveValue("4");

  // the folded section, opened, stays open in the next window
  await section(win, "Advanced").locator("summary").click();
  await expect(section(win, "Advanced")).toHaveAttribute("open");
  await win.close();
  const again = await openParams(page, "HV Battery Pack");
  await expect(section(again, "Advanced")).toHaveAttribute("open");
});

test("the active case's own value shows in the part's window and leads to Cases & Parameters", async ({ page }) => {
  await openApp(page);
  // a case that drives the WLTC instead of the task's own cycle
  await page.getByTitle("Active simulation case").selectOption("case-wltc");
  const win = await openParams(page, "Vehicle Task");
  const line = win.getByRole("button", { name: /^Case “WLTC Class 3b” runs with .+, not this part's own value\.$/ });
  await expect(line).toBeVisible();
  // another case without its own value: no line
  await page.getByTitle("Active simulation case").selectOption("case-city");
  await expect(line).toHaveCount(0);
  await page.getByTitle("Active simulation case").selectOption("case-wltc");
  await line.click();
  // the value, in Cases & Parameters in the main window
  await showPanel(page, "Cases & Parameters");
  await expect(page.locator("[data-override='el-task:cycle']")).toBeInViewport();
});

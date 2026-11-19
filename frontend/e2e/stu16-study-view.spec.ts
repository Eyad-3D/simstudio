// STU-17: the sweep form runs a typed list of values or log steps, and says
// how far a running sweep is. STU-16: the Study view charts each chosen
// result of a saved study against the swept value; it reads the study kept
// with the runs, so it still draws after a reload and after the runs are
// cleared from the history.
import { expect, test, type Page } from "@playwright/test";
import { finishNotice, openApp, ribbonTab, showPanel } from "./app";
import { importProject } from "./ui-helpers";

const studyCharts = (page: Page) => page.getByRole("img", { name: /^Study chart: \S/ }).filter({ visible: true });

test("STU-17: log steps and a typed list of values", async ({ page }) => {
  await openApp(page);
  await showPanel(page, "Cases & Parameters");
  const sweepElement = page.locator("select", { has: page.locator("option", { hasText: "Element…" }) }).nth(1);
  await sweepElement.selectOption({ label: "Vehicle" });
  const values = page.getByLabel("Values the sweep runs");

  await page.getByLabel("Sweep values").selectOption({ label: "Log steps" });
  await page.getByLabel("From", { exact: true }).fill("10");
  await page.getByLabel("To", { exact: true }).fill("1000");
  await page.getByLabel("Steps", { exact: true }).fill("3");
  await expect(values).toHaveText("10 kg100 kg1000 kg");
  await page.getByLabel("From", { exact: true }).fill("0");
  await expect(page.getByRole("alert").filter({ hasText: "Log steps need both ends above 0." })).toBeVisible();
  await expect(page.getByRole("button", { name: /^Run sweep/ })).toBeDisabled();

  // a list starts from the values the range gave, and runs in the order typed
  await page.getByLabel("From", { exact: true }).fill("10");
  await page.getByLabel("Sweep values").selectOption({ label: "List of values" });
  await expect(page.getByLabel("Values to run")).toHaveValue("10, 100, 1000");
  await page.getByLabel("Values to run").fill("1500, 1200; 1800 1200");
  await expect(values).toHaveText("1500 kg1200 kg1800 kg");
  await expect(page.getByText("1 repeated value left out.")).toBeVisible();
  await expect(page.getByRole("button", { name: "Run sweep (3)" })).toBeEnabled();
  await page.getByLabel("Values to run").fill("1500, 1,5e3x");
  await expect(page.getByRole("alert").filter({ hasText: "'5e3x' is not a number" })).toBeVisible();
  // the parameter's limits are said, not refused
  await page.getByLabel("Values to run").fill("1500, 0");
  await expect(page.getByText("Vehicle Mass must be above 0 kg: 0 is not.")).toBeVisible();
});

test("STU-16: a list sweep's study charts each result against the swept value, also after a reload and a cleared history", async ({
  page,
}) => {
  test.setTimeout(150_000);
  const n = Date.now() % 100000;
  const id = `e2e-stu16-${n}`;
  await openApp(page);
  const example = await (await page.request.get("/api/examples/bev-car")).json();
  example.cases[0].duration = 120; // over 100 m: a Consumption figure
  await importProject(page, { ...example, id, name: `E2E study view ${n}` });
  await ribbonTab(page, "Home").click();
  await showPanel(page, "Cases & Parameters");
  const sweepElement = page.locator("select", { has: page.locator("option", { hasText: "Element…" }) }).nth(1);
  await sweepElement.selectOption({ label: "Vehicle" });
  await page.getByLabel("Sweep values").selectOption({ label: "List of values" });
  await page.getByLabel("Values to run").fill("1500, 1200, 1800");
  await page.getByRole("button", { name: "Run sweep (3)" }).click();

  // its progress, in the form and the status bar, until the notice
  await expect(page.getByText(/^sweep: \d of 3 points done · /)).toBeVisible();
  const notice = finishNotice(page);
  await expect(notice).toContainText("Sweep finished: 3 of 3 points complete.", { timeout: 90_000 });
  await expect(page.getByText(/points done/)).toHaveCount(0);
  await notice.getByRole("button", { name: "Study charts" }).click();

  // the Study view: a chart per headline figure, against the swept mass
  await expect(page.getByRole("button", { name: "Study", exact: true })).toHaveAttribute("aria-pressed", "true");
  const charts = studyCharts(page);
  await expect(charts.first()).toBeVisible();
  const count = await charts.count();
  expect(count).toBeGreaterThanOrEqual(2);
  const consumption = page.getByRole("img", { name: /^Study chart: Consumption;/ });
  await expect(consumption).toHaveAccessibleName(/Vehicle Mass \[kg\] 1,200 to 1,800$/);
  await expect(page.getByText("3 of 3 points complete")).toBeVisible();

  // another figure from the study's columns
  await page.getByRole("button", { name: /^Figures \(\d+\)$/ }).click();
  await page.getByRole("group", { name: "Figures to chart" }).getByRole("checkbox", { checked: false }).first().check();
  await page.keyboard.press("Escape");
  await expect(charts).toHaveCount(count + 1);

  // after a reload: the view, its figures and the study come back
  await page.reload();
  await ribbonTab(page, "Results").click();
  await expect(studyCharts(page)).toHaveCount(count + 1);

  // the runs cleared from the history: the saved study still draws
  await page.getByRole("button", { name: "Clear history" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Clear history" }).click();
  await expect(page.getByText("No results yet — run a simulation case first.")).toBeVisible();
  await expect(studyCharts(page)).toHaveCount(count + 1);
  await expect(page.getByRole("combobox", { name: "Study" })).toContainText("Vehicle · Vehicle Mass");
});

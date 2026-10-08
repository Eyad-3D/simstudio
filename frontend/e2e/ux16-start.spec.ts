// UX-16: the Start page at launch and on New, and the empty states' next
// steps. These tests start as a new user (openApp starts as a returning one,
// who skips the Start page).
import { expect, test, type Page } from "@playwright/test";
import { expectProject, ribbonTab, runButton, showPanel } from "./app";

const startHeading = (page: Page) => page.getByRole("heading", { name: "Start", exact: true });
const nodes = (page: Page) => page.locator(".react-flow__node");
const projectName = (page: Page) => page.getByText("Project name").locator("xpath=following-sibling::input");

test("UX-16: a first launch shows Start, and an example runs in two clicks", async ({ page }) => {
  await page.goto("/");
  await expect(startHeading(page)).toBeVisible();
  const card = page.getByRole("button", { name: /^Battery Electric Car, 24 parts · opens as a copy$/ });
  // its "what to expect" is its description, not part of its name
  await expect(card).toHaveAccessibleDescription(/What to expect/);
  await expect(card.locator("svg rect")).toHaveCount(24); // the sketch of its diagram (with its Ambient, CON-30, and Climate Control, MOD-41)
  // the Formula Student example is listed too, with its own results to expect
  await expect(
    page.getByRole("button", { name: /^FS Electric \(generic\), \d+ parts · opens as a copy$/ }),
  ).toHaveAccessibleDescription(/What to expect \(estimates\)/);
  await card.click(); // 1
  await expect(startHeading(page)).toHaveCount(0);
  await expect(nodes(page).first()).toBeVisible();
  await runButton(page).click(); // 2
  await expect(page.getByPlaceholder("Search channels…")).toBeVisible({ timeout: 60_000 });
});

test("UX-16: New opens Start; an example is ready to run in two clicks; Blank offers the next steps", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: /^Continue with 'Battery Electric Car'/ }).click();
  await expect(page.getByRole("button", { name: "Open", exact: true })).toBeVisible();

  await page.getByRole("button", { name: "New", exact: true }).click(); // 1
  await expect(startHeading(page)).toBeVisible();
  await expect(startHeading(page)).toBeFocused(); // the keyboard starts on the page
  await page.getByRole("button", { name: /^P2 Hybrid Car,/ }).click(); // 2
  await expect(nodes(page).first()).toBeVisible();
  await expectProject(page, "P2 Hybrid Car", { unsaved: false });
  await expect(runButton(page)).toBeEnabled();

  await page.getByRole("button", { name: "New", exact: true }).click();
  await page.getByRole("button", { name: /^Blank project/ }).click();
  await expect(nodes(page)).toHaveCount(0);
  await page.getByRole("button", { name: "Add a part", exact: true }).click();
  await expect(page.getByPlaceholder("Search components…")).toBeFocused();
  await page.getByRole("button", { name: "Start from an example", exact: true }).click();
  await expect(startHeading(page)).toBeVisible();
});

test("UX-16: the status bar's + shows the blank project, and a part dropped on its buttons lands", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "New project", exact: true }).click();
  await expect(startHeading(page)).toHaveCount(0);
  const add = page.getByRole("button", { name: "Add a part", exact: true });
  await expect(add).toBeVisible();
  await page.locator("[data-component-id='motor.emotor']").dragTo(add);
  await expect(nodes(page)).toHaveCount(1);
});

test("UX-16: a saved project is listed under Recent with its date and size", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: /^Continue with/ }).click();
  const name = `E2E recent ${Date.now() % 100000}`;
  await ribbonTab(page, "Project").click();
  await projectName(page).fill(name);
  await ribbonTab(page, "Home").click();
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expectProject(page, name, { unsaved: false });
  await nodes(page).first().click();
  await ribbonTab(page, "Start").click();
  await page.keyboard.press("Delete"); // the hidden diagram keeps its selected part
  const card = page.getByRole("region", { name: "Recent projects" }).getByRole("button", { name: new RegExp(`^${name},`) });
  await expect(card).toContainText(/Saved .+ · 24 parts/);
  await expect(card.locator("svg rect")).toHaveCount(24);

  // it reopens from there, asking first about unsaved work
  await page.getByRole("button", { name: /^Continue with/ }).click();
  await expect(nodes(page)).toHaveCount(24);
  await ribbonTab(page, "Project").click();
  await projectName(page).fill(`${name} edited`);
  await ribbonTab(page, "Start").click();
  await card.click();
  await page.getByRole("button", { name: "Don't save", exact: true }).click();
  await expectProject(page, name, { unsaved: false });
});

test("UX-16: 'Skip this page', and unsaved work, open the model at start-up", async ({ page }) => {
  await page.goto("/");
  await page.getByLabel("Skip this page: open my last project at start-up").check();
  await page.reload();
  await expect(nodes(page).first()).toBeVisible();
  await expect(startHeading(page)).toHaveCount(0);
  await ribbonTab(page, "Start").click();
  await expect(page.getByLabel("Skip this page: open my last project at start-up")).toBeChecked();
  await page.getByLabel("Skip this page: open my last project at start-up").uncheck();
  await page.reload();
  await expect(startHeading(page)).toBeVisible();

  // work not saved yet is shown at once, not behind the Start page
  await page.getByRole("button", { name: /^Continue with/ }).click();
  await ribbonTab(page, "Project").click();
  await projectName(page).fill("Unsaved edit");
  await expect
    .poll(() => page.evaluate(() => JSON.parse(localStorage.getItem("lightsim-draft-v1") ?? "{}").clean))
    .toBe(false);
  page.on("dialog", (d) => d.accept()); // the browser's leave-page prompt
  await page.reload();
  await expectProject(page, "Unsaved edit", { unsaved: true });
  await expect(startHeading(page)).toHaveCount(0);
});

test("UX-16: the Monitors panel's empty state places a Monitor", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: /^Blank project/ }).click();
  await showPanel(page, "Monitors");
  await page.getByRole("button", { name: "Place a Monitor", exact: true }).click();
  await expect(page.getByRole("status").filter({ hasText: "Click the diagram to place" })).toContainText("Monitor");
  // the empty diagram's buttons step aside: a click in the middle places it
  await expect(page.getByRole("button", { name: "Add a part", exact: true })).toHaveCount(0);
  await page.locator(".react-flow__pane").first().click();
  await expect(nodes(page)).toHaveCount(1);
  await expect(nodes(page)).toContainText("Monitor 1");

  // from the keyboard it lands in the middle, with the keyboard on it
  await page.keyboard.press("Control+z");
  await expect(nodes(page)).toHaveCount(0);
  await showPanel(page, "Monitors");
  await page.getByRole("button", { name: "Place a Monitor", exact: true }).press("Enter");
  await expect(nodes(page)).toHaveCount(1);
  await expect(nodes(page)).toContainText("Monitor");
  await expect(nodes(page)).toBeFocused();
});

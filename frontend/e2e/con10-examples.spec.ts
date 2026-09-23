// CON-10: the examples come from the app itself, read-only. The Open menu
// lists them apart from the user's projects; an example opens as an unsaved
// copy that Save keeps as a new project, and one the user does not want is
// hidden (and restored), never deleted. The test engine starts with an empty
// projects folder, as a new install does.
import { expect, test, type Locator, type Page } from "@playwright/test";
import { expectProject, logLines, openApp, openFromMenu, ribbonTab, runActiveCase } from "./app";

/** Open the ribbon's Open menu; returns its two sections. */
async function openMenu(page: Page): Promise<{ projects: Locator; examples: Locator }> {
  await ribbonTab(page, "Home").click();
  await page.getByRole("button", { name: "Open", exact: true }).click();
  const menu = page.getByRole("menu", { name: "Open project" });
  return {
    projects: menu.getByRole("group", { name: "Your projects" }),
    examples: menu.getByRole("group", { name: "Examples" }),
  };
}

/** A section's entry whose name is exactly `name`. */
function entry(section: Locator, name: string): Locator {
  return section.getByRole("menuitem").filter({ has: section.page().getByText(name, { exact: true }) });
}

async function closeMenu(page: Page): Promise<void> {
  await ribbonTab(page, "Home").click(); // any click outside it
  await expect(page.getByRole("menu", { name: "Open project" })).toHaveCount(0);
}

test("CON-10: an example opens as a copy, and Save makes it a new project, leaving the example as shipped", async ({
  page,
}) => {
  const name = `E2E hybrid copy ${Date.now() % 100000}`;
  const shipped = await (await page.request.get("/api/examples/hybrid-car")).json();
  await openApp(page);

  const menu = await openMenu(page);
  await expect(entry(menu.projects, "P2 Hybrid Car")).toHaveCount(0);
  await entry(menu.examples, "P2 Hybrid Car").click();
  await expectProject(page, "P2 Hybrid Car", { unsaved: false });
  await expect(await logLines(page, "Example 'P2 Hybrid Car' opened as a copy.")).toBeVisible();

  await ribbonTab(page, "Project").click();
  await page.getByText("Project name").locator("xpath=following-sibling::input").fill(name);
  await ribbonTab(page, "Home").click();
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expectProject(page, name, { unsaved: false });
  await expect(await logLines(page, `Project '${name}' saved to the server as a new project`)).toBeVisible();

  // a new project with an id of its own; the example is as it shipped
  const projects: { id: string; name: string }[] = await (await page.request.get("/api/projects")).json();
  const saved = projects.find((p) => p.name === name)!;
  try {
    expect(saved.id).toMatch(/^hybrid-car-[a-z0-9]+$/);
    expect(await (await page.request.get("/api/examples/hybrid-car")).json()).toEqual(shipped);
    const again = await openMenu(page);
    await expect(entry(again.projects, name)).toContainText(saved.id);
    await expect(entry(again.examples, "P2 Hybrid Car")).toBeVisible();
    await expect(entry(again.examples, name)).toHaveCount(0);
  } finally {
    await page.request.delete(`/api/projects/${saved.id}`);
  }
});

test("CON-10: a hidden example stays out of the Open menu until the examples are restored", async ({ page }) => {
  await openApp(page);
  try {
    let menu = await openMenu(page);
    await menu.examples.getByRole("menuitem", { name: "Hide example 'P2 Hybrid Car'" }).click();
    await expect(entry(menu.examples, "P2 Hybrid Car")).toHaveCount(0);
    await expect(entry(menu.examples, "Battery Electric Car")).toBeVisible();
    await expect(await logLines(page, "Example 'P2 Hybrid Car' hidden from the Open menu")).toBeVisible();

    await page.reload(); // hidden for good, not for this session
    await expect(page.locator(".react-flow__node").first()).toBeVisible();
    menu = await openMenu(page);
    await expect(entry(menu.examples, "P2 Hybrid Car")).toHaveCount(0);
    await menu.examples.getByRole("menuitem", { name: "Restore hidden examples (1)" }).click();
    await expect(entry(menu.examples, "P2 Hybrid Car")).toBeVisible();
    await expect(menu.examples.getByRole("menuitem", { name: /^Restore hidden examples/ })).toHaveCount(0);
  } finally {
    await page.request.post("/api/examples/restore");
  }
});

test("CON-10: a copy an earlier version put in the projects folder stays the user's, apart from the example", async ({
  page,
}) => {
  // an earlier version copied the example in under its own id, and the user edited it
  const old = await (await page.request.get("/api/examples/bev-car")).json();
  old.name = `E2E earlier copy ${Date.now() % 100000}`;
  expect((await page.request.put("/api/projects/bev-car", { data: old })).ok()).toBe(true);
  try {
    await openApp(page); // the example, opened at start-up as a copy
    await expectProject(page, "Battery Electric Car", { unsaved: false });
    const menu = await openMenu(page);
    await expect(entry(menu.projects, old.name)).toContainText("bev-car");
    await expect(entry(menu.examples, "Battery Electric Car")).toBeVisible();
    await closeMenu(page);

    // runs made on the example are its copy's, not the earlier copy's, and
    // are listed again after a restart
    await runActiveCase(page);
    await expect(page.getByText("1 stored run", { exact: true })).toBeVisible();
    expect(await (await page.request.get("/api/projects/bev-car/runs")).json()).toEqual([]);
    await page.reload();
    await ribbonTab(page, "Results").click();
    await expect(page.getByText("1 stored run", { exact: true })).toBeVisible();

    await openFromMenu(page, old.name);
    await expectProject(page, old.name, { unsaved: false });
    await ribbonTab(page, "Results").click();
    await expect(page.getByText("No stored runs yet", { exact: true })).toBeVisible();
  } finally {
    await page.request.delete("/api/projects/bev-car");
  }
});

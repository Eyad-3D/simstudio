// PLT-33: projects as .lightsim files in any folder; PLT-07: older files are
// upgraded and newer ones open read-only; STD-02: files attached to a project.
//
// The desktop shell is the only one that names file paths. Here the test
// plays the shell: it tells the engine about a file with page.request (the
// test engine allows that, e2e/serve-engine.mjs) or, for Save As, stands in
// for the shell's bridge (window.lightsimDesktop) in the page.
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { expectProject, logLines, openApp, ribbonTab } from "./app";
import { ribbonButton } from "./ui-helpers";

let folder: string;
test.beforeEach(() => {
  folder = mkdtempSync(join(tmpdir(), "lightsim-e2e-repo-"));
});
test.afterEach(() => rmSync(folder, { recursive: true, force: true }));

async function example(page: Page): Promise<Record<string, unknown>> {
  return (await page.request.get("/api/examples/bev-car")).json();
}

/** Open a file from Home → Open → Recent files, as if the shell had opened it. */
async function openRecent(page: Page, path: string): Promise<void> {
  const res = await page.request.post("/api/files/open", { data: { path } });
  expect(res.ok(), await res.text()).toBe(true);
  await ribbonTab(page, "Home").click();
  await ribbonButton(page, "Open");
  const recent = page.getByRole("group", { name: "Recent files" });
  await recent.getByRole("menuitem", { name: new RegExp(path.replace(/[\\.]/g, "\\$&")) }).click();
  const discard = page.locator("button", { hasText: /^(Don.t save|Discard)/ });
  await page.waitForTimeout(300);
  if (await discard.count()) await discard.first().click();
}

test("PLT-33: a .lightsim file in any folder opens, saves in place and offers a reload when it changes", async ({ page }) => {
  const n = Date.now() % 100000;
  await openApp(page);
  const file = join(folder, "team-car.lightsim");
  writeFileSync(file, JSON.stringify({ ...(await example(page)), id: `e2e-plt33-${n}`, name: `Team car ${n}` }, null, 2));

  await openRecent(page, file);
  await expectProject(page, `Team car ${n}`, { unsaved: false });
  await expect(await logLines(page, `opened from ${file}`)).toHaveCount(1);
  await ribbonTab(page, "Project").click();
  await expect(page.getByText(file, { exact: true })).toBeVisible();

  // edit and save: the file itself changes, in the current format
  await page.getByRole("textbox", { name: "Project name" }).fill(`Team car ${n} v2`);
  await page.keyboard.press("Control+s");
  await expectProject(page, `Team car ${n} v2`, { unsaved: false });
  const saved = JSON.parse(readFileSync(file, "utf8"));
  expect(saved.name).toBe(`Team car ${n} v2`);
  expect(saved.schemaVersion).toBe(2);
  expect(existsSync(join(folder, "team-car.lightsim-backups", ".gitignore"))).toBe(true);

  // a teammate's version arrives with a git pull: LightSim offers to reload it
  writeFileSync(file, JSON.stringify({ ...saved, name: `Team car ${n} pulled` }, null, 2));
  await expect(page.getByText("Project changed on disk")).toBeVisible({ timeout: 15_000 });
  await page.getByRole("button", { name: "Reload", exact: true }).click();
  await expectProject(page, `Team car ${n} pulled`, { unsaved: false });
});

test("PLT-33: Save As writes a .lightsim file where the user chose", async ({ page }) => {
  const target = join(folder, "saved-as.lightsim");
  // the shell's bridge: its Save dialog "picks" `target`
  await page.addInitScript((path) => {
    window.lightsimDesktop = {
      openFile: async () => null,
      openDroppedFile: async () => null,
      showFile: async () => undefined,
      saveFileAs: async (projectId: string) => {
        const res = await fetch("/api/files/save-as", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ path, projectId }),
        });
        return res.json();
      },
    };
  }, target);
  await openApp(page);
  await ribbonTab(page, "Home").click();
  await page.getByRole("button", { name: "Save As…" }).click();
  await expect(await logLines(page, `saved as ${target}`)).toHaveCount(1);
  const saved = JSON.parse(readFileSync(target, "utf8"));
  expect(saved.name).toBe("Battery Electric Car");
  expect(saved.savedWith).toBeTruthy();
});

test("PLT-07: a file from a newer LightSim opens read-only and is never saved over", async ({ page }) => {
  const n = Date.now() % 100000;
  await openApp(page);
  const file = join(folder, "newer.lightsim");
  const newer = { ...(await example(page)), id: `e2e-plt07-${n}`, name: `Newer ${n}`, schemaVersion: 99, savedWith: "9.0.0" };
  writeFileSync(file, JSON.stringify(newer));
  const before = readFileSync(file, "utf8");

  await openRecent(page, file);
  await expect(await logLines(page, "saved by LightSim 9.0.0")).not.toHaveCount(0);
  await ribbonTab(page, "Project").click();
  await expect(page.getByRole("status").filter({ hasText: "Read-only" })).toBeVisible();
  await page.getByRole("textbox", { name: "Project name" }).fill(`Newer ${n} edited`);
  await page.keyboard.press("Control+s");
  await expect(await logLines(page, "Not saved: This project was saved by LightSim 9.0.0")).toHaveCount(1);
  expect(readFileSync(file, "utf8")).toBe(before);
});

test("STD-02: a file attached to the project is listed and goes out with the export", async ({ page }) => {
  await openApp(page);
  await ribbonTab(page, "Project").click();
  await page.getByRole("button", { name: "Attached" }).click();
  await page.getByLabel("Attach a file").setInputFiles({
    name: "drive-log.csv",
    mimeType: "text/csv",
    buffer: Buffer.from("t,v\n0,0\n1,2.5\n"),
  });
  const menu = page.getByRole("menu", { name: "Attached files" });
  await expect(menu.getByText("drive-log.csv")).toBeVisible();
  await expect(await logLines(page, "Attached 'drive-log.csv'")).toHaveCount(1);

  await ribbonTab(page, "Home").click();
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "Export" }).click();
  expect((await download).suggestedFilename()).toMatch(/\.lightsim\.zip$/);
});

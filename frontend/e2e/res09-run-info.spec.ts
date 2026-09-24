// RES-09: every run keeps the model, case settings, app version and live
// edits it was made with; Run info shows them and opens that model again.
import { expect, test } from "@playwright/test";
import { expectProject, openApp, runActiveCase } from "./app";
import { importProject } from "./ui-helpers";

test("RES-09: Run info shows what made a run and opens its model as an unsaved copy", async ({ page }) => {
  const n = Date.now() % 100000;
  const id = `e2e-res09-${n}`;
  const name = `E2E run info ${n}`;
  await openApp(page);
  // a project of its own, so runs stored by tests running alongside stay apart
  const example = await (await page.request.get("/api/examples/bev-car")).json();
  example.cases[0].duration = 60;
  await importProject(page, { ...example, id, name });
  await expectProject(page, name, { unsaved: true });
  const { version } = await (await page.request.get("/api/health")).json();

  await runActiveCase(page);
  await page.getByTitle(/^Run info/).click();
  const info = page.getByRole("region", { name: "Run info" });
  await expect(info).toContainText("City Cycle");
  await expect(info).toContainText("60 s · step 1 s · no pacing");
  await expect(info).toContainText(`${name} · 22 element(s) · #`);
  await expect(info).toContainText(`LightSim ${version}`);
  await expect(info).toContainText(/Live edits\s*none/);

  // stored on disk with the run; the run list stays small
  const [listed] = await (await page.request.get(`/api/projects/${id}/runs`)).json();
  expect(listed.snapshot).toBeUndefined();
  const stored = await (await page.request.get(`/api/projects/${id}/runs/${listed.id}`)).json();
  expect(stored.snapshot.project.id).toBe(id);
  expect(stored.snapshot.case).toMatchObject({ id: example.cases[0].id, duration: 60 });
  expect(stored.snapshot.appVersion).toBe(version);
  expect(stored.snapshot.modelHash).toMatch(/^[0-9a-f]{64}$/);
  expect(stored.snapshot.liveEdits).toEqual([]);

  await info.getByRole("button", { name: "Open as model" }).click();
  await page.getByRole("button", { name: "Don't save", exact: true }).click(); // the imported copy
  await expect(page.getByText(new RegExp(`^${name} \\(run of .+\\) •$`))).toBeVisible();
  await expect(page.locator(".react-flow__node")).toHaveCount(22);
});

// VAL-37: every run lists the data and methods it rests on, with their
// licences and credits, and saves them as citations.
import { readFile } from "node:fs/promises";
import { expect, test } from "@playwright/test";
import { openApp, runActiveCase } from "./app";
import { importProject } from "./ui-helpers";

test("VAL-37: Run info lists a run's sources and saves them as BibTeX", async ({ page }) => {
  const n = Date.now() % 100000;
  await openApp(page);
  const example = await (await page.request.get("/api/examples/bev-car")).json();
  example.cases[0].duration = 30;
  await importProject(page, { ...example, id: `e2e-val37-${n}`, name: `E2E sources ${n}` });
  await runActiveCase(page);

  await page.getByTitle(/^Run info/).click();
  await page.getByText("Sources & credits").click();
  const list = page.getByRole("region", { name: "Sources and credits" });
  await expect(list).toContainText("Battery Electric Car example");
  await expect(list).toContainText("source unknown");
  await expect(list).toContainText("FASTSim");
  await expect(list).toContainText("This run uses values whose source is unknown");

  const download = page.waitForEvent("download");
  await list.getByRole("button", { name: "BibTeX" }).click();
  const bib = await readFile(await (await download).path(), "utf-8");
  expect(bib).toMatch(/^@software\{lightsim,/);
  expect(bib).toContain("@misc{DR-01,");
});

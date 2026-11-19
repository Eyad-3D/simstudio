// RES-37: the Results chart's PNG button saves the whole chart (lines, axes
// and legend) at twice its on-screen size. It used to save the first <svg>
// in the chart area, which in Recharts 3 is a 14 px legend icon (a 28×28 PNG).
// Since RES-05 the chart is a canvas, and the picture is drawn from a copy of
// it at twice the size, with the legend in a row under it.
import { readFile } from "node:fs/promises";
import { expect, test } from "@playwright/test";
import { drawnLines, openApp, runActiveCase } from "./app";

test("RES-37: the PNG export is the whole chart at twice its on-screen size", async ({ page }) => {
  await openApp(page);
  await runActiveCase(page);
  const chart = drawnLines(page).first();
  await expect(chart).toBeVisible();
  const plot = (await chart.locator("canvas").boundingBox())!;
  // each line's colour, from its legend swatch, and the axes' token colour
  const hex = (rgb: string) =>
    "#" + (rgb.match(/\d+/g) ?? []).slice(0, 3).map((v) => Number(v).toString(16).padStart(2, "0")).join("");
  const colours = (
    await chart
      .locator(".u-legend .u-series:not(:first-child) .u-marker")
      .evaluateAll((ms) => ms.map((m) => getComputedStyle(m).borderTopColor))
  ).map(hex);
  expect(colours.length).toBeGreaterThan(0);
  const axis = await page.evaluate(() =>
    getComputedStyle(document.documentElement).getPropertyValue("--ss-text-dim").trim().toLowerCase(),
  );

  const [download] = await Promise.all([
    page.waitForEvent("download"),
    page.getByRole("button", { name: "PNG", exact: true }).click(),
  ]);
  expect(download.suggestedFilename()).toMatch(/^lightsim-.+\.png$/);
  const png = await readFile(await download.path());
  // a PNG starts with its 8-byte signature, then the IHDR chunk whose width
  // and height are the big-endian numbers at bytes 16 and 20
  expect(png.subarray(1, 4).toString()).toBe("PNG");
  expect(png.readUInt32BE(16)).toBeGreaterThanOrEqual(Math.floor(2 * plot.width));
  expect(png.readUInt32BE(20)).toBeGreaterThanOrEqual(Math.floor(2 * plot.height));

  // count the picture's colours in the plot and in the legend under it
  const legendTop = Math.round(2 * plot.height);
  const counts = await page.evaluate(
    async ({ b64, legendTop }) => {
      const img = new Image();
      img.src = `data:image/png;base64,${b64}`;
      await img.decode();
      const canvas = document.createElement("canvas");
      canvas.width = img.width;
      canvas.height = img.height;
      const ctx = canvas.getContext("2d")!;
      ctx.drawImage(img, 0, 0);
      const px = ctx.getImageData(0, 0, img.width, img.height).data;
      const plot: Record<string, number> = {};
      const key: Record<string, number> = {};
      for (let i = 0; i < px.length; i += 4) {
        const hex = "#" + [px[i], px[i + 1], px[i + 2]].map((v) => v.toString(16).padStart(2, "0")).join("");
        const into = i / 4 / img.width < legendTop ? plot : key;
        into[hex] = (into[hex] ?? 0) + 1;
      }
      return { plot, key, corner: [...px.subarray(0, 4)] };
    },
    { b64: png.toString("base64"), legendTop },
  );
  expect(counts.corner).toEqual([255, 255, 255, 255]); // the light theme's solid panel colour
  expect(counts.plot[axis] ?? 0, "axis tick labels").toBeGreaterThan(500);
  for (const c of colours) {
    expect(counts.plot[c] ?? 0, `line ${c}`).toBeGreaterThan(500);
    expect(counts.key[c] ?? 0, `legend icon in ${c}`).toBeGreaterThan(50);
  }
});

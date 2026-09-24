// RES-37: the Results chart's PNG button saves the whole chart (lines, axes
// and legend) at twice its on-screen size. It used to save the first <svg>
// in the chart area, which in Recharts 3 is a 14 px legend icon (a 28×28 PNG).
import { readFile } from "node:fs/promises";
import { expect, test } from "@playwright/test";
import { drawnLines, openApp, runActiveCase } from "./app";

test("RES-37: the PNG export is the whole chart at twice its on-screen size", async ({ page }) => {
  await openApp(page);
  await runActiveCase(page);
  await expect(drawnLines(page).first()).toBeVisible();
  const chart = (await page.locator(".recharts-wrapper").filter({ visible: true }).first().boundingBox())!;
  const legend = (await page.locator(".recharts-legend-wrapper").filter({ visible: true }).first().boundingBox())!;
  const colours = await drawnLines(page).evaluateAll((lines) => lines.map((l) => l.getAttribute("stroke")!));

  const [download] = await Promise.all([
    page.waitForEvent("download"),
    page.getByRole("button", { name: "PNG", exact: true }).click(),
  ]);
  expect(download.suggestedFilename()).toMatch(/^lightsim-.+\.png$/);
  const png = await readFile(await download.path());
  // a PNG starts with its 8-byte signature, then the IHDR chunk whose width
  // and height are the big-endian numbers at bytes 16 and 20
  expect(png.subarray(1, 4).toString()).toBe("PNG");
  expect(png.readUInt32BE(16)).toBeGreaterThanOrEqual(Math.floor(2 * chart.width));
  expect(png.readUInt32BE(20)).toBeGreaterThanOrEqual(Math.floor(2 * chart.height));

  // count the picture's colours above the legend (plot) and in it (legend)
  const legendTop = Math.round(2 * (legend.y - chart.y));
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
  expect(counts.plot["#666666"] ?? 0, "axis lines and tick labels").toBeGreaterThan(500);
  for (const c of colours) {
    expect(counts.plot[c] ?? 0, `line ${c}`).toBeGreaterThan(500);
    expect(counts.key[c] ?? 0, `legend icon and name in ${c}`).toBeGreaterThan(50);
  }
});

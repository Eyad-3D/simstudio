// STD-01: an FMU file dropped on the diagram becomes an FMU block. LightSim
// asks once whether it may run, shows what it is and where it runs, lets the
// user change a start value and pick its pins, and runs it with the car.
//
// The FMU is LightSimTest, built from backend/tests/fmu with the engine's
// Python; the test skips without a C compiler or without the FMU pack (FMPy).
import { expect, test } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { openApp, runActiveCase, showPanel } from "./app";

const backend = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "backend");
const python = process.env.LIGHTSIM_PYTHON || (process.platform === "win32" ? "python" : "python3");

function buildTestFmu(): string | null {
  try {
    const out = mkdtempSync(join(tmpdir(), "lightsim-fmu-"));
    const script =
      "import sys, pathlib; sys.path.insert(0, 'tests'); import fmpy; " +
      "from fmu.build import build; print(build(pathlib.Path(sys.argv[1])))";
    return execFileSync(python, ["-c", script, out], { cwd: backend }).toString().trim();
  } catch {
    return null;
  }
}

test("STD-01: a dropped FMU runs with the car", async ({ page }) => {
  const fmu = buildTestFmu();
  test.skip(!fmu, "needs a C compiler and the FMU pack (FMPy) in the engine's Python");

  await openApp(page);
  // drop the file on the diagram, as from the desktop
  const bytes = readFileSync(fmu!).toString("base64");
  const files = await page.evaluateHandle((b64) => {
    const data = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    const dt = new DataTransfer();
    dt.items.add(new File([data], "LightSimTest.fmu"));
    return dt;
  }, bytes);
  const pane = page.locator(".react-flow__pane");
  const box = (await pane.boundingBox())!;
  await pane.dispatchEvent("drop", { dataTransfer: files, clientX: box.x + 200, clientY: box.y + 120 });

  // asked once whether it may run
  await expect(page.getByText("Allow LightSimTest.fmu to run?")).toBeVisible();
  await page.getByRole("button", { name: "Allow it to run" }).click();

  // the block is named after the model and says what the FMU is
  await expect(page.locator(".react-flow__node").filter({ hasText: "LightSimTest" })).toBeVisible();
  await showPanel(page, "Properties");
  await expect(page.getByTestId("fmu-badge")).toHaveText("Runs here");
  await expect(page.getByText("FMI 2.0 · Co-Simulation · made with LightSim tests")).toBeVisible();
  await expect(page.getByText("Allowed to run on this computer")).toBeVisible();

  // its variables: change the gain's start value, and watch the FMU's own clock
  await page.getByRole("button", { name: /Variables and pins/ }).click();
  const table = page.getByRole("table", { name: "FMU variables" });
  await expect(table.getByRole("checkbox", { name: "Pin for u" })).toBeChecked();
  await expect(table.getByRole("checkbox", { name: "Pin for k" })).toBeDisabled();
  const k = table.getByRole("textbox", { name: "Start value of k" });
  await k.fill("3");
  await k.press("Enter");
  await expect(k).toHaveValue("3");
  await page.keyboard.press("Escape");

  // wire its input to the vehicle's speed on the Data Bus
  await showPanel(page, "Data Bus Connections");
  const source = page.getByRole("combobox", { name: "Source of LightSimTest · u" });
  await source.click();
  await page.getByRole("option", { name: "Vehicle · Vehicle Speed [km/h]" }).click();
  await expect(source).toHaveValue("Vehicle · Vehicle Speed [km/h]");

  // it runs with the rest of the model, and its outputs are results
  await runActiveCase(page);
  await page.getByPlaceholder("Search channels…").fill("LightSimTest");
  await expect(page.getByText("success", { exact: true })).toBeVisible();
  for (const output of ["y", "t_fmu"]) await expect(page.getByText(output, { exact: true })).toBeVisible();
});

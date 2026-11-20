// A part's help from its parameter window: the Help button (and F1) open the
// part's page in one Help window of its own, which every part's Help reuses.
import { expect, test } from "@playwright/test";
import { openApp, openParams } from "./app";

test("a part's Help opens its page in the Help window; another part's Help shows its page there", async ({
  page,
  context,
}) => {
  await openApp(page);
  const motor = await openParams(page, "E-Motor");
  const [help] = await Promise.all([
    motor.waitForEvent("popup"),
    motor.getByRole("button", { name: "Help", exact: true }).click(),
  ]);
  await expect(help).toHaveURL(/\/help\/reference\/components\/motor\.emotor\.html$/);
  await expect(help.getByRole("heading", { level: 1, name: "E-Motor" })).toBeVisible();

  // another part's Help: the same window shows that part's page
  const battery = await openParams(page, "HV Battery Pack");
  await battery.getByRole("button", { name: "Help", exact: true }).click();
  await expect(help).toHaveURL(/\/help\/reference\/components\/battery\.generic\.html$/);
  await expect(help.getByRole("heading", { level: 1, name: "HV Battery Pack" })).toBeVisible();
  // still one Help window: the app, two parameter windows and the help
  expect(context.pages()).toHaveLength(4);

  // F1 in a field: that parameter's row on the part's page, marked
  await motor.getByRole("spinbutton", { name: /^Maximum Speed/ }).focus();
  await motor.keyboard.press("F1");
  await expect(help).toHaveURL(/\/help\/reference\/components\/motor\.emotor\.html#max_speed_rpm$/);
  const row = help.locator("tr", { has: help.locator("#max_speed_rpm") });
  await expect(row).toBeInViewport();
  expect(await row.locator("td").first().evaluate((td) => getComputedStyle(td).backgroundColor)).not.toBe(
    "rgba(0, 0, 0, 0)",
  );
  expect(context.pages()).toHaveLength(4);
  // the main window's Help panel stays closed
  await expect(page.getByRole("complementary", { name: "Help" })).toHaveCount(0);
});

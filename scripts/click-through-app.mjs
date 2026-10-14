#!/usr/bin/env node
/**
 * Click through the PACKAGED desktop app and report what each click does.
 *
 * Starts the real executable with remote debugging on, opens the example car,
 * then in each main view clicks every visible button the way a user would (a
 * mouse click at its centre) and records:
 *   - COVERED: something else is on top of the button, so a click never
 *     reaches it (the button looks clickable but is not)
 *   - ERROR: a page error, a console error, a refused or failed request
 *   - NO EFFECT: nothing on the page changed and nothing was asked of the
 *     engine (a dead button, or one whose effect cannot be seen here: a
 *     chart drawn on a canvas, a view already where it was asked to go)
 *   - GONE: not there any more when its turn came (an earlier click changed
 *     the view); counted, not a fault
 * A button under an opaque layer (another view drawn over it) or scrolled
 * out of its panel is not one the user can see, so it is not tried. Buttons
 * that open the system's own dialogs (open, save, import, downloads), start
 * a long run or replace the project are left out. The first-steps tour a new
 * user sees is checked first: the first click must still act (UX-26).
 *
 * Prints one line per button as it goes and a summary; exits 1 if any button
 * is COVERED or gives an ERROR, unless --report-only. Stops clicking after
 * --minutes (default 15) and names the views it did not finish.
 *
 *   node scripts/click-through-app.mjs <executable> [args…] [--report-only] [--minutes N]
 *   xvfb-run -a node scripts/click-through-app.mjs desktop/node_modules/electron/dist/electron --no-sandbox desktop
 */
import { spawn, spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const require = createRequire(join(root, "frontend", "package.json"));
const { chromium } = require("playwright");

const argv = process.argv.slice(2);
const reportOnly = argv.includes("--report-only");
const mi = argv.indexOf("--minutes");
const minutes = mi >= 0 ? Number(argv[mi + 1]) : 15;
const [exe, ...extra] = argv.filter((a, i) => a !== "--report-only" && i !== mi && i !== mi + 1);
const DEBUG_PORT = 9334;
if (!exe || !existsSync(exe)) {
  console.error(`✗ no app at ${exe ?? "(no path given)"}`);
  process.exit(1);
}

// buttons that open a native dialog or a download (a dialog that waits for a
// person), or start a long run; matched on the name or the visible text
const NATIVE = /^(Open file…|Open…|Save As…|Import|Export|CSV|PNG|MATLAB|FS template|Allow a folder…)|^(Run|Stop)$/;
// buttons that leave the view under test or replace the project (not tried)
const LEAVES = /^(New|New project|Blank project|Start from an example|Reset UI|Clear history|Start|Project|Home|Simulations|Parameters|Results|Open the full Results page)$|parts · opens/;

let log = "";
// its own process group (POSIX), so the stop below ends the engine with it
const child = spawn(exe, [...extra, `--remote-debugging-port=${DEBUG_PORT}`, "--remote-allow-origins=*"],
  { detached: process.platform !== "win32" });
child.stdout.on("data", (d) => (log += d));
child.stderr.on("data", (d) => (log += d));
// the whole app, its engine included: ending the shell's process alone
// leaves the engine and Chromium's helpers running (and the debugging port
// taken)
const stop = () => {
  try {
    if (process.platform === "win32") spawnSync("taskkill", ["/pid", String(child.pid), "/T", "/F"], { windowsHide: true });
    else process.kill(-child.pid, "SIGKILL");
  } catch { /* gone */ }
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function connect() {
  const deadline = Date.now() + 180_000;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error(`app exited early (code ${child.exitCode})\n${log.slice(-2000)}`);
    try {
      const browser = await chromium.connectOverCDP(`http://127.0.0.1:${DEBUG_PORT}`);
      for (let i = 0; i < 240; i++) {
        for (const ctx of browser.contexts()) {
          const page = ctx.pages().find((p) => p.url().startsWith("http://127.0.0.1"));
          if (page) return { browser, page };
        }
        await sleep(500);
      }
    } catch { /* not up yet */ }
    await sleep(500);
  }
  throw new Error("the window never showed the app");
}

const results = [];
let problems = [];
const unfinished = [];
const t0 = Date.now();
const record = (r) => {
  results.push(r);
  console.log(`${r.verdict.padEnd(9)} ${r.view.padEnd(11)} ${r.name.padEnd(50).slice(0, 50)} ${r.detail.slice(0, 300)}`);
};

async function main() {
  const { page } = await connect();
  page.on("pageerror", (e) => problems.push(`page error: ${e.message}`));
  page.on("console", (m) => { if (m.type() === "error") problems.push(`console: ${m.text()}`); });
  page.on("requestfailed", (r) => problems.push(`request failed: ${r.method()} ${r.url()} ${(r.failure() || {}).errorText}`));
  page.on("response", (r) => { if (r.status() >= 400) problems.push(`HTTP ${r.status()}: ${r.request().method()} ${r.url()}`); });
  let requests = 0;
  page.on("request", () => requests++);
  // a browser dialog (alert, confirm) would block the window: note it, close it
  page.on("dialog", (d) => { problems.push(`a ${d.type()} dialog: ${d.message().slice(0, 120)}`); d.dismiss().catch(() => {}); });
  await page.waitForLoadState("domcontentloaded");
  await page.getByText("backend connected").waitFor({ timeout: 120_000 });

  const mutations = () => page.evaluate(() => {
    if (!window.__ctMo) {
      window.__ctMut = 0;
      window.__ctMo = new MutationObserver((m) => { window.__ctMut += m.length; });
      window.__ctMo.observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
    }
    return window.__ctMut;
  });

  /** The visible buttons, each with its name, centre and whether a click at
   *  the centre reaches it. */
  const candidates = () => page.evaluate(() => {
    const out = [];
    const all = [...document.querySelectorAll("button, [role=button], [role=tab], [role=menuitem]")];
    all.forEach((el, index) => {
      if (!(el instanceof HTMLElement) || !el.offsetParent) return;
      const r = el.getBoundingClientRect();
      if (r.width < 4 || r.height < 4 || r.bottom < 0 || r.right < 0 || r.top > innerHeight || r.left > innerWidth) return;
      const x = Math.min(innerWidth - 1, r.left + r.width / 2), y = Math.min(innerHeight - 1, r.top + r.height / 2);
      const top = document.elementFromPoint(x, y);
      const name = (el.getAttribute("aria-label") || el.title || el.textContent || "").trim().replace(/\s+/g, " ").slice(0, 70);
      const text = (el.textContent || "").trim().replace(/\s+/g, " ").slice(0, 70);
      const covered = !(top && (top === el || el.contains(top)));
      // Under an opaque layer (another view drawn over this one) the user
      // cannot see the button at all: hidden, not a trap. Under a clear or
      // see-through layer they see it and their click lands elsewhere.
      let hidden = false;
      // scrolled out of its panel: the user scrolls to it first
      for (let p = el.parentElement; covered && p; p = p.parentElement) {
        const o = getComputedStyle(p);
        if (!/(auto|scroll|hidden|clip)/.test(o.overflow + o.overflowX + o.overflowY)) continue;
        const q = p.getBoundingClientRect();
        if (x < q.left || x > q.right || y < q.top || y > q.bottom) { hidden = true; break; }
      }
      for (let n = covered && !hidden ? top : null; n && !n.contains(el); n = n.parentElement) {
        const st = getComputedStyle(n);
        const a = st.backgroundColor.match(/rgba?\(([^)]+)\)/);
        const alpha = a ? (a[1].split(",")[3] === undefined ? 1 : parseFloat(a[1].split(",")[3])) : 0;
        if (alpha >= 0.95 || st.backgroundImage !== "none") { hidden = true; break; }
      }
      const coveredBy = covered && top ? `${top.tagName.toLowerCase()}${top.id ? "#" + top.id : ""}.${String(top.className && top.className.baseVal !== undefined ? top.className.baseVal : top.className).split(" ").slice(0, 3).join(".")} "${(top.textContent || "").trim().slice(0, 40)}"` : "";
      // a tab swaps what the view shows: tried after the rest
      const isTab = el.getAttribute("role") === "tab" || el.hasAttribute("aria-selected") || !!el.closest(".dv-tab");
      out.push({ index, name, text, x, y, isTab, covered: covered && !hidden, hidden, coveredBy,
                 disabled: el.disabled === true || el.getAttribute("aria-disabled") === "true" });
    });
    return out;
  });

  // a ribbon tab, with whatever a click left open closed first
  const tab = async (name) => {
    for (let i = 0; i < 2; i++) { await page.keyboard.press("Escape"); await sleep(80); }
    await page.getByRole("button", { name, exact: true }).click({ timeout: 4000 });
  };

  const views = [
    ["Start", async () => { await tab("Start"); }],
    ["Home", async () => {
      await tab("Home");
      await page.locator(".react-flow__node", { hasText: "E-Motor" }).first().click({ timeout: 1500 }).catch(() => {});
    }],
    ["Simulations", async () => { await tab("Simulations"); }],
    ["Parameters", async () => { await tab("Parameters"); }],
    ["Results", async () => { await tab("Results"); }],
    ["Project", async () => { await tab("Project"); }],
  ];

  // the example car, and one run so Results has something to show
  await page.getByRole("button", { name: /Battery Electric Car, 24 parts|Continue with/ }).first().click();
  await sleep(2500);
  // a first-time user gets the first-steps tour; note whether it blocks the window, then close it
  const tour = await page.evaluate(() => !!document.querySelector(".driver-overlay"));
  if (tour) {
    const b = await page.getByRole("button", { name: "Results", exact: true }).boundingBox();
    await page.mouse.click(b.x + b.width / 2, b.y + b.height / 2);
    await sleep(800);
    const onResults = await page.evaluate(() => document.querySelector("[data-tour='tab-results']")?.className.includes("font-semibold") ?? false);
    const stillTour = await page.evaluate(() => !!document.querySelector(".driver-overlay"));
    record({ view: "Start", name: "first click after opening a project (tour shown)", verdict: onResults ? "ok" : "ERROR",
                   detail: onResults ? "" : `the click only closed the tour${stillTour ? " (the tour is still open)" : ""}` });
    if (stillTour) await page.locator(".driver-popover-close-btn").click().catch(() => {});
  }
  await page.locator("[data-tour='run']").click();
  await sleep(30_000); // the example's City Cycle runs in a few seconds
  problems = [];

  const deadline = t0 + minutes * 60_000;
  for (const [view, enter] of views) {
    if (Date.now() > deadline) { unfinished.push(view); continue; }
    const v0 = Date.now();
    try { await enter(); } catch (e) { record({ view, name: "(the view itself)", verdict: "ERROR", detail: `could not open it: ${e.message.split("\n")[0]}` }); continue; }
    await sleep(1000);
    const seen = new Set();
    const all = await candidates();
    const list = [...all.filter((c) => !c.isTab), ...all.filter((c) => c.isTab)];
    let left = 0;
    for (const c of list) {
      const key = c.name;
      if (!c.name || seen.has(key) || NATIVE.test(c.name) || NATIVE.test(c.text) || LEAVES.test(c.name) || LEAVES.test(c.text)) continue;
      seen.add(key);
      if (c.disabled || c.hidden) continue;
      if (Date.now() > deadline) { left++; continue; }
      if (c.covered) { record({ view, name: c.name, verdict: "COVERED", detail: `under ${c.coveredBy}` }); continue; }
      problems = [];
      try { await enter(); } catch { record({ view, name: c.name, verdict: "GONE", detail: "could not return to the view before it" }); continue; }
      await sleep(250);
      // the same button again, now (the view was re-entered)
      const now = (await candidates()).find((x) => x.name === c.name && !x.covered && !x.hidden);
      if (!now) { record({ view, name: c.name, verdict: "GONE", detail: "not there after re-entering the view" }); continue; }
      if (now.disabled) continue; // off now (an earlier click changed the model)
      const m0 = await mutations(); const r0 = requests;
      // a click that leaves the window unable to answer stops the sweep
      const m1 = await Promise.race([
        (async () => { await page.mouse.click(now.x, now.y); await sleep(800); return await mutations(); })(),
        sleep(30_000).then(() => null),
      ]);
      if (m1 === null) {
        record({ view, name: c.name, verdict: "ERROR", detail: "the window stopped answering after this click" });
        throw new Error(`the window stopped answering after "${c.name}" in ${view}`);
      }
      const verdict = problems.length ? "ERROR" : (m1 - m0 === 0 && requests - r0 === 0) ? "NO EFFECT" : "ok";
      record({ view, name: c.name, verdict, detail: problems.slice(0, 3).join(" ; ") });
      for (let i = 0; i < 2; i++) { await page.keyboard.press("Escape"); await sleep(120); }
    }
    if (left) unfinished.push(`${view} (${left} buttons not tried)`);
    console.log(`— ${view}: ${Math.round((Date.now() - v0) / 1000)} s`);
  }
}

let stopped = "";
try {
  await main();
} catch (e) {
  stopped = e.message;
  console.error(`✗ click-through stopped: ${e.message}\n\n--- app output ---\n${log.slice(-2000)}`);
}
const count = (v) => results.filter((r) => r.verdict === v).length;
console.log("\nwhat needs a look:");
for (const r of results.filter((r) => r.verdict !== "ok" && r.verdict !== "GONE")) console.log(`  ${r.verdict.padEnd(9)} ${r.view.padEnd(11)} ${r.name.slice(0, 50)}`);
if (unfinished.length) {
  console.log(`\nnot finished within ${minutes} min: ${unfinished.join(", ")}`);
  if (process.env.GITHUB_ACTIONS) console.log(`::warning::click-through: not finished within ${minutes} min: ${unfinished.join(", ")}`);
}
console.log(`\n${results.length} buttons in ${Math.round((Date.now() - t0) / 1000)} s: ${count("ok")} ok, ${count("COVERED")} covered, ${count("ERROR")} with errors, ${count("NO EFFECT")} with no visible effect, ${count("GONE")} gone`);
stop();
process.exit(stopped || (!reportOnly && (count("COVERED") || count("ERROR"))) ? 1 : 0);

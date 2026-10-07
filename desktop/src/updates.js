"use strict";

/**
 * Update checks that ask first (PLT-18).
 *
 * LightSim promises to contact nothing outside the computer. An update check
 * is an outside call, so there is none until the user says yes: the first
 * launch of an installed app asks once ("Check for updates once a day?" Yes /
 * No / Ask later), Help → Updates changes the answer, and the machine-wide
 * policy file (policy.js) can fix it for every user.
 *
 * With checks on, LightSim asks GitHub, where its downloads are, once a day
 * whether a newer version exists. The request names only the app's version
 * and platform (User-Agent "LightSim/<version> (<platform>)"); GitHub sees the
 * computer's internet address, as with any download. Nothing installs without
 * the user's say: a new version offers "Install on quit", "Skip this version"
 * or "Later", with the release notes' "Your results will change" part shown
 * first. Releases roll out in stages (stagingPercentage in latest.yml): each
 * install draws a random number once and keeps it on the computer.
 *
 * Installs that cannot replace themselves only say a new version exists and
 * open its download page: .deb packages (they need an install command), MSI
 * installs (electron-updater cannot update them; they default to off) and
 * unsigned Windows builds (an update could not be checked for its publisher).
 *
 * The pure functions here decide what to do and are unit-tested; initUpdates
 * wires them to Electron and electron-updater.
 */

const fs = require("node:fs");
const path = require("node:path");

const DAY_MS = 24 * 60 * 60 * 1000;
const RELEASES_URL = "https://github.com/Eyad-3D/simstudio/releases";

const QUESTION = {
  title: "Updates",
  message: "Check for updates once a day?",
  detail:
    "LightSim then asks GitHub, where its downloads are, whether a newer version " +
    "exists. It sends only its version number and platform; GitHub also sees your " +
    "computer's internet address, as with any download. LightSim never installs an " +
    "update without asking you first.\n\n" +
    "Until you say yes, LightSim contacts nothing outside your computer. You can " +
    "change this later in Help → Updates.",
  buttons: ["Yes", "No", "Ask later"],
};

/**
 * What kind of install this is, which decides whether it can update itself.
 * "dev" (not packaged), "nsis", "msi", "mac", "appimage" or "deb".
 */
function installKind({ isPackaged, platform, execPath, env = process.env, exists = fs.existsSync }) {
  if (!isPackaged) return "dev";
  if (platform === "win32") {
    // The NSIS installer leaves its uninstaller next to the app; an MSI does not.
    const dir = path.win32.dirname(execPath);
    return exists(path.win32.join(dir, "Uninstall LightSim.exe")) ? "nsis" : "msi";
  }
  if (platform === "darwin") return "mac";
  return env.APPIMAGE ? "appimage" : "deb";
}

/**
 * Whether this build can install its own updates: the update must be checked
 * for its publisher on Windows (a signed build names one in app-update.yml),
 * and macOS only updates signed apps (unsigned Mac builds are never published).
 */
function canInstall(kind, { signedWindows = false } = {}) {
  if (kind === "nsis") return signedWindows;
  return kind === "mac" || kind === "appimage";
}

/**
 * What the updater does now.
 *
 *   mode     "off" | "ask" | "notify" | "install"
 *   managed  true when the policy file decides it (the menu shows it locked)
 *   checks   whether the user's own setting is "check once a day"
 */
function decideMode({ kind, policy = {}, settings = {}, installable = false }) {
  if (kind === "dev") return { mode: "off", managed: false, checks: false };
  const ableMode = installable ? "install" : "notify";
  if (policy.updates === "off") return { mode: "off", managed: true, checks: false };
  if (policy.updates === "notify") return { mode: "notify", managed: true, checks: true };
  if (policy.updates === "auto") return { mode: ableMode, managed: true, checks: true };
  // MSI installs are for managed PCs: off unless the policy turns them on.
  if (kind === "msi") return { mode: "off", managed: false, checks: false };
  if (settings.consent === "yes") return { mode: ableMode, managed: false, checks: true };
  if (settings.consent === "no") return { mode: "off", managed: false, checks: false };
  return { mode: "ask", managed: false, checks: false };
}

/** Whether a scheduled check is due (at most one a day). */
function checkDue(settings, now = Date.now()) {
  const last = Number(settings.lastCheck) || 0;
  return now - last >= DAY_MS || last > now;
}

function readSettings(file) {
  try {
    const raw = JSON.parse(fs.readFileSync(file, "utf8"));
    return raw && typeof raw === "object" && !Array.isArray(raw) ? raw : {};
  } catch {
    return {};
  }
}

function writeSettings(file, settings) {
  try {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, JSON.stringify(settings, null, 2) + "\n");
  } catch { /* not fatal: the question comes again next launch */ }
}

const decode = (s) =>
  s.replace(/&nbsp;/g, " ").replace(/&lt;/g, "<").replace(/&gt;/g, ">")
    .replace(/&quot;/g, "\"").replace(/&#39;/g, "'").replace(/&#x27;/g, "'")
    .replace(/&rarr;/g, "→").replace(/&amp;/g, "&");

/** HTML release notes (as GitHub's feed gives them) to plain text. */
function htmlToText(html) {
  return decode(
    html
      .replace(/<\/(td|th)>\s*<(td|th)[^>]*>/gi, " | ")
      .replace(/<li[^>]*>/gi, "• ")
      .replace(/<br\s*\/?>|<\/(p|tr|li|h\d|div|table|thead|tbody|ul|ol)>/gi, "\n")
      .replace(/<[^>]+>/g, ""),
  ).replace(/[ \t]+\n/g, "\n").replace(/\n{3,}/g, "\n\n").trim();
}

/**
 * The "Your results will change" part of a release's notes, as plain text,
 * so the user sees what an update changes in their numbers before installing.
 * Takes electron-updater's releaseNotes: a string (Markdown or HTML) or a list
 * of { version, note } for every version between this one and the new one.
 */
function resultsThatChange(releaseNotes, maxChars = 1800) {
  const notes = Array.isArray(releaseNotes)
    ? releaseNotes.map((n) => ({ version: n.version, note: n.note || "" }))
    : [{ version: null, note: releaseNotes || "" }];
  const parts = [];
  for (const { version, note } of notes) {
    const text = /<[a-z][^>]*>/i.test(note) ? htmlToText(note) : note;
    const lines = text.split(/\r?\n/);
    const start = lines.findIndex((l) => /your results will change/i.test(l));
    if (start < 0) continue;
    const out = [];
    for (const line of lines.slice(start + 1)) {
      if (/^#{1,3} /.test(line) || /^(New|Fixed|Changed|Security|Removed|Known limits)\b/.test(line.trim())) break;
      // a Markdown table row: keep its cells, drop the separator row
      if (/^\|[\s|:-]+\|$/.test(line.trim())) continue;
      const row = line.trim().replace(/^\|\s*|\s*\|$/g, "").replace(/\s*\|\s*/g, " | ");
      if (row) out.push(row);
    }
    if (out.length) parts.push((version ? `${version}:\n` : "") + out.join("\n"));
  }
  if (!parts.length) return null;
  const all = parts.join("\n\n");
  return all.length > maxChars ? `${all.slice(0, maxChars).trimEnd()}…` : all;
}

/** The message shown when a newer version exists. */
function offerText({ version, mode, releaseNotes }) {
  const changes = resultsThatChange(releaseNotes);
  const what = changes
    ? `What changes in your results:\n\n${changes}`
    : "Its release notes list no changes to results.";
  const how = mode === "install"
    ? "Install on quit downloads it now and installs it when you close LightSim."
    : "Download page opens the page to download it in your browser.";
  return {
    message: `LightSim ${version} is available.`,
    detail: `${what}\n\n${how} Your projects are kept.`,
    buttons: mode === "install"
      ? ["Install on quit", "Release notes", "Skip this version", "Later"]
      : ["Download page", "Release notes", "Skip this version", "Later"],
  };
}

/** Whether app-update.yml names a publisher (only signed Windows builds do). */
function namesPublisher(resourcesPath) {
  try {
    const text = fs.readFileSync(path.join(resourcesPath, "app-update.yml"), "utf8");
    return /^publisherName:/m.test(text);
  } catch {
    return false;
  }
}

/**
 * Wire update checks into the app. Returns the menu items for Help → Updates
 * and a function that runs the first-launch question and the daily check
 * (call it once the window shows the UI).
 */
function initUpdates({ app, dialog, shell, getWindow, policy, logEvent, rebuildMenu }) {
  const file = path.join(app.getPath("userData"), "update-settings.json");
  let settings = readSettings(file);
  const kind = installKind({ isPackaged: app.isPackaged, platform: process.platform, execPath: process.execPath });
  const installable = canInstall(kind, {
    signedWindows: process.platform === "win32" && namesPublisher(process.resourcesPath),
  });
  const state = () => decideMode({ kind, policy, settings, installable });
  let updater = null;
  let timer = null;
  let busy = false;

  const save = (patch) => {
    settings = { ...settings, ...patch };
    writeSettings(file, settings);
  };

  /** electron-updater, loaded only once checks are allowed. */
  function getUpdater() {
    if (updater) return updater;
    const { autoUpdater } = require("electron-updater");
    autoUpdater.autoDownload = false; // nothing downloads before the user agrees
    autoUpdater.autoInstallOnAppQuit = true; // what "Install on quit" means
    autoUpdater.allowPrerelease = false;
    autoUpdater.logger = { info: () => {}, warn: (m) => logEvent(`updates: ${m}`), error: (m) => logEvent(`updates: ${m}`), debug: () => {} };
    // Only the version and platform, not Electron's full browser User-Agent.
    autoUpdater.requestHeaders = { "User-Agent": `LightSim/${app.getVersion()} (${process.platform}-${process.arch})` };
    updater = autoUpdater;
    return updater;
  }

  async function check({ manual = false } = {}) {
    const { mode, managed } = state();
    if (busy || (mode !== "notify" && mode !== "install" && !manual)) return;
    if (mode === "off" && managed) return;
    // A check the user asks for offers the install where this build can do it,
    // unless the policy says to notify only.
    const offerMode = installable && !(managed && mode === "notify") ? "install" : "notify";
    busy = true;
    save({ lastCheck: Date.now() });
    try {
      const result = await getUpdater().checkForUpdates();
      const info = result && result.isUpdateAvailable ? result.updateInfo : null;
      if (!info) {
        if (manual) await dialog.showMessageBox(getWindow(), { type: "info", title: "Updates", message: `LightSim ${app.getVersion()} is the newest version.` });
        return;
      }
      if (!manual && settings.skipVersion === info.version) return;
      await offer(info, offerMode);
    } catch (err) {
      logEvent(`updates: check failed (${err && err.message ? err.message : err})`);
      if (manual) dialog.showErrorBox("Updates", `LightSim could not check for updates:\n${err && err.message ? err.message : err}`);
    } finally {
      busy = false;
    }
  }

  async function offer(info, mode) {
    const text = offerText({ version: info.version, mode, releaseNotes: info.releaseNotes });
    const page = `${RELEASES_URL}/tag/v${info.version}`;
    for (;;) {
      const { response } = await dialog.showMessageBox(getWindow(), {
        type: "info", title: "Updates", ...text, defaultId: 0, cancelId: 3, noLink: true,
      });
      if (response === 1) { shell.openExternal(page); continue; }
      if (response === 2) save({ skipVersion: info.version });
      if (response === 0) {
        if (mode === "install") {
          try {
            await getUpdater().downloadUpdate();
            logEvent(`updates: ${info.version} downloaded; installs on quit`);
          } catch (err) {
            dialog.showErrorBox("Updates", `The download failed:\n${err && err.message ? err.message : err}`);
          }
        } else {
          shell.openExternal(page);
        }
      }
      return;
    }
  }

  function schedule() {
    if (timer) clearInterval(timer);
    timer = null;
    const { mode } = state();
    if (mode !== "notify" && mode !== "install") return;
    timer = setInterval(() => { if (checkDue(settings)) check(); }, 60 * 60 * 1000);
    timer.unref?.();
    if (checkDue(settings)) check();
  }

  async function ask() {
    const { response } = await dialog.showMessageBox(getWindow(), {
      type: "question", ...QUESTION, defaultId: 0, cancelId: 2, noLink: true,
    });
    save({ consent: ["yes", "no", "later"][response] ?? "later", askedAt: new Date().toISOString() });
    logEvent(`updates: user answered '${settings.consent}' to update checks`);
    rebuildMenu();
  }

  /** Run once the UI is up: ask on first launch, then check if due. */
  async function start() {
    if (state().mode === "ask") await ask();
    schedule();
  }

  function setChecks(on) {
    save({ consent: on ? "yes" : "no" });
    rebuildMenu();
    schedule();
  }

  function menuItems() {
    const s = state();
    if (kind === "dev") {
      return [{ label: "Updates are off when running from source", enabled: false }];
    }
    const managedNote = s.managed ? " (managed by your organisation)" : "";
    return [
      {
        label: `Check for Updates Once a Day${managedNote}`,
        type: "checkbox",
        checked: s.mode === "notify" || s.mode === "install",
        enabled: !s.managed && kind !== "msi",
        click: (item) => setChecks(item.checked),
      },
      {
        label: "Check for Updates Now",
        enabled: !(s.managed && s.mode === "off"),
        click: () => check({ manual: true }),
      },
      ...(kind === "msi" && !s.managed
        ? [{ label: "This install is managed by MSI: your IT updates it", enabled: false }]
        : []),
    ];
  }

  return { start, menuItems, kind, state };
}

module.exports = {
  DAY_MS,
  QUESTION,
  installKind,
  canInstall,
  decideMode,
  checkDue,
  readSettings,
  writeSettings,
  htmlToText,
  resultsThatChange,
  offerText,
  namesPublisher,
  initUpdates,
};

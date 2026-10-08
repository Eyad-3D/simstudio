"use strict";

/**
 * LightSim desktop shell.
 *
 * The app is the existing web stack wrapped in a window: a frozen copy of the
 * FastAPI backend runs as a child process on a stable loopback port and serves
 * both the API and the built UI, so the renderer talks to one origin and the
 * frontend's relative `/api` calls work untouched.
 */

const { app, BrowserWindow, Menu, dialog, ipcMain, session, shell } = require("electron");
const { spawn } = require("node:child_process");
const crypto = require("node:crypto");
const net = require("node:net");
const path = require("node:path");
const fs = require("node:fs");
const { copyOldProjects } = require("./old-projects");
const { EngineFiles, isProjectFile, projectFilesIn, suggestedName, withSuffix } = require("./project-files");
const { readPolicy, expandPath, engineEnv } = require("./policy");
const { initUpdates } = require("./updates");

const HEALTH_TIMEOUT_MS = 40_000;
const isWindows = process.platform === "win32";

/** @type {import("node:child_process").ChildProcess | null} */
let backend = null;
/** @type {BrowserWindow | null} */
let mainWindow = null;
let backendLog = "";
let quitting = false;

// A fresh secret for each launch. The engine refuses /api calls that do not
// carry it, so the web pages the user visits cannot drive the engine even
// though they can reach its loopback port. The window presents it once, on
// its first page load, and the engine answers with an HttpOnly cookie that
// the UI then sends by itself (see backend/app/security.py).
const launchToken = crypto.randomBytes(32).toString("hex");
// A second secret only this process has (never the window): the engine takes
// file paths only from requests that carry it (PLT-33, backend/app/security.py).
const shellToken = crypto.randomBytes(32).toString("hex");
/** The engine's origin once it is running; the window never leaves it. */
let appOrigin = null;
/** @type {EngineFiles | null} the engine's file routes, once it runs */
let engineFiles = null;
/** .lightsim files to open once the window is ready (a double-click at launch). */
const pendingFiles = [];
let windowReady = false;
/** Help → Updates (PLT-18); set once the app is ready. */
let updates = null;

// The window only talks to the engine on 127.0.0.1, so it needs no proxy,
// and looking one up (WPAD on Windows) is a query on the network: none from
// the first request on. An update check the user agreed to switches its
// session to the system's proxy (src/updates.js).
app.commandLine.appendSwitch("no-proxy-server");

// The machine-wide policy file IT can put on a PC (PLT-36, src/policy.js).
// Read once per launch; it is written by an administrator, not by the app.
const policy = readPolicy();

/**
 * Where projects are saved: the app's data folder, or the first folder of
 * the policy's projectsRoots (for example a network drive in a lab).
 */
function projectsDir() {
  const roots = policy.settings.projectsRoots;
  if (roots && roots.length) return path.resolve(expandPath(roots[0]));
  return path.join(app.getPath("userData"), "projects");
}

// The UI keeps its settings (theme, dock layout, crash-recovery draft) in
// localStorage, which is keyed by origin — so the port must stay the same
// between launches or those settings silently vanish on every restart. Each
// installation picks its own port once rather than sharing a fixed number,
// so there is no well-known port for other software to aim at.

/** Ask the OS for any free port. */
function findFreePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.unref();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

function portIsFree(port) {
  return new Promise((resolve) => {
    const server = net.createServer();
    server.unref();
    server.once("error", () => resolve(false));
    server.listen(port, "127.0.0.1", () => server.close(() => resolve(true)));
  });
}

/**
 * Reuse the port from the last launch; on first launch, or if another program
 * has taken it since, pick a new free port and remember that instead.
 */
async function choosePort() {
  const file = path.join(app.getPath("userData"), "backend-port.json");
  let saved = null;
  try {
    saved = JSON.parse(fs.readFileSync(file, "utf8")).port;
  } catch { /* first launch, or unreadable — pick a new port */ }

  const reusable = Number.isInteger(saved) && saved > 0 && await portIsFree(saved);
  const port = reusable ? saved : await findFreePort();

  if (port !== saved) {
    try {
      fs.mkdirSync(path.dirname(file), { recursive: true });
      fs.writeFileSync(file, JSON.stringify({ port }));
    } catch { /* not fatal: settings just won't carry over to the next launch */ }
  }
  return port;
}

/** Record a one-off event in main.log in the app's data folder. */
function logEvent(message) {
  console.log(message);
  try {
    const line = `${new Date().toISOString()} ${message}\n`;
    fs.appendFileSync(path.join(app.getPath("userData"), "main.log"), line);
  } catch { /* not fatal */ }
}

/**
 * Locate the backend executable and the built UI.
 *
 * Packaged, both are unpacked next to the app under `resources/`. In
 * development we fall back to the repo layout: the PyInstaller output if it
 * has been built, otherwise the interpreter on PATH.
 */
function resolveBackend() {
  const exeName = isWindows ? "lightsim-backend.exe" : "lightsim-backend";
  const packagedDir = path.join(process.resourcesPath || "", "backend");
  const packagedExe = path.join(packagedDir, exeName);
  const staticPackaged = path.join(process.resourcesPath || "", "frontend");

  if (app.isPackaged || fs.existsSync(packagedExe)) {
    return { command: packagedExe, args: [], staticDir: staticPackaged };
  }

  const repoRoot = path.resolve(__dirname, "..", "..");
  const frozenExe = path.join(
    repoRoot, "backend", "dist", "lightsim-backend", exeName,
  );
  const staticDev = path.join(repoRoot, "frontend", "dist");

  if (fs.existsSync(frozenExe)) {
    return { command: frozenExe, args: [], staticDir: staticDev };
  }
  return {
    command: isWindows ? "python" : "python3",
    args: [path.join(repoRoot, "backend", "run_backend.py")],
    staticDir: staticDev,
    cwd: path.join(repoRoot, "backend"),
  };
}

/** Poll /api/health until the backend answers, or give up. */
async function waitForBackend(port) {
  const deadline = Date.now() + HEALTH_TIMEOUT_MS;
  const url = `http://127.0.0.1:${port}/api/health`;
  while (Date.now() < deadline) {
    if (backend && backend.exitCode !== null) {
      throw new Error(`The simulation engine stopped before it was ready.\n\n${backendLog.slice(-2000)}`);
    }
    try {
      const res = await fetch(url);
      if (res.ok) return;
    } catch {
      /* not listening yet — keep polling */
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error(`The simulation engine did not start within ${HEALTH_TIMEOUT_MS / 1000}s.\n\n${backendLog.slice(-2000)}`);
}

function startBackend(port) {
  const { command, args, staticDir, cwd } = resolveBackend();
  const projects = projectsDir();
  fs.mkdirSync(projects, { recursive: true });

  backend = spawn(command, [...args, "--port", String(port), "--host", "127.0.0.1"], {
    cwd,
    windowsHide: true,
    env: {
      ...engineEnv(process.env, app.isPackaged),
      LIGHTSIM_PROJECTS_DIR: projects,
      // this user's own folder: script approvals and other answers that must
      // never sit in a projects folder others can write (backend/app/paths.py)
      LIGHTSIM_DATA_DIR: app.getPath("userData"),
      LIGHTSIM_STATIC_DIR: staticDir,
      LIGHTSIM_TOKEN: launchToken,
      LIGHTSIM_SHELL_TOKEN: shellToken,
      // the settings the policy file fixes that the engine applies
      // (scriptTrust, examples); the UI reads them from /api/policy
      LIGHTSIM_POLICY: JSON.stringify(policy.settings),
      PYTHONUNBUFFERED: "1",
    },
  });

  const record = (chunk) => {
    backendLog += chunk.toString();
    if (backendLog.length > 20_000) backendLog = backendLog.slice(-20_000);
  };
  backend.stdout.on("data", record);
  backend.stderr.on("data", record);

  backend.on("error", (err) => {
    backendLog += `\nFailed to launch ${command}: ${err.message}\n`;
  });

  backend.on("exit", (code) => {
    backend = null;
    if (quitting || code === 0) return;
    dialog.showErrorBox(
      "Simulation engine stopped",
      `LightSim's background engine exited unexpectedly (code ${code}).\n\n` +
        `Saved projects are safe in:\n${projects}\n\nRestart LightSim to continue.\n\n${backendLog.slice(-1500)}`,
    );
  });
}

function stopBackend() {
  if (!backend) return;
  const child = backend;
  backend = null;
  if (isWindows) {
    // The frozen backend spawns its own children; /T takes the whole tree.
    spawn("taskkill", ["/pid", String(child.pid), "/T", "/F"], { windowsHide: true });
  } else {
    child.kill("SIGTERM");
    setTimeout(() => child.killed || child.kill("SIGKILL"), 3000).unref();
  }
}

/**
 * A document installed with the app (extraResources in electron-builder.yml),
 * or its source in the repo while developing.
 */
function bundledDoc(name, repoPath) {
  if (app.isPackaged) return path.join(process.resourcesPath, name);
  return path.join(__dirname, "..", "..", repoPath);
}

/**
 * Open a document in the user's default app. With no app registered for the
 * file type (common for .md on Windows), show it in its folder instead.
 */
async function openDoc(file) {
  if (!fs.existsSync(file)) {
    dialog.showErrorBox("File not found", `${path.basename(file)} is missing:\n${file}`);
    return;
  }
  const error = await shell.openPath(file);
  if (error) shell.showItemInFolder(file);
}

/** Run one of the window's file commands (App.tsx); false if it cannot. */
function inWindow(script) {
  if (!mainWindow || mainWindow.isDestroyed() || !windowReady) return Promise.resolve(false);
  return mainWindow.webContents.executeJavaScript(script).catch(() => false);
}

/** Open a .lightsim file the user picked or double-clicked, in the window. */
async function openProjectFile(file) {
  if (!engineFiles || !windowReady) {
    pendingFiles.push(file);
    return;
  }
  try {
    const info = await engineFiles.open(file);
    app.addRecentDocument(info.path);
    await inWindow(`window.lightsimOpenProjectId ? window.lightsimOpenProjectId(${JSON.stringify(info.id)}) : false`);
    buildMenu();
  } catch (err) {
    dialog.showErrorBox("LightSim could not open the file", `${file}\n\n${err.message || err}`);
  }
}

/** The files the File → Open Recent menu lists (none before the engine runs). */
let recentFiles = [];
async function refreshRecent() {
  if (!engineFiles) return;
  try {
    recentFiles = (await engineFiles.recent()).filter((f) => f.exists).slice(0, 10);
  } catch {
    recentFiles = [];
  }
}

// The Idea issue form (.github/ISSUE_TEMPLATE/idea.yml); GitHub fills a
// form field from the query parameter named after its id.
function feedbackUrl(version) {
  const q = new URLSearchParams({ template: "idea.yml", title: "What stopped me: ", version });
  return `https://github.com/Eyad-3D/simstudio/issues/new?${q}`;
}

function buildMenu() {
  const projects = projectsDir();
  const knownLimits = bundledDoc("KNOWN-LIMITS.md", "docs/KNOWN-LIMITS.md");
  const notices = bundledDoc("THIRD-PARTY-NOTICES.txt", "THIRD-PARTY-NOTICES.txt");
  // The help pages, served by the engine, open in the window's Help panel
  // (LRN-09); in the system browser if the window cannot show them, and
  // before the engine is up Known Limits is the file installed with the app.
  const inWindow = async (call) => {
    if (!mainWindow || !appOrigin) return false;
    try {
      return (await mainWindow.webContents.executeJavaScript(call)) === true;
    } catch {
      return false;
    }
  };
  const helpPage = async (page) => {
    if (await inWindow(`window.lightsimHelp ? window.lightsimHelp(${JSON.stringify(page)}) : false`)) return;
    return appOrigin ? shell.openExternal(`${appOrigin}/help/${page}`) : openDoc(knownLimits);
  };
  const pages = [
    ["Your First Electric Car (Tutorial)", "tutorials/first-electric-car.html"],
    ["Formula Student Lessons", "lessons/fs-1-acceleration.html"],
    ["Examples Guide", "examples/bev-car.html"],
    ["Results Numbers Explained", "reference/results.html"],
    ["Keyboard Shortcuts", "reference/keyboard-shortcuts.html"],
    ["Glossary", "glossary.html"],
  ].map(([label, page]) => ({ label, click: () => helpPage(page) }));
  const template = [
    {
      label: "File",
      submenu: [
        {
          label: "Open…",
          accelerator: "CmdOrCtrl+O",
          registerAccelerator: false, // the page handles the keys (it asks about unsaved work first)
          click: () => inWindow("window.lightsimOpenFile ? window.lightsimOpenFile() : false"),
        },
        {
          label: "Open Recent",
          submenu: recentFiles.length
            ? recentFiles.map((f) => ({
                label: `${f.name} — ${f.path}`,
                click: () => inWindow(`window.lightsimOpenProjectId ? window.lightsimOpenProjectId(${JSON.stringify(f.id)}) : false`),
              }))
            : [{ label: "No recent files", enabled: false }],
        },
        { type: "separator" },
        {
          label: "Save",
          accelerator: "CmdOrCtrl+S",
          registerAccelerator: false,
          click: () => inWindow("window.lightsimSave ? window.lightsimSave() : false"),
        },
        {
          label: "Save As…",
          accelerator: "CmdOrCtrl+Shift+S",
          registerAccelerator: false,
          click: () => inWindow("window.lightsimSaveAs ? window.lightsimSaveAs() : false"),
        },
        { type: "separator" },
        {
          label: "Open Projects Folder",
          click: () => shell.openPath(projects),
        },
        { type: "separator" },
        { role: isWindows ? "quit" : "close" },
      ],
    },
    {
      label: "View",
      submenu: [
        { role: "reload" },
        { role: "forceReload" },
        { role: "toggleDevTools" },
        { type: "separator" },
        { role: "resetZoom" },
        { role: "zoomIn" },
        { role: "zoomOut" },
        { type: "separator" },
        { role: "togglefullscreen" },
      ],
    },
    {
      label: "Help",
      submenu: [
        {
          label: "Documentation",
          accelerator: "F1",
          registerAccelerator: false, // the page handles F1 (the selected part's page)
          click: () => helpPage("index.html"),
        },
        ...pages,
        { type: "separator" },
        {
          label: "Known Limits",
          click: () => helpPage("known-limits.html"),
        },
        {
          label: "What Is Validated",
          click: () => helpPage("validation.html"),
        },
        {
          label: "Release Notes",
          click: () => helpPage("release-notes.html"),
        },
        {
          label: "Show the First-Steps Tour",
          click: () => inWindow("window.lightsimTour ? (window.lightsimTour(), true) : false"),
        },
        {
          label: "Report a Problem…",
          click: () => shell.openExternal("https://github.com/Eyad-3D/simstudio/issues"),
        },
        { type: "separator" },
        {
          label: "Third-Party Notices",
          click: () => openDoc(notices),
        },
        { type: "separator" },
        { label: "Updates", submenu: updates ? updates.menuItems() : [] },
        { type: "separator" },
        {
          // BIZ-35: opens the public Idea form in the browser, with the
          // version filled in; nothing is sent unless the user submits it.
          label: "What Stopped You?…",
          click: () => shell.openExternal(feedbackUrl(app.getVersion())),
        },
        { type: "separator" },
        {
          label: "About LightSim",
          click: async () => {
            const { response } = await dialog.showMessageBox({
              type: "info",
              title: "About LightSim",
              message: `LightSim ${app.getVersion()}`,
              detail:
                "A desktop app for simulating vehicle energy use, range and powertrains.\n\n" +
                "This is an early version: the physics are simplified, nothing is " +
                "validated against measured vehicles yet, and some results are known " +
                "to be wrong. Read Known Limits before relying on a number.\n\n" +
                `Projects folder:\n${projects}\n\n` +
                "Copyright © 2026 Eyad Abualkhair. All rights reserved.\n" +
                "Free for non-commercial use; commercial use needs a licence (see EULA.txt).\n" +
                "Includes open-source software and data under their own licences (Help → Third-Party Notices).",
              buttons: ["OK", "Known Limits"],
              defaultId: 0,
              cancelId: 0,
            });
            if (response === 1) helpPage("known-limits.html");
          },
        },
      ],
    },
  ];
  Menu.setApplicationMenu(Menu.buildFromTemplate(template));
}

/**
 * Closing or reloading the window with unsaved changes: the UI refuses to
 * unload (beforeunload) and this asks Save / Don't save / Cancel. Save goes
 * through the UI's own save, then closes or reloads; if the save fails the
 * window stays open with the error in Messages.
 */
function askBeforeUnsavedUnload(win) {
  let closing = false; // "close" comes before beforeunload; a reload has none
  win.on("close", () => {
    closing = true;
  });
  win.webContents.on("will-prevent-unload", (event) => {
    const wasClosing = closing;
    closing = false;
    const choice = dialog.showMessageBoxSync(win, {
      type: "warning",
      buttons: ["Save", "Don't save", "Cancel"],
      defaultId: 0,
      cancelId: 2,
      noLink: true,
      title: "Unsaved changes",
      message: `Save changes to the project before ${wasClosing ? "closing" : "reloading"}?`,
      detail:
        "If you don't save, LightSim keeps your changes only as a recovery " +
        "draft, which it offers again the next time it opens.",
    });
    if (choice === 1) {
      event.preventDefault(); // unload without saving
    } else if (choice === 0) {
      win.webContents
        .executeJavaScript("window.lightsimSave ? window.lightsimSave() : false")
        .then((saved) => {
          if (!saved || win.isDestroyed()) return;
          if (wasClosing) win.close();
          else win.webContents.reload();
        })
        .catch(() => {});
    }
  });
}

/** Answer the window's file requests (preload.js) only when they come from
 *  the engine's own page. */
function fromApp(event) {
  try {
    return Boolean(appOrigin) && new URL(event.senderFrame.url).origin === appOrigin;
  } catch {
    return false;
  }
}

function handleFileRequests() {
  const filters = [{ name: "LightSim project", extensions: ["lightsim"] }];
  ipcMain.handle("lightsim:open-file", async (event) => {
    if (!fromApp(event) || !engineFiles) return null;
    const { canceled, filePaths } = await dialog.showOpenDialog(mainWindow, {
      title: "Open a LightSim project",
      properties: ["openFile"],
      filters,
    });
    if (canceled || !filePaths[0]) return null;
    const info = await engineFiles.open(filePaths[0]);
    app.addRecentDocument(info.path);
    await refreshRecent();
    buildMenu();
    return info;
  });
  ipcMain.handle("lightsim:save-file-as", async (event, projectId, name) => {
    if (!fromApp(event) || !engineFiles) return null;
    const { canceled, filePath } = await dialog.showSaveDialog(mainWindow, {
      title: "Save the project as",
      defaultPath: path.join(app.getPath("documents"), suggestedName(name)),
      filters,
      properties: ["showOverwriteConfirmation", "createDirectory"],
    });
    if (canceled || !filePath) return null;
    const info = await engineFiles.saveAs(withSuffix(filePath), projectId);
    app.addRecentDocument(info.path);
    await refreshRecent();
    buildMenu();
    return info;
  });
  ipcMain.handle("lightsim:open-dropped", async (event, file) => {
    if (!fromApp(event) || !engineFiles || !file || !isProjectFile(file) || !fs.existsSync(file)) return null;
    const info = await engineFiles.open(file);
    app.addRecentDocument(info.path);
    await refreshRecent();
    buildMenu();
    return info;
  });
  ipcMain.handle("lightsim:show-file", async (event, projectId) => {
    if (!fromApp(event) || !engineFiles) return;
    const file = (await engineFiles.recent()).find((f) => f.id === projectId);
    if (file) shell.showItemInFolder(file.path);
  });
}

async function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1600,
    height: 1000,
    minWidth: 1024,
    minHeight: 700,
    show: false,
    backgroundColor: "#1e1e1e",
    title: "LightSim",
    icon: path.join(__dirname, "..", "build", "icon.png"),
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      preload: path.join(__dirname, "preload.js"),
      // Electron's spell checker downloads its dictionaries from Google on
      // Linux; LightSim contacts nothing outside the computer (PLT-18).
      spellcheck: false,
    },
  });
  askBeforeUnsavedUnload(mainWindow);

  // Keep external links in the user's browser rather than inside the app.
  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    if (/^https?:\/\//i.test(url)) shell.openExternal(url);
    return { action: "deny" };
  });
  // The window only ever shows the engine's UI; a link or a dropped file
  // that would navigate it elsewhere opens in the browser, or not at all.
  mainWindow.webContents.on("will-navigate", (event, url) => {
    if (appOrigin && new URL(url).origin === appOrigin) return;
    event.preventDefault();
    if (/^https?:\/\//i.test(url)) shell.openExternal(url);
  });

  await mainWindow.loadFile(path.join(__dirname, "loading.html"));
  mainWindow.show();

  try {
    // before the engine starts (it creates the projects folder)
    await copyOldProjects(app.getPath("appData"), app.getPath("userData"), logEvent);
    const port = await choosePort();
    startBackend(port);
    await waitForBackend(port);
    appOrigin = `http://127.0.0.1:${port}`;
    engineFiles = new EngineFiles(appOrigin, launchToken, shellToken);
    await mainWindow.loadURL(`${appOrigin}/`, {
      extraHeaders: `Authorization: Bearer ${launchToken}\n`,
    });
    windowReady = true;
    await refreshRecent();
    buildMenu();
    // files double-clicked to start the app
    for (const file of pendingFiles.splice(0)) await openProjectFile(file);
    // The update question comes once the UI is up, never before: until the
    // user says yes, nothing leaves the computer.
    updates?.start().catch((err) => logEvent(`updates: ${err.message || err}`));
  } catch (err) {
    dialog.showErrorBox("LightSim could not start", String(err.message || err));
    app.quit();
  }
}

/**
 * Electron's spell checker fetches its dictionaries from Google's servers
 * (on Linux, and on Windows when it uses Hunspell). LightSim contacts
 * nothing outside the computer unless the user agrees (PLT-18), so it is off
 * and any download it still tries goes nowhere. No proxy lookup either.
 */
function stopSpellCheckDownloads() {
  const ses = session.defaultSession;
  try {
    ses.setSpellCheckerEnabled(false);
    ses.setSpellCheckerDictionaryDownloadURL("http://127.0.0.1:9/");
    if (process.platform !== "darwin") ses.setSpellCheckerLanguages([]);
  } catch (err) {
    logEvent(`spell checker: ${err.message || err}`);
  }
}

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  // a .lightsim file double-clicked while the app runs comes in a second
  // instance's command line
  app.on("second-instance", (_event, argv, cwd) => {
    if (mainWindow) {
      if (mainWindow.isMinimized()) mainWindow.restore();
      mainWindow.focus();
    }
    for (const file of projectFilesIn(argv.slice(1), cwd)) void openProjectFile(file);
  });
  // macOS hands files over this way (before or after launch)
  app.on("open-file", (event, file) => {
    event.preventDefault();
    if (isProjectFile(file)) void openProjectFile(file);
  });
  pendingFiles.push(...projectFilesIn(process.argv.slice(1)));

  app.whenReady().then(() => {
    handleFileRequests();
    stopSpellCheckDownloads();
    if (policy.found) {
      logEvent(`policy: ${policy.file} sets ${JSON.stringify(policy.settings)}`);
      for (const problem of policy.problems) logEvent(`policy: ${problem}`);
    }
    updates = initUpdates({
      app, dialog, shell, logEvent,
      policy: policy.settings,
      getWindow: () => mainWindow,
      rebuildMenu: buildMenu,
    });
    buildMenu();
    createWindow();
    app.on("activate", () => {
      if (BrowserWindow.getAllWindows().length === 0) createWindow();
    });
  });

  app.on("window-all-closed", () => app.quit());
  // Stop the engine only once the quit goes ahead: the window may still ask
  // to save unsaved changes (which needs the engine), and Cancel keeps both.
  app.on("will-quit", () => {
    quitting = true;
    stopBackend();
  });
  app.on("quit", stopBackend);
}

"use strict";

/**
 * SimStudio desktop shell.
 *
 * The app is the existing web stack wrapped in a window: a frozen copy of the
 * FastAPI backend runs as a child process on a stable loopback port and serves
 * both the API and the built UI, so the renderer talks to one origin and the
 * frontend's relative `/api` calls work untouched.
 */

const { app, BrowserWindow, Menu, dialog, shell } = require("electron");
const { spawn } = require("node:child_process");
const crypto = require("node:crypto");
const net = require("node:net");
const path = require("node:path");
const fs = require("node:fs");

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
/** The engine's origin once it is running; the window never leaves it. */
let appOrigin = null;

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

/**
 * Locate the backend executable and the built UI.
 *
 * Packaged, both are unpacked next to the app under `resources/`. In
 * development we fall back to the repo layout: the PyInstaller output if it
 * has been built, otherwise the interpreter on PATH.
 */
function resolveBackend() {
  const exeName = isWindows ? "simstudio-backend.exe" : "simstudio-backend";
  const packagedDir = path.join(process.resourcesPath || "", "backend");
  const packagedExe = path.join(packagedDir, exeName);
  const staticPackaged = path.join(process.resourcesPath || "", "frontend");

  if (app.isPackaged || fs.existsSync(packagedExe)) {
    return { command: packagedExe, args: [], staticDir: staticPackaged };
  }

  const repoRoot = path.resolve(__dirname, "..", "..");
  const frozenExe = path.join(
    repoRoot, "backend", "dist", "simstudio-backend", exeName,
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
  const projectsDir = path.join(app.getPath("userData"), "projects");
  fs.mkdirSync(projectsDir, { recursive: true });

  backend = spawn(command, [...args, "--port", String(port), "--host", "127.0.0.1"], {
    cwd,
    windowsHide: true,
    env: {
      ...process.env,
      SIMSTUDIO_PROJECTS_DIR: projectsDir,
      SIMSTUDIO_STATIC_DIR: staticDir,
      SIMSTUDIO_TOKEN: launchToken,
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
      `SimStudio's background engine exited unexpectedly (code ${code}).\n\n` +
        `Saved projects are safe in:\n${projectsDir}\n\nRestart SimStudio to continue.\n\n${backendLog.slice(-1500)}`,
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

function buildMenu() {
  const projectsDir = path.join(app.getPath("userData"), "projects");
  const template = [
    {
      label: "File",
      submenu: [
        {
          label: "Open Projects Folder",
          click: () => shell.openPath(projectsDir),
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
          label: "About SimStudio",
          click: () =>
            dialog.showMessageBox({
              type: "info",
              title: "About SimStudio",
              message: `SimStudio ${app.getVersion()}`,
              detail:
                "A vehicle system simulation tool.\n\n" +
                "Component physics are simplified placeholders — this " +
                "demonstrates the workflow, not validated component fidelity.\n\n" +
                `Projects folder:\n${projectsDir}\n\n` +
                "Copyright © 2026 Eyad Abualkhair. All rights reserved.\n" +
                "Free for non-commercial use; commercial use needs a licence (see EULA.txt).",
            }),
        },
      ],
    },
  ];
  Menu.setApplicationMenu(Menu.buildFromTemplate(template));
}

async function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1600,
    height: 1000,
    minWidth: 1024,
    minHeight: 700,
    show: false,
    backgroundColor: "#1e1e1e",
    title: "SimStudio",
    icon: path.join(__dirname, "..", "build", "icon.png"),
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });

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
    const port = await choosePort();
    startBackend(port);
    await waitForBackend(port);
    appOrigin = `http://127.0.0.1:${port}`;
    await mainWindow.loadURL(`${appOrigin}/`, {
      extraHeaders: `Authorization: Bearer ${launchToken}\n`,
    });
  } catch (err) {
    dialog.showErrorBox("SimStudio could not start", String(err.message || err));
    app.quit();
  }
}

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on("second-instance", () => {
    if (mainWindow) {
      if (mainWindow.isMinimized()) mainWindow.restore();
      mainWindow.focus();
    }
  });

  app.whenReady().then(() => {
    buildMenu();
    createWindow();
    app.on("activate", () => {
      if (BrowserWindow.getAllWindows().length === 0) createWindow();
    });
  });

  app.on("window-all-closed", () => app.quit());
  app.on("before-quit", () => {
    quitting = true;
    stopBackend();
  });
  app.on("quit", stopBackend);
}

"use strict";

/**
 * .lightsim project files anywhere on disk (PLT-33), the shell's side.
 *
 * Only the shell names file paths to the engine: it shows the system's Open
 * and Save dialogs, receives files the user double-clicks or drops on the
 * window, and passes the chosen path to the engine with a secret the window
 * never sees (LIGHTSIM_SHELL_TOKEN, see backend/app/security.py). The engine
 * answers with a project id, which is all the window gets. A folder AI tools
 * may see (AI-01) is picked the same way, in the system's folder dialog.
 */

const path = require("node:path");

const SUFFIX = ".lightsim";

/** Whether `file` is named like a LightSim project file. */
function isProjectFile(file) {
  return typeof file === "string" && path.extname(file).toLowerCase() === SUFFIX;
}

/** `file` with the .lightsim extension (a Save dialog may leave it off). */
function withSuffix(file) {
  return isProjectFile(file) ? file : `${file}${SUFFIX}`;
}

/**
 * The .lightsim files named on a command line (a double-click starts the app,
 * or a second instance, with the file's path). Options and the app's own
 * path are skipped; relative paths are taken from `cwd`.
 */
function projectFilesIn(argv, cwd = process.cwd()) {
  return argv
    .filter((a) => typeof a === "string" && !a.startsWith("-") && isProjectFile(a))
    .map((a) => path.resolve(cwd, a));
}

/** A file name the Save dialog can suggest for a project name. */
function suggestedName(name) {
  const clean = String(name || "project")
    .replace(/[<>:"/\\|?*\x00-\x1f]/g, "_")
    .replace(/[. ]+$/, "")
    .trim()
    .slice(0, 100);
  return `${clean || "project"}${SUFFIX}`;
}

/**
 * Calls to the engine's file routes. `origin` is the engine's address,
 * `launchToken` the secret every /api call carries and `shellToken` the one
 * only the shell has.
 */
class EngineFiles {
  constructor(origin, launchToken, shellToken, fetchImpl = fetch) {
    this.origin = origin;
    this.headers = {
      "Content-Type": "application/json",
      Authorization: `Bearer ${launchToken}`,
      "X-LightSim-Shell": shellToken,
    };
    this.fetch = fetchImpl;
  }

  async call(method, route, body) {
    const res = await this.fetch(`${this.origin}${route}`, {
      method,
      headers: this.headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    let data = null;
    try {
      data = await res.json();
    } catch {
      /* no body */
    }
    if (!res.ok) {
      const detail = data && typeof data.detail === "string" ? data.detail : `${res.status} ${res.statusText}`;
      throw new Error(detail);
    }
    return data;
  }

  /** Remember a .lightsim file the user picked; resolves {id, path, name}. */
  open(file) {
    return this.call("POST", "/api/files/open", { path: file });
  }

  /** Remember where to save project `projectId`; resolves {id, path, name, runsMoved}. */
  saveAs(file, projectId) {
    return this.call("POST", "/api/files/save-as", { path: file, projectId });
  }

  /** Recent files: [{id, path, name, exists, …}], most recent first. */
  recent() {
    return this.call("GET", "/api/files");
  }

  /** Let AI tools see the projects in a folder the user picked (AI-01);
   *  resolves the AI access settings as they now are. */
  allowAiFolder(folder) {
    return this.call("POST", "/api/ai/access/folders", { path: folder });
  }
}

module.exports = { EngineFiles, isProjectFile, projectFilesIn, suggestedName, withSuffix, SUFFIX };

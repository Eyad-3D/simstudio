"use strict";

/**
 * Projects saved under the app's old name.
 *
 * Until 0.2.0 the app was called SimStudio. Electron keeps an app's data in a
 * folder named after the product (%APPDATA%\<name> on Windows,
 * ~/.config/<name> on Linux), so LightSim looks in a new folder and would not
 * see the projects saved by SimStudio. On LightSim's first launch this copies
 * SimStudio's projects folder (the projects with their runs/, .backups/ and
 * .hidden-examples) into LightSim's. It copies and never moves or deletes:
 * SimStudio's folder stays exactly as it was.
 */

const fs = require("node:fs");
const path = require("node:path");

const OLD_NAME = "SimStudio";

async function hasEntries(dir) {
  try {
    return (await fs.promises.readdir(dir)).length > 0;
  } catch {
    return false; // missing or unreadable
  }
}

/**
 * Copy <appData>/SimStudio/projects to <userData>/projects, unless the new
 * folder already has something in it (then it does nothing, every time).
 * The copy goes to a side folder first and is renamed into place only when
 * complete, so an interrupted copy is never taken for the user's projects
 * and is simply retried on the next launch.
 *
 * @param {string} appData app.getPath("appData")
 * @param {string} userData app.getPath("userData")
 * @param {(message: string) => void} log
 * @returns {Promise<"copied" | "nothing to copy" | "already has projects" | "failed">}
 */
async function copyOldProjects(appData, userData, log) {
  const from = path.join(appData, OLD_NAME, "projects");
  const to = path.join(userData, "projects");
  if (await hasEntries(to)) return "already has projects";
  if (!(await hasEntries(from))) return "nothing to copy";

  const partial = `${to}.copying`;
  try {
    await fs.promises.rm(partial, { recursive: true, force: true }); // left by a copy cut short
    await fs.promises.cp(from, partial, { recursive: true, preserveTimestamps: true, errorOnExist: true, force: false });
    // an empty folder left by an earlier launch is in the way of the rename
    // (on Windows); rmdir refuses to remove one with anything in it
    await fs.promises.rmdir(to).catch((err) => {
      if (err.code !== "ENOENT") throw err;
    });
    await fs.promises.rename(partial, to);
    log(`Copied the projects saved by ${OLD_NAME} from ${from} to ${to} (${OLD_NAME}'s folder is unchanged).`);
    return "copied";
  } catch (err) {
    log(`Could not copy the projects saved by ${OLD_NAME} from ${from} to ${to}: ${err.message}`);
    await fs.promises.rm(partial, { recursive: true, force: true }).catch(() => {});
    return "failed";
  }
}

module.exports = { copyOldProjects, OLD_NAME };

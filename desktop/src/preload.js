"use strict";

/**
 * What the window may ask of the shell (PLT-33), as window.lightsimDesktop
 * (frontend/src/desktop.ts). Each call opens a system dialog or hands over a
 * file the user dropped; none takes a path from the page, so a page cannot
 * make the shell read or write a file the user did not pick.
 */

const { contextBridge, ipcRenderer, webUtils } = require("electron");

contextBridge.exposeInMainWorld("lightsimDesktop", {
  openFile: () => ipcRenderer.invoke("lightsim:open-file"),
  saveFileAs: (projectId, name) => ipcRenderer.invoke("lightsim:save-file-as", String(projectId), String(name)),
  // a dropped file's place on disk; "" for anything but a real dropped file
  openDroppedFile: (file) => ipcRenderer.invoke("lightsim:open-dropped", webUtils.getPathForFile(file)),
  showFile: (projectId) => ipcRenderer.invoke("lightsim:show-file", String(projectId)),
});

// What the desktop app's shell offers the page (desktop/src/preload.js).
// In a plain browser (development, the browser tests) there is no shell:
// `desktop()` is null and the file commands that need one are hidden.

/** A .lightsim file the shell opened or chose to save to, by the project id
 *  the engine gave it (the page never names paths itself, PLT-33). */
export interface DesktopFile {
  id: string;
  path: string;
  name: string;
  /** Save As only: the runs of a never-saved project moved along */
  runsMoved?: boolean;
}

export interface DesktopBridge {
  /** Show the system's Open dialog; null when the user cancels. */
  openFile: () => Promise<DesktopFile | null>;
  /** Show the system's Save dialog for project `projectId`; null on cancel. */
  saveFileAs: (projectId: string, name: string) => Promise<DesktopFile | null>;
  /** Open a .lightsim file dropped on the window. */
  openDroppedFile: (file: File) => Promise<DesktopFile | null>;
  /** Show a .lightsim file in the system's file manager. */
  showFile: (projectId: string) => Promise<void>;
}

declare global {
  interface Window {
    lightsimDesktop?: DesktopBridge;
    /** called by the shell: open project `id` (File → Open, a double-clicked file) */
    lightsimOpenProjectId?: (id: string) => Promise<boolean>;
    /** called by the shell: File → Open… */
    lightsimOpenFile?: () => Promise<boolean>;
    /** called by the shell: File → Save As… */
    lightsimSaveAs?: () => Promise<boolean>;
  }
}

export function desktop(): DesktopBridge | null {
  return typeof window !== "undefined" ? (window.lightsimDesktop ?? null) : null;
}

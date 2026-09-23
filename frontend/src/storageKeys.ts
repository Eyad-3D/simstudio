// The keys the UI keeps its settings under in localStorage (in the desktop
// app, the window's own storage).
//
// Until 0.2.0 the app was called SimStudio and these keys started with
// "simstudio-". Loading this module renames any such key once, before a store
// reads its setting: the value is copied to the new name unless that one is
// already set, and then the old key is removed.

/** "light" or "dark" (store/uiStore.ts) */
export const THEME_KEY = "lightsim-theme";
/** the UI scale (store/uiStore.ts) */
export const FONT_SCALE_KEY = "lightsim-font-scale";
/** the dock layout (components/DockLayout.tsx) */
export const LAYOUT_KEY = "lightsim-layout-v1";
/** the crash-recovery draft of the working project (persist.ts) */
export const DRAFT_KEY = "lightsim-draft-v1";

const PREFIX = "lightsim-";
const OLD_PREFIX = "simstudio-";

/** Rename every "simstudio-…" key in `storage` to "lightsim-…". An old key
 *  whose value cannot be copied (storage full) is kept for the next start. */
export function migrateOldKeys(storage: Storage): void {
  const old: string[] = [];
  for (let i = 0; i < storage.length; i++) {
    const key = storage.key(i);
    if (key?.startsWith(OLD_PREFIX)) old.push(key);
  }
  for (const key of old) {
    try {
      const renamed = PREFIX + key.slice(OLD_PREFIX.length);
      const value = storage.getItem(key);
      if (value !== null && storage.getItem(renamed) === null) storage.setItem(renamed, value);
      storage.removeItem(key);
    } catch {
      /* not copied: keep the old key */
    }
  }
}

try {
  migrateOldKeys(window.localStorage);
} catch {
  /* storage unavailable: the settings start from their defaults */
}

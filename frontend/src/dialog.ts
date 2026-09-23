// Promise-based helpers for the app's styled confirm/prompt modal — a drop-in
// replacement for window.confirm / window.prompt that matches the app chrome
// (and works in embedded/preview contexts where native dialogs are blocked).
// The modal itself is rendered by <DialogHost/> (mounted in App).

import { useProjectStore } from "./store/projectStore";
import { useUIStore } from "./store/uiStore";

export function confirmDialog(opts: {
  title: string;
  message?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
}): Promise<boolean> {
  return new Promise((resolve) => {
    useUIStore.getState().openDialog({
      kind: "confirm",
      ...opts,
      resolve: (v) => resolve(v === true),
    });
  });
}

export function promptDialog(opts: {
  title: string;
  message?: string;
  defaultValue?: string;
  placeholder?: string;
  confirmLabel?: string;
}): Promise<string | null> {
  return new Promise((resolve) => {
    useUIStore.getState().openDialog({
      kind: "prompt",
      ...opts,
      resolve: (v) => resolve(typeof v === "string" ? v : null),
    });
  });
}

/** Save / Don't save / Cancel. Esc or a click outside counts as Cancel. */
export function unsavedChangesDialog(opts: {
  title: string;
  message?: string;
}): Promise<"save" | "discard" | "cancel"> {
  return new Promise((resolve) => {
    useUIStore.getState().openDialog({
      kind: "confirm",
      ...opts,
      confirmLabel: "Save",
      altLabel: "Don't save",
      cancelLabel: "Cancel",
      resolve: (v) => resolve(v === true ? "save" : v === "alt" ? "discard" : "cancel"),
    });
  });
}

/** Ask before `action` (New, Open, Import) replaces a project with unsaved
 *  changes. Resolves true when it may go ahead: nothing was unsaved, the save
 *  worked, or the user chose not to save. A failed save keeps the project
 *  open; its error is in Messages. */
export async function confirmReplaceProject(action: string): Promise<boolean> {
  const { dirty, project } = useProjectStore.getState();
  if (!dirty || !project) return true;
  const choice = await unsavedChangesDialog({
    title: `Save changes to '${project.name}'?`,
    message: `${action} replaces the open project. Changes you don't save are lost.`,
  });
  if (choice !== "save") return choice === "discard";
  await useProjectStore.getState().saveRemote();
  return !useProjectStore.getState().dirty;
}

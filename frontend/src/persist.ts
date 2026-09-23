// Local autosave of the working project. This is a crash/tab-close safety net
// kept in localStorage; it is separate from "Save" (which persists to the
// backend). The draft is the current working copy and is restored on reload.
// A clean draft only records which project was open: nothing in it is unsaved.

import { DRAFT_KEY } from "./storageKeys";
import type { Project } from "./types";

export interface Draft {
  project: Project;
  savedAt: number;
  /** true when the copy matches what was last saved/opened (no unsaved edits) */
  clean?: boolean;
  /** revision of the project file the copy is based on (null: not on disk) */
  revision?: string | null;
  /** the example the copy was opened from, while it is not saved (its id) */
  example?: string | null;
}

export function saveDraft(
  project: Project,
  clean = false,
  revision?: string | null,
  example?: string | null,
): void {
  try {
    window.localStorage.setItem(
      DRAFT_KEY,
      JSON.stringify({ project, savedAt: Date.now(), clean, revision, example } satisfies Draft),
    );
  } catch {
    /* storage unavailable / quota exceeded — non-fatal */
  }
}

export function loadDraft(): Draft | null {
  try {
    const raw = window.localStorage.getItem(DRAFT_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Draft;
    if (parsed?.project && Array.isArray(parsed.project.systems)) return parsed;
  } catch {
    /* corrupt draft — ignore */
  }
  return null;
}

export function clearDraft(): void {
  try {
    window.localStorage.removeItem(DRAFT_KEY);
  } catch {
    /* storage unavailable */
  }
}

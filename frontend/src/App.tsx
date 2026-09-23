import { useEffect } from "react";
import { DockLayout } from "./components/DockLayout";
import { DialogHost } from "./components/DialogHost";
import { ParameterDialog } from "./components/ParameterDialog";
import { Ribbon } from "./components/Ribbon";
import { StatusBar } from "./components/StatusBar";
import { ResultsPanel } from "./components/panels/ResultsPanel";
import { useProjectStore } from "./store/projectStore";
import { useUIStore } from "./store/uiStore";
import { saveDraft } from "./persist";

declare global {
  interface Window {
    /** Saves the open project; resolves true when nothing is left unsaved. */
    lightsimSave?: () => Promise<boolean>;
  }
}

let initStarted = false;

export default function App() {
  const loaded = useProjectStore((s) => s.loaded);
  const ribbonTab = useUIStore((s) => s.ribbonTab);
  const onResultsPage = ribbonTab === "results";

  useEffect(() => {
    if (!initStarted) {
      initStarted = true;
      void useProjectStore.getState().init();
    }
  }, []);

  // autosave the working project to localStorage (debounced) and flush on
  // tab close, so unsaved work survives a refresh or crash. This is separate
  // from Save (server); see persist.ts. Unsaved work is only ever recorded once
  // the user genuinely edits the project, so returning users aren't told they
  // "restored a draft" they never made.
  useEffect(() => {
    let edited = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const unsub = useProjectStore.subscribe((state, prev) => {
      if (!state.project) return;
      if (!prev.project) {
        // the initial null→project population (init / draft restore): an
        // example opened as a copy is recorded, clean, so that the next launch
        // opens the same copy again, with the runs made on it
        if (state.exampleId && !state.dirty) {
          saveDraft(state.project, true, state.revision, state.exampleId);
        }
        return;
      }
      if (state.project === prev.project && state.dirty === prev.dirty) return;
      edited = true;
      clearTimeout(timer);
      if (state.dirty) {
        timer = setTimeout(() => saveDraft(state.project!, false, state.revision, state.exampleId), 800);
      } else {
        // saved / opened / new: record which project is open, but mark it clean
        // so the next launch doesn't report unsaved work that was already saved
        saveDraft(state.project, true, state.revision, state.exampleId);
      }
    });
    const flush = () => {
      const { project: p, dirty, revision, exampleId } = useProjectStore.getState();
      if (edited && p) saveDraft(p, !dirty, revision, exampleId);
    };
    window.addEventListener("beforeunload", flush);
    return () => {
      unsub();
      clearTimeout(timer);
      window.removeEventListener("beforeunload", flush);
    };
  }, []);

  // closing or reloading with unsaved changes asks first: the browser's
  // leave-page prompt, which the desktop shell turns into Save / Don't save /
  // Cancel (desktop/src/main.js) and answers "Save" through lightsimSave.
  // Leaving anyway still keeps the recovery draft written above.
  useEffect(() => {
    const onBeforeUnload = (e: BeforeUnloadEvent) => {
      if (!useProjectStore.getState().dirty) return;
      e.preventDefault();
      e.returnValue = "";
    };
    window.addEventListener("beforeunload", onBeforeUnload);
    window.lightsimSave = async () => {
      await useProjectStore.getState().saveRemote();
      return !useProjectStore.getState().dirty;
    };
    return () => {
      window.removeEventListener("beforeunload", onBeforeUnload);
      delete window.lightsimSave;
    };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const meta = e.ctrlKey || e.metaKey;
      if (!meta) return;
      const target = e.target as HTMLElement;
      const typing =
        target.tagName === "INPUT" ||
        target.tagName === "TEXTAREA" ||
        target.tagName === "SELECT" ||
        target.isContentEditable;
      const store = useProjectStore.getState();
      if (e.key === "Enter") {
        // Ctrl/Cmd+Enter runs the active case from anywhere (except while
        // editing multi-line code, where Enter belongs to the editor).
        if (target.tagName === "TEXTAREA" || target.isContentEditable) return;
        e.preventDefault();
        if (!store.running) void store.run();
      } else if (e.key.toLowerCase() === "s") {
        e.preventDefault();
        void store.saveRemote();
      } else if (!typing && e.key.toLowerCase() === "z" && !e.shiftKey) {
        e.preventDefault();
        store.undo();
      } else if (!typing && (e.key.toLowerCase() === "y" || (e.key.toLowerCase() === "z" && e.shiftKey))) {
        e.preventDefault();
        store.redo();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="flex h-full flex-col">
      <Ribbon />
      <div className="relative min-h-0 flex-1 p-1">
        {loaded ? (
          <>
            {/* Home / model workspace — kept mounted (hidden on the Results
                page) so its dock layout and live state survive tab switches.
                It stays laid out while hidden, so it follows window resizes;
                `inert` keeps clicks, focus and screen readers out of it. */}
            <div
              className={`absolute inset-1${onResultsPage ? " ss-dock-hidden" : ""}`}
              inert={onResultsPage}
            >
              <DockLayout />
            </div>
            {onResultsPage && (
              <div className="ss-zoom absolute inset-1 overflow-hidden rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)]">
                <ResultsPanel />
              </div>
            )}
          </>
        ) : (
          <div className="flex h-full items-center justify-center text-[13px] text-[color:var(--ss-text-dim)]">
            Loading LightSim…
          </div>
        )}
      </div>
      <StatusBar />
      <ParameterDialog />
      <DialogHost />
    </div>
  );
}

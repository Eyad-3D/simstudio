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
  // from Save (server); see persist.ts. A draft is only ever written once the
  // user genuinely edits/switches the project — a pristine demo never creates
  // one (so returning users aren't told they "restored a draft" they never made).
  useEffect(() => {
    let edited = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const unsub = useProjectStore.subscribe((state, prev) => {
      // skip the initial null→project population (init / draft restore)
      if (!state.project || !prev.project) return;
      if (state.project === prev.project && state.dirty === prev.dirty) return;
      edited = true;
      clearTimeout(timer);
      if (state.dirty) {
        timer = setTimeout(() => saveDraft(state.project!), 800);
      } else {
        // saved / opened / new: record which project is open, but mark it clean
        // so the next launch doesn't report unsaved work that was already saved
        saveDraft(state.project, true);
      }
    });
    const flush = () => {
      const { project: p, dirty } = useProjectStore.getState();
      if (edited && p) saveDraft(p, !dirty);
    };
    window.addEventListener("beforeunload", flush);
    return () => {
      unsub();
      clearTimeout(timer);
      window.removeEventListener("beforeunload", flush);
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
            Loading SimStudio…
          </div>
        )}
      </div>
      <StatusBar />
      <ParameterDialog />
      <DialogHost />
    </div>
  );
}

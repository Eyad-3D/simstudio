// The app's keyboard shortcuts (F1, Ctrl+S, Ctrl+Z, …), heard in the main
// window and in every part's parameter window (components/ParamWindows.tsx),
// so an undo typed in a parameter window undoes the project's last change.
import { desktop } from "./desktop";
import { componentHelpPage, openHelp } from "./help";
import { confirmReplaceProject, useProjectStore } from "./store/projectStore";

export function onAppKey(e: KeyboardEvent): void {
  // the key's own window: the main one or a parameter window
  const target = e.target as HTMLElement;
  const doc = target.ownerDocument ?? document;
  if (e.key === "F1") {
    // the parameter in the open help card, else the focused parameter's
    // help, else the selected part's help page, else the help of the
    // panel or page the focus is in, else the help's front page
    e.preventDefault();
    const { project, selectedElementId } = useProjectStore.getState();
    const el = project?.systems.flatMap((s) => s.elements).find((x) => x.id === selectedElementId);
    // (an element of another window is not an `instanceof Element` here)
    const at = doc.querySelector(".ss-help-card:popover-open") ?? (typeof target.closest === "function" ? target : null);
    const param = at?.closest("[data-help]")?.getAttribute("data-help");
    const panel = at?.closest("[data-help-panel]")?.getAttribute("data-help-panel");
    openHelp(param ?? (el ? componentHelpPage(el.componentDefId) : (panel ?? undefined)));
    return;
  }
  const meta = e.ctrlKey || e.metaKey;
  if (!meta) return;
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
  } else if (e.key.toLowerCase() === "s" && e.shiftKey && desktop()) {
    e.preventDefault();
    void store.saveAs();
  } else if (e.key.toLowerCase() === "s") {
    e.preventDefault();
    void store.saveRemote();
  } else if (e.key.toLowerCase() === "o" && desktop()) {
    e.preventDefault();
    void confirmReplaceProject("Opening a project file").then((ok) => {
      if (ok) void store.openFile();
    });
  } else if (!typing && e.key.toLowerCase() === "z" && !e.shiftKey) {
    e.preventDefault();
    store.undo();
  } else if (!typing && (e.key.toLowerCase() === "y" || (e.key.toLowerCase() === "z" && e.shiftKey))) {
    e.preventDefault();
    store.redo();
  }
}

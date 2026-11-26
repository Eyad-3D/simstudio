// Dockview closes a focused panel tab on Delete or Backspace (its tab list's
// own keydown handler). In LightSim those keys delete parts on the diagram, so
// a user who clicked a tab and then pressed Delete lost the panel instead.
// Caught here, on the dock, before dockview sees them: on a tab they do nothing.
// (Of the tab list's other keys, the arrows, Home and End move between tabs
// and Enter and Space show one; none of them closes anything.)

const editable = (el: Element) =>
  el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || (el as HTMLElement).isContentEditable;

export function keepTabsOnDelete(root: HTMLElement): () => void {
  const onKey = (e: KeyboardEvent) => {
    if (e.key !== "Delete" && e.key !== "Backspace") return;
    const t = e.target;
    if (!(t instanceof Element) || editable(t) || !t.closest(".dv-tab")) return;
    e.stopPropagation();
    e.preventDefault();
  };
  root.addEventListener("keydown", onKey, true);
  return () => root.removeEventListener("keydown", onKey, true);
}

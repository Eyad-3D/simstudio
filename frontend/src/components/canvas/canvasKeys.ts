// The model diagram's keyboard shortcuts (UX-19): which key does what, and
// when a key belongs to the diagram rather than to what has the focus.
// Plain functions, so they unit-test without a browser.

export type CanvasAction = "selectAll" | "copy" | "cut" | "paste" | "duplicate" | "delete" | "rename" | "properties";

type Keys = Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey">;

/** The diagram's action for a key press (Ctrl on Windows and Linux, Cmd on
 *  a Mac), or null. */
export function canvasAction(e: Keys): CanvasAction | null {
  if (e.altKey) return null;
  if (e.ctrlKey || e.metaKey) {
    if (e.shiftKey) return null;
    switch (e.key.toLowerCase()) {
      case "a":
        return "selectAll";
      case "c":
        return "copy";
      case "x":
        return "cut";
      case "v":
        return "paste";
      case "d":
        return "duplicate";
    }
    return null;
  }
  switch (e.key) {
    case "Delete":
    case "Backspace":
      return "delete";
    case "F2":
      return e.shiftKey ? null : "rename";
    case "Enter":
      return e.shiftKey ? null : "properties";
  }
  return null;
}

/** True when the key belongs to the focused element: text being typed in
 *  a field, a list or an editor, and Enter on a button or a link (which
 *  presses it). */
export function keyBelongsToTarget(target: Element | null, action: CanvasAction): boolean {
  if (!target) return false;
  if ((target as HTMLElement).isContentEditable) return true;
  if (/^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName)) return true;
  return (
    action === "properties" &&
    !!target.closest("button, a[href], summary, [role=button], [role=link], [role=menuitem], [role=option], [role=tab]")
  );
}

/** True when the diagram has the keyboard: the focus is in it, or nowhere
 *  (the page itself) while the diagram was the last place clicked or the
 *  pointer is over it. */
export function diagramHasKeys(
  root: Element | null,
  focused: Element | null,
  body: Element | null,
  lastClickInside: boolean,
  hovered: boolean,
): boolean {
  if (!root) return false;
  if (focused && focused !== body) return root.contains(focused);
  return lastClickInside || hovered;
}

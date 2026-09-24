import { useEffect, type RefObject } from "react";
import { useUIStore } from "../store/uiStore";

/** Closes an open menu the way users expect: on Esc (focus goes back to
 *  `button`, the one that opened it), on a press anywhere outside `ref`, and
 *  when a modal dialog opens over it. The press is caught in the capture
 *  phase: React Flow stops the mousedown on the diagram before it reaches
 *  the window, so a plain listener never heard a click there. */
export function useDismiss(
  open: boolean,
  close: () => void,
  ref: RefObject<HTMLElement | null>,
  button?: RefObject<HTMLElement | null>,
) {
  const modal = useUIStore((s) => s.paramDialogId !== null || s.dialog !== null);
  useEffect(() => {
    if (open && modal) close();
  }, [open, modal, close]);

  useEffect(() => {
    if (!open) return;
    const onPointer = (e: PointerEvent) => {
      if (!ref.current?.contains(e.target as Node)) close();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      close();
      button?.current?.focus();
    };
    window.addEventListener("pointerdown", onPointer, true);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("keydown", onKey);
    };
  }, [open, close, ref, button]);
}

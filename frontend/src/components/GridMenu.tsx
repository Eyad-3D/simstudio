// The right-click menu of a table grid (SpreadsheetGrid): a short list of
// actions at the pointer. An action that needs a number ("Multiply by…")
// turns the menu into a one-line field; Enter applies it, Esc closes.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useOwnerWindow } from "../ownerWindow";

export interface GridMenuItem {
  label: string;
  disabled?: boolean;
  /** asks for a number first: the field's label */
  ask?: string;
  /** does it; a message keeps the field open and says what is wrong */
  run: (text: string) => string | void;
}

export function GridMenu({
  x,
  y,
  label,
  items,
  onClose,
}: {
  /** where, in the menu's positioned parent */
  x: number;
  y: number;
  label: string;
  items: GridMenuItem[];
  onClose: (refocus: boolean) => void;
}) {
  const win = useOwnerWindow();
  const ref = useRef<HTMLDivElement>(null);
  const [askingLabel, setAsking] = useState<string | null>(null);
  // (looked up each time: the items are made for the selection as it is now)
  const asking = items.find((it) => it.label === askingLabel) ?? null;
  const [text, setText] = useState("");
  const [problem, setProblem] = useState("");

  useEffect(() => {
    ref.current?.querySelector<HTMLElement>("[role=menuitem]:not([disabled]), input")?.focus();
  }, [askingLabel]);
  // kept inside what clips it (a dialog, a panel, the window): moved left or up
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.transform = "";
    let clip = el.parentElement;
    while (clip && win.getComputedStyle(clip).overflow === "visible") clip = clip.parentElement;
    const bound = clip?.getBoundingClientRect() ?? { right: win.innerWidth, bottom: win.innerHeight };
    const r = el.getBoundingClientRect();
    const dx = Math.min(0, Math.min(bound.right, win.innerWidth) - 4 - r.right);
    const dy = Math.min(0, Math.min(bound.bottom, win.innerHeight) - 4 - r.bottom);
    if (dx || dy) el.style.transform = `translate(${dx}px, ${dy}px)`;
  }, [x, y, askingLabel, win]);
  useEffect(() => {
    // a press anywhere else closes it (caught first: the grid takes its own presses)
    const away = (e: PointerEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose(false);
    };
    win.addEventListener("pointerdown", away, true);
    return () => win.removeEventListener("pointerdown", away, true);
  }, [win, onClose]);

  const choose = (item: GridMenuItem) => {
    if (item.ask) {
      setAsking(item.label);
      return;
    }
    item.run("");
    onClose(true);
  };
  const apply = () => {
    const bad = asking?.run(text.trim());
    if (bad) setProblem(bad);
    else onClose(true);
  };
  const onKeyDown = (e: React.KeyboardEvent) => {
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      onClose(true);
      return;
    }
    if (asking || (e.key !== "ArrowDown" && e.key !== "ArrowUp")) return;
    e.preventDefault();
    const all = [...(ref.current?.querySelectorAll<HTMLElement>("[role=menuitem]:not([disabled])") ?? [])];
    const at = all.indexOf(win.document.activeElement as HTMLElement);
    all[(at + (e.key === "ArrowDown" ? 1 : all.length - 1)) % all.length]?.focus();
  };

  const item =
    "block w-full px-3 py-1 text-left text-body text-[color:var(--ss-text)] hover:bg-[color:var(--ss-accent-soft)] focus:bg-[color:var(--ss-accent-soft)] focus:outline-none disabled:opacity-40 disabled:hover:bg-transparent";
  return (
    <div
      ref={ref}
      role={asking ? "dialog" : "menu"}
      aria-label={asking ? asking.label : label}
      onKeyDown={onKeyDown}
      onContextMenu={(e) => e.preventDefault()}
      className="ss-grid-menu absolute z-50 min-w-[190px] rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] py-1 shadow-lg"
      style={{ left: x, top: y }}
    >
      {asking ? (
        <form
          className="flex flex-col gap-1 px-2 py-1"
          onSubmit={(e) => {
            e.preventDefault();
            apply();
          }}
        >
          <label className="flex flex-col gap-1 whitespace-nowrap text-body text-[color:var(--ss-text)]">
            {asking.ask}
            <input
              className="ss-input w-full py-0.5 text-body"
              value={text}
              aria-invalid={problem ? true : undefined}
              onChange={(e) => {
                setText(e.target.value);
                setProblem("");
              }}
            />
          </label>
          {problem && <span className="text-meta text-[color:var(--ss-err)]">{problem}</span>}
          <div className="flex justify-end gap-1">
            <button type="button" className="ss-toolbtn" onClick={() => onClose(true)}>
              Cancel
            </button>
            <button type="submit" className="ss-toolbtn">
              OK
            </button>
          </div>
        </form>
      ) : (
        items.map((it) => (
          <button key={it.label} type="button" role="menuitem" className={item} disabled={it.disabled} onClick={() => choose(it)}>
            {it.label}
          </button>
        ))
      )}
    </div>
  );
}

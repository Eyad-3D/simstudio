// The add-a-part box: a small search field at a spot on the diagram (a
// double-click on empty space, Shift+A, or a wire let go there), with the
// parts it finds under it. Arrows move, Enter places the part, Esc or a
// press elsewhere closes it and leaves nothing behind.
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { componentIcon } from "../../icons";
import type { ComponentDef, PortDef } from "../../types";
import { partsFor, wirePort } from "./addPart";

/** Rows shown before the list scrolls. */
const SHORT = 8;

export function AddPartBox({
  x,
  y,
  library,
  from,
  onPick,
  onClose,
}: {
  /** where it opens, in its positioned parent (the diagram) */
  x: number;
  y: number;
  library: ComponentDef[];
  /** the port a dragged wire comes from: only parts it may connect to */
  from?: PortDef | null;
  onPick: (def: ComponentDef) => void;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const parts = useMemo(() => partsFor(library, query, from), [library, query, from]);
  const at = Math.min(active, Math.max(0, parts.length - 1));

  useEffect(() => {
    // a press anywhere else closes it (caught first: the diagram takes its own presses)
    const away = (e: PointerEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    window.addEventListener("pointerdown", away, true);
    return () => window.removeEventListener("pointerdown", away, true);
  }, [onClose]);
  // kept inside the diagram: moved left or up when it would run past an
  // edge; then the field takes the keyboard without scrolling anything (a
  // box over the edge made the panel scroll sideways and the diagram re-fit)
  useLayoutEffect(() => {
    const el = ref.current;
    const box = el?.offsetParent?.getBoundingClientRect();
    if (!el || !box) return;
    el.style.transform = "";
    const r = el.getBoundingClientRect();
    // (never above the diagram's top, where its search field would be lost)
    const dx = Math.max(box.left + 4 - r.left, Math.min(0, box.right - 4 - r.right));
    const dy = Math.max(box.top + 4 - r.top, Math.min(0, box.bottom - 4 - r.bottom));
    el.style.transform = `translate(${dx}px, ${dy}px)`;
  }, [parts.length]);
  useLayoutEffect(() => ref.current?.querySelector("input")?.focus({ preventScroll: true }), []);
  // the row the arrows are on stays in the list's view (the list only: not the panel)
  useEffect(() => {
    const li = ref.current?.querySelector<HTMLElement>(`[data-index="${at}"]`);
    const ul = li?.parentElement;
    if (!li || !ul) return;
    if (li.offsetTop < ul.scrollTop) ul.scrollTop = li.offsetTop;
    else if (li.offsetTop + li.offsetHeight > ul.scrollTop + ul.clientHeight)
      ul.scrollTop = li.offsetTop + li.offsetHeight - ul.clientHeight;
  }, [at]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    // the diagram's own keys (Delete, Enter, Ctrl+A…) must not see these
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (parts.length) setActive((at + (e.key === "ArrowDown" ? 1 : parts.length - 1)) % parts.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (parts[at]) onPick(parts[at]);
    }
  };

  const label = from ? `Add a part wired to ${from.name}` : "Add a part";
  return (
    <div
      ref={ref}
      data-add-part
      className="nokey absolute z-30 w-[230px] rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] shadow-lg"
      style={{ left: x, top: y }}
      onKeyDown={onKeyDown}
      onDoubleClick={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.preventDefault()}
    >
      <input
        className="ss-input w-full rounded-b-none border-0 border-b border-[color:var(--ss-border)] text-[12px]"
        placeholder={from ? "Part to connect…" : "Add a part…"}
        aria-label={label}
        role="combobox"
        aria-expanded
        aria-controls="ss-add-part-list"
        aria-activedescendant={parts[at] ? `ss-add-part-${parts[at].id}` : undefined}
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          setActive(0);
        }}
      />
      <ul
        id="ss-add-part-list"
        role="listbox"
        aria-label={label}
        className="relative overflow-y-auto py-0.5"
        style={{ maxHeight: SHORT * 26 }}
      >
        {parts.map((def, i) => {
          const Icon = componentIcon(def.icon);
          const port = from ? def.ports.find((p) => p.id === wirePort(def, from)) : null;
          return (
            <li
              key={def.id}
              id={`ss-add-part-${def.id}`}
              data-index={i}
              role="option"
              aria-selected={i === at}
              className={`flex h-[26px] cursor-pointer items-center gap-2 px-2 text-body text-[color:var(--ss-text)] ${
                i === at ? "bg-[color:var(--ss-accent-soft)]" : "hover:bg-[color:var(--ss-hover)]"
              }`}
              onMouseEnter={() => setActive(i)}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => onPick(def)}
            >
              <Icon size={14} strokeWidth={1.6} className="shrink-0 text-[color:var(--ss-node-icon)]" />
              <span className="truncate">{def.name}</span>
              {port && <span className="ml-auto truncate text-meta text-[color:var(--ss-text-dim)]">{port.name}</span>}
            </li>
          );
        })}
        {parts.length === 0 && (
          <li className="px-2 py-1 text-meta text-[color:var(--ss-text-dim)]">
            {from && !query ? `No part has a port this wire can join.` : `No part matches “${query}”.`}
          </li>
        )}
      </ul>
    </div>
  );
}

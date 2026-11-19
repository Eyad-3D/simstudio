// The Help menu in the header (LRN-09): the help's main pages, opened in the
// Help panel, the first-steps tour (UX-26), and where to report a problem.
// The desktop app's own Help menu opens the same pages (desktop/src/main.js).
import { useRef, useState } from "react";
import { CircleHelp } from "lucide-react";
import { HELP_MENU, REPORT_PROBLEM_URL, openHelp } from "../help";
import { startTour } from "../tour";
import { useDismiss } from "./useDismiss";

export function HelpMenu() {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  useDismiss(open, () => setOpen(false), ref, button);

  const choose = (fn: () => void) => {
    setOpen(false);
    fn();
  };
  // up and down move between the items, as in a native menu
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const items = [...(menu.current?.querySelectorAll<HTMLElement>("[role=menuitem]") ?? [])];
    const at = items.indexOf(document.activeElement as HTMLElement);
    items[(at + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length]?.focus();
  };

  const item = "block w-full px-3 py-1.5 text-left text-[12px] text-[color:var(--ss-text)] hover:bg-[color:var(--ss-accent-soft)] focus:bg-[color:var(--ss-accent-soft)]";
  return (
    <div ref={ref} className="relative">
      <button
        ref={button}
        className="rounded p-1 hover:bg-[color:var(--ss-hover)]"
        title="Help (F1 opens the help on what is selected)"
        aria-label="Help"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => {
          setOpen(!open);
          // the first item takes the focus once the menu is drawn
          requestAnimationFrame(() => menu.current?.querySelector<HTMLElement>("[role=menuitem]")?.focus());
        }}
      >
        <CircleHelp size={13} />
      </button>
      {open && (
        <div
          ref={menu}
          role="menu"
          aria-label="Help"
          onKeyDown={onKeyDown}
          className="absolute right-0 top-[26px] z-50 w-[260px] rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] py-1 shadow-lg"
        >
          {HELP_MENU.map(({ label, page }) => (
            <button key={page} role="menuitem" className={item} onClick={() => choose(() => openHelp(page))}>
              {label}
            </button>
          ))}
          <div className="my-1 border-t border-[color:var(--ss-border)]" />
          <button role="menuitem" className={item} onClick={() => choose(startTour)}>
            Show the first-steps tour
          </button>
          <button
            role="menuitem"
            className={item}
            onClick={() => choose(() => window.open(REPORT_PROBLEM_URL, "_blank", "noopener"))}
          >
            Report a problem…
          </button>
        </div>
      )}
    </div>
  );
}

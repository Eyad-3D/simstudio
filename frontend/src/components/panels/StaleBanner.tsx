// "These results are from before 3 changes" (UX-41): the model on screen
// has changed since the run shown in Results was made.
import { useMemo, useState } from "react";
import { History, Play } from "lucide-react";
import { useProjectStore } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { changesSince } from "../../staleness";
import type { SimRun } from "../../types";

export function useStaleness(run: SimRun | null) {
  const project = useProjectStore((s) => s.project);
  const lib = useProjectStore((s) => s.libraryById);
  return changesSince(run, project, lib);
}

export function StaleBanner({ run }: { run: SimRun }) {
  const { changes } = useStaleness(run);
  const running = useProjectStore((s) => s.running);
  const activeCaseId = useProjectStore((s) => s.activeCaseId);
  const setActiveCase = useProjectStore((s) => s.setActiveCase);
  const runCase = useProjectStore((s) => s.run);
  const project = useProjectStore((s) => s.project);
  const inProject = useMemo(() => new Set(project?.systems.flatMap((sy) => sy.elements.map((e) => e.id))), [project]);
  const [open, setOpen] = useState(false);
  if (changes.length === 0) return null;
  const n = changes.length;
  const show = (elementId: string) => {
    const ui = useUIStore.getState();
    ui.setRibbonTab("home");
    ui.focusPanel("topology");
    ui.revealElements?.([elementId]);
  };
  const rerun = () => {
    if (activeCaseId !== run.caseId) setActiveCase(run.caseId);
    void runCase();
  };
  return (
    <div
      role="status"
      aria-label="Results out of date"
      className="shrink-0 border-b border-[color:var(--ss-border)] px-2 py-1 text-[11px]"
      style={{ background: "color-mix(in srgb, var(--ss-warn) 10%, var(--ss-panel))" }}
    >
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
        <History size={13} className="shrink-0 text-[color:var(--ss-warn)]" />
        <span>
          These results are from before {n} change{n === 1 ? "" : "s"} to the model.
        </span>
        <button className="ss-toolbtn border border-[color:var(--ss-border)] py-0" disabled={running} onClick={rerun} title={`Run '${run.caseName}' again with the model as it is now`}>
          <Play size={11} className="text-[color:var(--ss-accent)]" /> Re-run
        </button>
        <button className="text-[color:var(--ss-accent)] hover:underline" aria-expanded={open} onClick={() => setOpen(!open)}>
          {open ? "Hide changes" : "Show changes"}
        </button>
      </div>
      {open && (
        <ul className="mt-1 max-h-[96px] overflow-y-auto pl-5">
          {changes.map((c, i) => (
            <li key={i} className="list-disc break-words">
              {c.elementId && inProject.has(c.elementId) ? (
                <button className="text-left text-[color:var(--ss-accent)] hover:underline" title="Show this part on the diagram" onClick={() => show(c.elementId!)}>
                  {c.text}
                </button>
              ) : (
                c.text
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

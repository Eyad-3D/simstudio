import { ListChecks, Loader2 } from "lucide-react";
import { countOf, useProblems, useProjectStore, type Problem } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { levelIcon } from "./MessagesPanel";

/** The Problems list: the latest Data Checks (re-run by themselves after
 *  every change) and the latest run's warnings and errors. A row selects the
 *  parts it is about and pans and zooms the diagram to them. */
export function DataChecksPanel() {
  const problems = useProblems();
  const dataChecks = useProjectStore((s) => s.dataChecks);
  const checking = useProjectStore((s) => s.checking);
  const runDataChecks = useProjectStore((s) => s.runDataChecks);
  const project = useProjectStore((s) => s.project);
  const labels = new Map(project?.systems.flatMap((s) => s.elements.map((e) => [e.id, e.label])) ?? []);

  const show = (p: Problem) => {
    const ui = useUIStore.getState();
    if (ui.ribbonTab !== "home") ui.setRibbonTab("home");
    ui.focusPanel("topology");
    ui.revealElements?.(p.elementIds);
  };

  const errors = problems.filter((p) => p.level === "error").length;
  const warnings = problems.filter((p) => p.level === "warning").length;

  return (
    <div className="flex h-full flex-col">
      <div className="ss-panel-toolbar">
        <button
          className="ss-toolbtn border border-[color:var(--ss-border)]"
          disabled={checking}
          onClick={() => void runDataChecks()}
        >
          {checking ? <Loader2 size={13} className="animate-spin" /> : <ListChecks size={13} />}
          Run Data Checks
        </button>
        <span className="ml-2 text-[11px] text-[color:var(--ss-text-dim)]">
          {countOf(errors, "error")}, {countOf(warnings, "warning")}
        </span>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {!dataChecks && problems.length === 0 && (
          <div className="px-3 py-2 text-[12px] text-[color:var(--ss-text-dim)]">
            Not checked yet. The model is checked by itself a moment after it opens or changes (unconnected
            parts, missing signals, parameter ranges, power sources).
          </div>
        )}
        <ul>
          {problems.map((p, i) => {
            const parts = p.elementIds.map((id) => labels.get(id)).join(", ");
            return (
              <li key={i} className="border-b border-[color:var(--ss-td-border)]">
                <button
                  className="flex w-full items-start gap-2 px-2 py-1 text-left text-[12px] enabled:hover:bg-[color:var(--ss-hover)]"
                  disabled={!parts} // about the project, not a part
                  onClick={() => show(p)}
                  title={parts ? `Show ${parts} on the diagram` : undefined}
                >
                  <span className="mt-[2px]">{levelIcon(p.level)}</span>
                  <span className="w-[150px] shrink-0 truncate text-[11px] font-medium">{parts || "—"}</span>
                  <span className="min-w-0 flex-1">
                    {p.text}
                    {p.fix && (
                      <span className="block text-[11px] text-[color:var(--ss-text-dim)]">How to fix: {p.fix}</span>
                    )}
                  </span>
                  <span className="shrink-0 text-[10px] text-[color:var(--ss-text-dim)]">
                    {p.source === "check"
                      ? "Data Checks"
                      : `Run ${p.source.caseName} · ${new Date(p.source.startedAt).toLocaleTimeString()}`}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      </div>
    </div>
  );
}

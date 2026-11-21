import { ListChecks, Loader2 } from "lucide-react";
import { countOf, useProblems, useProjectStore, type Problem } from "../../store/projectStore";
import { useUIStore } from "../../store/uiStore";
import { levelIcon } from "./MessagesPanel";

/** The Problems list: the latest Data Checks (re-run by themselves after
 *  every change) and the latest run's warnings and errors, or, while the
 *  engine refuses to read the model, the values it refused. A row selects the
 *  parts it is about and pans and zooms the diagram to them; a row about one
 *  parameter also opens the part's window at it. */
export function DataChecksPanel() {
  const problems = useProblems();
  const dataChecks = useProjectStore((s) => s.dataChecks);
  const checksError = useProjectStore((s) => s.checksError);
  const refused = useProjectStore((s) => s.refused);
  const checking = useProjectStore((s) => s.checking);
  const runDataChecks = useProjectStore((s) => s.runDataChecks);
  const project = useProjectStore((s) => s.project);
  const labels = new Map(project?.systems.flatMap((s) => s.elements.map((e) => [e.id, e.label])) ?? []);

  const show = (p: Problem) => {
    const ui = useUIStore.getState();
    if (ui.ribbonTab !== "home") ui.setRibbonTab("home");
    ui.focusPanel("topology");
    ui.revealElements?.(p.elementIds);
    if (p.paramKey && p.elementIds.length) ui.openParamDialog(p.elementIds[0], p.paramKey);
  };

  const seen = new Map<string, number>();
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
          {refused
            ? `The engine refused the model: ${countOf(refused.length, "problem")}`
            : checksError && !dataChecks
              ? "Not checked"
              : `${countOf(errors, "error")}, ${countOf(warnings, "warning")}`}
        </span>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {checksError && (
          <div role="alert" className="flex items-start gap-2 border-b border-[color:var(--ss-td-border)] px-2 py-1.5 text-[12px]">
            <span className="mt-[2px]">{levelIcon("warning")}</span>
            <span className="min-w-0 flex-1">
              The Data Checks could not run: {checksError}
              <span className="block text-[11px] text-[color:var(--ss-text-dim)]">
                {dataChecks
                  ? "The rows below are from the last check that ran. "
                  : "Nothing has been checked yet, so no problems are listed. "}
                They run again by themselves after the next change, or press Run Data Checks.
              </span>
            </span>
          </div>
        )}
        {!dataChecks && !checksError && problems.length === 0 && (
          <div className="px-3 py-2 text-[12px] text-[color:var(--ss-text-dim)]">
            Not checked yet. The model is checked by itself a moment after it opens or changes (unconnected
            parts, missing signals, parameter ranges, power sources).
          </div>
        )}
        <ul>
          {problems.map((p) => {
            const parts = p.elementIds.map((id) => labels.get(id)).join(", ");
            // keyed by content, so a re-check that adds a row keeps focus on the same problem
            const key = `${typeof p.source === "string" ? p.source : "run"}:${p.text}:${p.elementIds}`;
            const n = (seen.get(key) ?? 0) + 1;
            seen.set(key, n);
            return (
              <li key={`${key}:${n}`} className="border-b border-[color:var(--ss-td-border)]">
                <button
                  className="flex w-full items-start gap-2 px-2 py-1 text-left text-[12px] enabled:hover:bg-[color:var(--ss-hover)]"
                  disabled={!parts} // about the project, not a part
                  onClick={() => show(p)}
                  title={
                    parts
                      ? p.paramKey
                        ? `Open ${p.elementIds.map((id) => labels.get(id))[0]} at this value`
                        : `Show ${parts} on the diagram`
                      : undefined
                  }
                >
                  <span className="mt-[2px]">{levelIcon(p.level)}</span>
                  <span className="w-[150px] shrink-0 truncate text-[11px] font-medium">{parts || "—"}</span>
                  <span className="min-w-0 flex-1">
                    {p.text}
                    {p.fix && (
                      <span className="block text-[11px] text-[color:var(--ss-text-dim)]">How to fix: {p.fix}</span>
                    )}
                  </span>
                  <span className="shrink-0 text-right text-[10px] text-[color:var(--ss-text-dim)]">
                    {p.source === "check" ? (
                      "Data Checks"
                    ) : p.source === "refused" ? (
                      <span title={p.code}>Engine refused</span>
                    ) : (
                      <>
                        Last run: {p.source.caseName} · {new Date(p.source.startedAt).toLocaleTimeString()}
                        {p.source.changedSince > 0 && (
                          <span
                            className="block text-[color:var(--ss-warn)]"
                            title="The model has changed since this run: the problem may be fixed already. Run again to check."
                          >
                            model changed since: run again to check
                          </span>
                        )}
                      </>
                    )}
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

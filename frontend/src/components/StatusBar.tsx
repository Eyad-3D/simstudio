import { CircleAlert, CircleCheck, CloudOff, Loader2, Plus, TriangleAlert, X } from "lucide-react";
import { confirmReplaceProject, followNotice, problemCounts, useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";
import { progressText } from "../sweep";
import { useNow } from "./useNow";

const NOTICE_ICON = { info: CircleCheck, warning: TriangleAlert, error: CircleAlert } as const;
const NOTICE_COLOR = {
  info: "text-[color:var(--ss-ok)]",
  warning: "text-[color:var(--ss-warn)]",
  error: "text-[color:var(--ss-err)]",
} as const;

/** How the latest run, sweep or study ended (UX-21), over the right end of
 *  the status bar: the page stays where the user is, and the notice's button
 *  goes to what it made. It stays until closed, followed or the next run;
 *  going to the Results page puts a notice of results away. */
function FinishNotice() {
  const notice = useProjectStore((s) => s.finishNotice);
  const dismiss = useProjectStore((s) => s.dismissFinishNotice);
  const onResults = useUIStore((s) => s.ribbonTab === "results");
  const resultsAfterRun = useUIStore((s) => s.resultsAfterRun);
  if (!notice) return null;
  const Icon = NOTICE_ICON[notice.level];
  const label = notice.show === "results" ? "Show results" : notice.show === "cases" ? "Show in Cases" : "Show messages";
  return (
    <div
      role={notice.level === "error" ? "alert" : "status"}
      aria-label="Run finished"
      className="fixed bottom-[32px] right-3 z-50 w-[330px] max-w-[calc(100vw-24px)] rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] p-2 text-[12px] shadow-lg"
    >
      <div className="flex items-start gap-1.5">
        <Icon size={15} className={`mt-px shrink-0 ${NOTICE_COLOR[notice.level]}`} aria-hidden />
        <span className="min-w-0 flex-1 break-words">{notice.text}</span>
        <button
          className="shrink-0 rounded p-0.5 text-[color:var(--ss-text-dim)] hover:bg-[color:var(--ss-hover)]"
          title="Close"
          aria-label="Close"
          onClick={dismiss}
        >
          <X size={13} />
        </button>
      </div>
      <div className="mt-1.5 flex flex-wrap items-center gap-1.5 pl-[21px]">
        {!(onResults && notice.show === "results") && (
          <button className="ss-toolbtn border border-[color:var(--ss-border)]" onClick={() => followNotice("show")}>
            {label}
          </button>
        )}
        {notice.studyId && (
          <button
            className="ss-toolbtn border border-[color:var(--ss-border)]"
            title="Each result against the swept value, from the saved study (Results → Study)"
            onClick={() => followNotice("study")}
          >
            Study charts
          </button>
        )}
        {notice.show === "results" && (
          <label
            className="ml-auto flex items-center gap-1 text-[11px] text-[color:var(--ss-text-dim)]"
            title="Open the Results page by itself when a run or sweep ends"
          >
            <input
              type="checkbox"
              checked={resultsAfterRun}
              onChange={(e) => useUIStore.getState().setResultsAfterRun(e.target.checked)}
            />
            Always open Results
          </label>
        )}
      </div>
    </div>
  );
}

export function StatusBar() {
  const project = useProjectStore((s) => s.project);
  const offline = useProjectStore((s) => s.offline);
  const running = useProjectStore((s) => s.running);
  const livePct = useProjectStore((s) => s.livePct);
  const liveT = useProjectStore((s) => s.liveT);
  const sweep = useProjectStore((s) => s.sweepProgress);
  const dirty = useProjectStore((s) => s.dirty);
  const newProject = useProjectStore((s) => s.newProject);
  // the errors in the Problems list (the latest Data Checks, re-checked as the
  // model changes, and the latest run), not every error ever logged: Messages
  // keeps those
  const errors = useProjectStore((s) => problemCounts(s).errors);
  const elementCount =
    project?.systems.reduce((n, s) => n + s.elements.length, 0) ?? 0;
  // a sweep's time left counts down between its points (STU-17)
  const now = useNow(1000, Boolean(running && sweep));

  return (
    <div className="ss-zoom flex h-[26px] shrink-0 items-center border-t border-[color:var(--ss-border)] bg-[color:var(--ss-chrome)] text-[11px]">
      <div className="flex h-full items-end gap-0.5 px-1.5">
        {project && (
          <div className="flex h-[22px] items-center gap-2 rounded-t border border-b-0 border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] px-3 font-medium">
            {project.name}
            {dirty && <span className="text-[color:var(--ss-accent)]">•</span>}
          </div>
        )}
        <button
          className="mb-0.5 rounded p-0.5 hover:bg-[color:var(--ss-hover)]"
          title="New project"
          onClick={() =>
            void confirmReplaceProject("Creating a new project").then((ok) => {
              if (!ok) return;
              newProject();
              // from the Start page, show the new diagram
              const ui = useUIStore.getState();
              if (ui.ribbonTab === "start") ui.setRibbonTab("home");
            })
          }
        >
          <Plus size={13} />
        </button>
      </div>
      <div className="ml-auto flex items-center gap-3 px-3 text-[color:var(--ss-text-dim)]">
        {running && (
          <span className="flex items-center gap-1 text-[color:var(--ss-accent)]">
            <Loader2 size={12} className="animate-spin" />
            {sweep ? (
              <span title="The points run side by side; the time left follows the pace of those that ended">
                sweep: {progressText(sweep, Math.max(now, sweep.lastAt ?? 0))}
              </span>
            ) : (
              <>
                solving… t = {liveT.toFixed(0)} s ({livePct.toFixed(0)} %)
              </>
            )}
            <span className="ml-1 inline-block h-[6px] w-[90px] overflow-hidden rounded bg-[color:var(--ss-active)]">
              <span
                className="block h-full bg-[color:var(--ss-accent)] transition-[width]"
                style={{ width: `${livePct}%` }}
              />
            </span>
          </span>
        )}
        {errors > 0 && (
          <button
            className="text-[color:var(--ss-err)] hover:underline"
            title="Show the problems"
            onClick={() => {
              const ui = useUIStore.getState();
              if (ui.ribbonTab === "results") ui.setRibbonTab("home");
              ui.focusPanel("data-checks");
            }}
          >
            {errors} {errors === 1 ? "error" : "errors"}
          </button>
        )}
        <span>{elementCount} elements</span>
        {offline ? (
          <span className="flex items-center gap-1 text-[color:var(--ss-warn)]">
            <CloudOff size={12} /> backend offline
          </span>
        ) : (
          <span className="text-[color:var(--ss-ok)]">backend connected</span>
        )}
      </div>
      <FinishNotice />
    </div>
  );
}

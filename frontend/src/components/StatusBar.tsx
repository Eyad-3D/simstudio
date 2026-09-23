import { CloudOff, Loader2, Plus } from "lucide-react";
import { confirmReplaceProject, useProjectStore } from "../store/projectStore";

export function StatusBar() {
  const project = useProjectStore((s) => s.project);
  const offline = useProjectStore((s) => s.offline);
  const running = useProjectStore((s) => s.running);
  const livePct = useProjectStore((s) => s.livePct);
  const liveT = useProjectStore((s) => s.liveT);
  const dirty = useProjectStore((s) => s.dirty);
  const newProject = useProjectStore((s) => s.newProject);
  const messages = useProjectStore((s) => s.messages);
  const errors = messages.filter((m) => m.level === "error").length;
  const elementCount =
    project?.systems.reduce((n, s) => n + s.elements.length, 0) ?? 0;

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
          onClick={() => void confirmReplaceProject("Creating a new project").then((ok) => ok && newProject())}
        >
          <Plus size={13} />
        </button>
      </div>
      <div className="ml-auto flex items-center gap-3 px-3 text-[color:var(--ss-text-dim)]">
        {running && (
          <span className="flex items-center gap-1 text-[color:var(--ss-accent)]">
            <Loader2 size={12} className="animate-spin" />
            solving… t = {liveT.toFixed(0)} s ({livePct.toFixed(0)} %)
            <span className="ml-1 inline-block h-[6px] w-[90px] overflow-hidden rounded bg-[color:var(--ss-active)]">
              <span
                className="block h-full bg-[color:var(--ss-accent)] transition-[width]"
                style={{ width: `${livePct}%` }}
              />
            </span>
          </span>
        )}
        {errors > 0 && <span className="text-red-600">{errors} error(s)</span>}
        <span>{elementCount} elements</span>
        {offline ? (
          <span className="flex items-center gap-1 text-amber-600">
            <CloudOff size={12} /> backend offline
          </span>
        ) : (
          <span className="text-emerald-700">backend connected</span>
        )}
      </div>
    </div>
  );
}

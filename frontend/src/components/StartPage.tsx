import { useEffect, useRef, useState } from "react";
import { ArrowRight, CircleHelp, FilePlus2 } from "lucide-react";
import * as api from "../api";
import { openHelp } from "../help";
import { confirmReplaceProject, countOf, useProjectStore } from "../store/projectStore";
import { useUIStore } from "../store/uiStore";

/** A sketch of a project's top diagram: a box where each part sits. */
function Thumb({ pts }: { pts?: [number, number][] }) {
  if (!pts?.length) return <div className="h-[60px] rounded bg-[color:var(--ss-panel-alt)]" />;
  const xs = pts.map((p) => p[0]);
  const ys = pts.map((p) => p[1]);
  const x0 = Math.min(...xs) - 20;
  const y0 = Math.min(...ys) - 20;
  return (
    <svg
      viewBox={`${x0} ${y0} ${Math.max(...xs) - x0 + 112} ${Math.max(...ys) - y0 + 80}`}
      className="h-[60px] w-full rounded bg-[color:var(--ss-panel-alt)]"
      aria-hidden="true"
    >
      {pts.map(([x, y], i) => (
        <rect key={i} x={x} y={y} width={92} height={54} rx={6} fill="var(--ss-node-icon)" opacity={0.55} />
      ))}
    </svg>
  );
}

/** A project to start from. Its name and size name the button; a long
 *  description is read as its description, not as part of the name. */
function Card({
  id,
  title,
  meta,
  text,
  pts,
  onClick,
}: {
  id: string;
  title: string;
  meta: string;
  text?: string | null;
  pts?: [number, number][];
  onClick: () => void;
}) {
  return (
    <button
      className="flex flex-col gap-1.5 rounded border border-[color:var(--ss-field-border)] bg-[color:var(--ss-panel)] p-2.5 text-left hover:border-[color:var(--ss-accent)] focus-visible:outline-2 focus-visible:outline-[color:var(--ss-accent)]"
      aria-label={meta ? `${title}, ${meta}` : title}
      aria-describedby={text ? `${id}-text` : undefined}
      onClick={onClick}
    >
      <Thumb pts={pts} />
      <span className="text-[13px] font-semibold text-[color:var(--ss-text)]">{title}</span>
      <span className="text-[11px] text-[color:var(--ss-text-dim)] [overflow-wrap:anywhere]">{meta}</span>
      {text && (
        <span id={`${id}-text`} className="whitespace-pre-line text-[11px] leading-snug text-[color:var(--ss-text-dim)]">
          {text}
        </span>
      )}
    </button>
  );
}

const GRID = "grid grid-cols-[repeat(auto-fill,minmax(230px,1fr))] gap-3";
const H2 = "mb-2 text-[13px] font-semibold text-[color:var(--ss-text)]";

/** Start (UX-16), at launch and on New: go on with the open project, start
 *  from an example or a blank project, or reopen a recent one. */
export function StartPage() {
  const project = useProjectStore((s) => s.project);
  const dirty = useProjectStore((s) => s.dirty);
  const offline = useProjectStore((s) => s.offline);
  const openLast = useUIStore((s) => s.openLastAtStart);
  const setOpenLast = useUIStore((s) => s.setOpenLastAtStart);
  const [recent, setRecent] = useState<(api.ProjectEntry & { path?: string })[] | null>(null);
  const [examples, setExamples] = useState<api.ExampleEntry[]>([]);
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    // New hides the button it was pressed on: start the keyboard here
    heading.current?.focus({ preventScroll: true });
    void Promise.all([
      api.listProjects().catch(() => []),
      api.listExamples().catch(() => []),
      api.listFiles().catch(() => []),
    ]).then(([projects, shipped, files]) => {
      // projects in the projects folder and .lightsim files anywhere (PLT-33),
      // by when each was last saved or opened
      const at = (p: api.ProjectEntry & { opened?: number }) => Math.max(p.modified ?? 0, p.opened ?? 0);
      const all = [...projects, ...(files ?? []).filter((f) => f.exists)];
      setRecent(all.sort((a, b) => at(b) - at(a)).slice(0, 8));
      setExamples(shipped.filter((e) => !e.hidden));
    });
  }, []);

  const home = () => useUIStore.getState().setRibbonTab("home");
  const replaceWith = (action: string, open: () => Promise<void> | void) =>
    void confirmReplaceProject(action).then(async (ok) => {
      if (!ok) return;
      await open();
      home();
    });
  const store = useProjectStore.getState;
  const parts = (n?: number | null) => (n == null ? "" : countOf(n, "part"));

  return (
    <div className="mx-auto flex max-w-[1100px] flex-col gap-5 p-6">
      <div className="flex flex-wrap items-center gap-3">
        <h1 ref={heading} tabIndex={-1} className="text-[18px] font-semibold text-[color:var(--ss-text)] outline-none">
          Start
        </h1>
        {project && (
          <button className="ss-toolbtn border border-[color:var(--ss-field-border)] px-2" onClick={home}>
            Continue with '{project.name}'{dirty ? " (unsaved)" : ""} <ArrowRight size={13} />
          </button>
        )}
        <button className="ss-toolbtn ml-auto px-2" onClick={() => openHelp()}>
          <CircleHelp size={13} /> Help and tutorials (F1)
        </button>
      </div>
      {offline && (
        <p className="text-[12px] text-[color:var(--ss-warn)]">
          The engine is not reachable, so the examples and your saved projects are not listed.
        </p>
      )}

      <section aria-labelledby="start-new">
        <h2 id="start-new" className={H2}>
          New from an example
        </h2>
        <div className={GRID}>
          {examples.map((e) => (
            <Card
              key={e.id}
              id={`start-example-${e.id}`}
              title={e.name}
              meta={[parts(e.elements), "opens as a copy"].filter(Boolean).join(" · ")}
              text={e.description}
              pts={e.thumb}
              onClick={() => replaceWith(`Opening '${e.name}'`, () => store().openExample(e.id))}
            />
          ))}
          <button
            className="flex min-h-[120px] flex-col items-center justify-center gap-1 rounded border border-dashed border-[color:var(--ss-field-border)] p-2.5 text-[13px] font-semibold text-[color:var(--ss-text)] hover:border-[color:var(--ss-accent)] focus-visible:outline-2 focus-visible:outline-[color:var(--ss-accent)]"
            onClick={() => replaceWith("Creating a new project", () => store().newProject())}
          >
            <FilePlus2 size={20} className="text-[color:var(--ss-accent)]" />
            Blank project
            <span className="text-[11px] font-normal text-[color:var(--ss-text-dim)]">An empty diagram</span>
          </button>
        </div>
      </section>

      <section aria-labelledby="start-recent">
        <h2 id="start-recent" className={H2}>
          Recent projects
        </h2>
        {recent?.length === 0 && !offline && (
          <p className="text-[12px] text-[color:var(--ss-text-dim)]">
            None saved yet. A project you save is listed here.
          </p>
        )}
        <div className={GRID}>
          {recent?.map((p) => (
            <Card
              key={p.id}
              id={`start-recent-${p.id}`}
              title={p.name}
              meta={[p.path ?? "", p.modified ? `Saved ${new Date(p.modified).toLocaleString()}` : "", parts(p.elements)]
                .filter(Boolean)
                .join(" · ")}
              pts={p.thumb}
              onClick={() => replaceWith(`Opening '${p.name}'`, () => store().openProject(p.id))}
            />
          ))}
        </div>
      </section>

      <label className="flex items-center gap-2 text-[12px] text-[color:var(--ss-text-dim)]">
        <input type="checkbox" checked={openLast} onChange={(e) => setOpenLast(e.target.checked)} />
        Skip this page: open my last project at start-up
      </label>
    </div>
  );
}

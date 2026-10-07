import { useEffect, useState } from "react";
import { useProjectStore } from "../store/projectStore";
import type { ExampleCard } from "../types";

const EMPTY: ExampleCard = {
  question: "",
  tags: [],
  difficulty: "beginner",
  runTimeS: null,
  learn: [],
  status: "demo",
  features: [],
  author: "",
  version: "",
  licence: "",
  narrative: [],
};

const STATUS_TEXT: Record<ExampleCard["status"], string> = {
  demo: "Demo: shows the workflow",
  "plausibility-checked": "Plausibility-checked: results in bands from real cars",
  validated: "Validated: compared with measurements of that car",
};

/** One line a list item, as the card stores lists. */
const lines = (text: string) => text.split("\n").map((l) => l.trim()).filter(Boolean);

/** The project's card (CON-15): what it answers, for whom, what you learn,
 *  what happens when, and how far its results are checked. The expected
 *  results are its cases' expected values (Cases tab). The Project tab's
 *  Card button opens it. */
export function ExampleCardDialog({ onClose }: { onClose: () => void }) {
  const project = useProjectStore((s) => s.project);
  const setCard = useProjectStore((s) => s.setCard);
  const [card, setLocal] = useState<ExampleCard>(() => ({ ...EMPTY, ...(project?.card ?? {}) }));
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const patch = (p: Partial<ExampleCard>) => setLocal((c) => ({ ...c, ...p }));
  const refs = (project?.cases ?? []).flatMap((c) =>
    (c.references ?? []).map((r) => ({ c: c.name, r })),
  );
  const field = "grid grid-cols-[120px_1fr] items-start gap-2";
  const label = "pt-1 text-[color:var(--ss-text-dim)]";
  return (
    <div
      className="fixed inset-0 z-[110] flex items-center justify-center bg-black/35"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div
        role="dialog"
        aria-label="Project card"
        className="flex max-h-[88vh] w-[620px] max-w-[94vw] flex-col overflow-hidden rounded-md border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] text-[12px] shadow-2xl"
      >
        <div className="border-b border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] px-4 py-2.5 text-[13px] font-semibold">
          Card · {project?.name}
        </div>
        <div className="flex flex-col gap-2 overflow-y-auto px-4 py-3">
          <label className={field}>
            <span className={label}>Question</span>
            <textarea className="ss-input" rows={2} value={card.question} onChange={(e) => patch({ question: e.target.value })} />
          </label>
          <div className="grid grid-cols-3 gap-2">
            <label className="flex flex-col gap-0.5">
              <span className="text-[color:var(--ss-text-dim)]">Difficulty</span>
              <select className="ss-input" value={card.difficulty} onChange={(e) => patch({ difficulty: e.target.value as ExampleCard["difficulty"] })}>
                <option value="beginner">Beginner</option>
                <option value="intermediate">Intermediate</option>
                <option value="advanced">Advanced</option>
              </select>
            </label>
            <label className="flex flex-col gap-0.5">
              <span className="text-[color:var(--ss-text-dim)]">Run time (s)</span>
              <input
                type="number"
                min={0}
                className="ss-input"
                value={card.runTimeS ?? ""}
                onChange={(e) => patch({ runTimeS: e.target.value === "" ? null : Number(e.target.value) })}
              />
            </label>
            <label className="flex flex-col gap-0.5">
              <span className="text-[color:var(--ss-text-dim)]">Status</span>
              <select
                className="ss-input"
                value={card.status}
                title={STATUS_TEXT[card.status]}
                onChange={(e) => patch({ status: e.target.value as ExampleCard["status"] })}
              >
                {(Object.keys(STATUS_TEXT) as ExampleCard["status"][]).map((s) => (
                  <option key={s} value={s}>
                    {STATUS_TEXT[s]}
                  </option>
                ))}
              </select>
            </label>
          </div>
          {(
            [
              ["learn", "What you learn"],
              ["narrative", "What happens when"],
              ["features", "Features used"],
              ["tags", "Tags"],
            ] as const
          ).map(([key, name]) => (
            <label key={key} className={field}>
              <span className={label}>
                {name}
                <span className="block text-[10px]">one a line</span>
              </span>
              <textarea
                className="ss-input"
                rows={key === "narrative" ? 4 : 2}
                defaultValue={card[key].join("\n")}
                onChange={(e) => patch({ [key]: lines(e.target.value) })}
              />
            </label>
          ))}
          {(
            [
              ["author", "Author"],
              ["version", "Version"],
              ["licence", "Licence"],
            ] as const
          ).map(([key, name]) => (
            <label key={key} className={field}>
              <span className={label}>{name}</span>
              <input className="ss-input" value={card[key]} onChange={(e) => patch({ [key]: e.target.value })} />
            </label>
          ))}
          <div className={field}>
            <span className={label}>Expected results</span>
            <div className="pt-1">
              {refs.length === 0 ? (
                <span className="text-[color:var(--ss-text-dim)]">
                  None yet: add them per case under Expected values in the Cases tab.
                </span>
              ) : (
                <ul className="m-0 list-none p-0">
                  {refs.map(({ c, r }, i) => (
                    <li key={i}>
                      {c}: {r.kpi} {r.value.toLocaleString()} ± {r.tolerance}
                      {r.tolerancePct === false ? "" : " %"}
                      {r.source && <span className="text-[color:var(--ss-text-dim)]"> · {r.source}</span>}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </div>
        </div>
        <div className="flex justify-end gap-2 border-t border-[color:var(--ss-border)] px-4 py-2">
          <button className="ss-toolbtn border border-[color:var(--ss-border)] px-3" onClick={onClose}>
            Cancel
          </button>
          <button
            className="ss-toolbtn border border-[color:var(--ss-accent)] px-3 text-[color:var(--ss-accent)]"
            onClick={() => {
              setCard(card);
              onClose();
            }}
          >
            Save card
          </button>
        </div>
      </div>
    </div>
  );
}

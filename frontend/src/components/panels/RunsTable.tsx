import { Fragment, useMemo, useState, type ReactNode } from "react";
import { ArrowDown, ArrowUp } from "lucide-react";
import { runLabel } from "./RunInfo";
import {
  figureColumns,
  figureOf,
  ranText,
  rowName,
  sortRows,
  statusText,
  STATUS_COLOR,
  isShipped,
  type RunRow,
  type Sort,
  type SortKey,
} from "./runRows";
import { formatNumber } from "../../format";

/** The table of runs in Results: a row per stored run with its main
 *  figures. A click on a heading sorts by it (again: the other way). The
 *  run shown is the first one drawn and drives the channel list, table and
 *  summary; a click on another row shows that one first. A tick draws a
 *  run with it. A few rows high; it scrolls. */
export function RunsTable({
  rows,
  shownId,
  drawnIds,
  colorOf,
  reading,
  onTick,
  onShow,
  tools,
}: {
  rows: RunRow[];
  shownId: string | null;
  /** the runs drawn with the one shown, in order */
  drawnIds: string[];
  /** the colour of a drawn run's lines */
  colorOf: (id: string) => string;
  /** runs whose data is being read from disk */
  reading: Set<string>;
  onTick: (row: RunRow, on: boolean) => void;
  onShow: (row: RunRow) => void;
  /** buttons on the caption line */
  tools?: ReactNode;
}) {
  const [sort, setSort] = useState<Sort>({ key: "ran", desc: true });
  const cols = useMemo(() => figureColumns(rows), [rows]);
  const sorted = useMemo(() => sortRows(rows, sort, cols), [rows, sort, cols]);
  // (no Name column when no run has a name or a swept value)
  const named = rows.some((r) => rowName(r));
  const drawn = new Set(drawnIds);
  const alone = drawnIds.length === 0;

  const head = (key: SortKey, text: ReactNode, title: string, right = false) => {
    const on = sort.key === key;
    return (
      <th
        className={`ss-th whitespace-nowrap ${right ? "text-right" : ""}`}
        aria-sort={on ? (sort.desc ? "descending" : "ascending") : "none"}
        title={title}
      >
        <button
          className="inline-flex items-center gap-0.5 font-semibold"
          // a number sorts high to low first, as the time does (newest first)
          onClick={() => setSort({ key, desc: on ? !sort.desc : key !== "name" && key !== "case" && key !== "status" })}
        >
          {text}
          {on && (sort.desc ? <ArrowDown size={10} aria-hidden /> : <ArrowUp size={10} aria-hidden />)}
        </button>
      </th>
    );
  };

  return (
    <div className="shrink-0 border-b border-[color:var(--ss-border)]">
      <div className="flex items-center gap-1 px-2 py-0.5 text-meta text-[color:var(--ss-text-dim)]">
        <span>{rows.length === 1 ? "1 run" : `${rows.length} runs`}</span>
        <div className="ml-auto flex items-center gap-1">{tools}</div>
      </div>
      <div className="max-h-[118px] overflow-auto" role="region" aria-label="Runs" tabIndex={-1}>
        <table className="w-full border-collapse text-meta">
          <thead className="sticky top-0 z-10">
            <tr>
              <th className="ss-th w-[38px]">
                <span className="sr-only">Drawn</span>
              </th>
              {head("ran", "Ran", "When the run started")}
              {named && head("name", "Name", "The run's name: what changed since the run before, or what you typed")}
              {head("case", "Case", "The case it ran")}
              {head("status", "Status", "How the run ended")}
              {cols.map((c, i) => (
                <Fragment key={c.key ?? c.label}>
                  {head(
                    i,
                    <>
                      {c.label.split(" — ").pop()} <span className="font-normal">[{c.unit}]</span>
                    </>,
                    c.label,
                    true,
                  )}
                </Fragment>
              ))}
            </tr>
          </thead>
          <tbody>
            {sorted.map((r) => {
              const shown = r.id === shownId;
              const on = shown || drawn.has(r.id);
              const label = runLabel(r);
              // (its name holds this, the words it shows)
              const ran = ranText(r.startedAt, undefined, isShipped(r));
              const status = reading.has(r.id) ? "reading…" : statusText(r);
              return (
                <tr
                  key={r.id}
                  aria-current={shown || undefined}
                  className={`cursor-pointer ${shown ? "bg-[color:var(--ss-accent-soft)]" : "hover:bg-[color:var(--ss-hover)]"}`}
                  title={shown ? `${label} — the run shown` : `${label} — click to show it first`}
                  onClick={(e) => {
                    if ((e.target as HTMLElement).closest("input, button")) return;
                    if (!shown) onShow(r);
                  }}
                >
                  <td className="ss-td whitespace-nowrap">
                    <span className="flex items-center gap-1">
                      <input
                        type="checkbox"
                        aria-label={`Draw ${label}`}
                        checked={on}
                        disabled={(shown && alone) || reading.has(r.id)}
                        title={
                          shown
                            ? alone
                              ? "The run shown: tick another to draw it too, or click another row to show that one"
                              : "Untick to stop drawing the run shown: the next one ticked is shown"
                            : "Draw this run with the run shown"
                        }
                        onChange={(e) => onTick(r, e.target.checked)}
                      />
                      <span
                        className="h-2 w-2 shrink-0 rounded-full"
                        style={{ background: on ? colorOf(r.id) : "transparent" }}
                        aria-hidden
                      />
                    </span>
                  </td>
                  <td className={`ss-td whitespace-nowrap ${shown ? "font-semibold" : ""}`}>
                    {/* the keyboard's way to show it */}
                    <button
                      className="text-left"
                      aria-label={`Show ${label}${label.includes(ran) ? "" : ` · ${ran}`}`}
                      data-run-label={label}
                      aria-pressed={shown}
                      onClick={() => !shown && onShow(r)}
                    >
                      {ran}
                    </button>
                  </td>
                  {named && <td className="ss-td max-w-[220px] truncate">{rowName(r)}</td>}
                  <td className="ss-td max-w-[140px] truncate">{r.caseName}</td>
                  <td
                    className="ss-td whitespace-nowrap"
                    style={{ color: STATUS_COLOR[status] ?? STATUS_COLOR.warning }}
                    title={r.incomplete}
                  >
                    {status}
                  </td>
                  {cols.map((c) => {
                    const sv = figureOf(r, c);
                    return (
                      <td
                        key={c.key ?? c.label}
                        className={`ss-td whitespace-nowrap text-right font-mono ${sv?.notValid ? "text-[color:var(--ss-warn)]" : ""}`}
                        title={sv?.notValid ? `Not valid: ${sv.notValid}` : undefined}
                      >
                        {sv ? formatNumber(sv.value) : "—"}
                      </td>
                    );
                  })}
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}

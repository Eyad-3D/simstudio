import { useEffect, useState } from "react";
import * as api from "../api";
import { useProjectStore } from "../store/projectStore";

/** The tests, as the engine names them (backend/app/vehicle_tests.py). */
const TESTS: [string, string][] = [
  ["accel_0_100", "0-100 km/h"],
  ["accel_80_120", "80-120 km/h"],
  ["top_speed", "Top speed, and what limits it"],
  ["constant_speed", "Consumption and range at 50, 90 and 120 km/h"],
  ["gradeability", "Steepest grade at 30 km/h"],
  ["coast_down", "Virtual coast-down: road load A, B, C"],
];

const fmt = (v: number) => v.toLocaleString("en", { maximumSignificantDigits: 4 });

/** CON-06: one-click vehicle tests on the model as it is. */
export function VehicleTestsDialog({ onClose }: { onClose: () => void }) {
  const project = useProjectStore((s) => s.project);
  const [chosen, setChosen] = useState<Set<string>>(new Set(TESTS.map(([id]) => id)));
  const [busy, setBusy] = useState(false);
  const [rows, setRows] = useState<api.VehicleTestRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const run = async () => {
    if (!project) return;
    setBusy(true);
    setError(null);
    setRows(null);
    try {
      const ids = TESTS.map(([id]) => id).filter((id) => chosen.has(id));
      setRows((await api.vehicleTests(project, ids)).rows);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/40">
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Vehicle tests"
        className="flex max-h-[90vh] w-[640px] max-w-[94vw] flex-col overflow-hidden rounded-md border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] shadow-2xl"
      >
        <div className="border-b border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] px-4 py-2.5 text-[13px] font-semibold">
          Vehicle tests
        </div>
        <div className="flex flex-col gap-2 overflow-y-auto px-4 py-3 text-[12px]">
          <p className="leading-snug text-[color:var(--ss-text-dim)]">
            Runs standard tests on this model as it is, without changing it. Each test adds its own runs; none is stored.
          </p>
          <fieldset className="flex flex-col gap-1">
            {TESTS.map(([id, label]) => (
              <label key={id} className="flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={chosen.has(id)}
                  onChange={(e) =>
                    setChosen((prev) => {
                      const next = new Set(prev);
                      if (e.target.checked) next.add(id);
                      else next.delete(id);
                      return next;
                    })
                  }
                />
                {label}
              </label>
            ))}
          </fieldset>
          {error && (
            <p role="alert" className="text-red-600">
              {error}
            </p>
          )}
          {rows && (
            <table className="w-full border-collapse text-left">
              <thead>
                <tr className="text-[11px] text-[color:var(--ss-text-dim)]">
                  <th className="py-1 pr-2 font-normal">Figure</th>
                  <th className="py-1 pr-2 text-right font-normal">Value</th>
                  <th className="py-1 font-normal">How, and what limits it</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r, i) => (
                  <tr key={i} className="border-t border-[color:var(--ss-border)]">
                    <td className="py-1 pr-2">{r.what}</td>
                    <td className="whitespace-nowrap py-1 pr-2 text-right tabular-nums">
                      {r.value == null ? "—" : `${fmt(r.value)} ${r.unit}`}
                    </td>
                    <td className="py-1 text-[11px] text-[color:var(--ss-text-dim)]">
                      {r.how}
                      {r.note && <span className="block text-[color:var(--ss-text)]">{r.note}</span>}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
        <div className="flex justify-end gap-2 border-t border-[color:var(--ss-border)] px-4 py-2.5">
          <button className="ss-toolbtn border border-[color:var(--ss-border)] px-3" onClick={onClose}>
            Close
          </button>
          <button
            className="rounded bg-[color:var(--ss-accent-fill)] px-3 py-1 text-[12px] font-semibold text-white hover:brightness-110 disabled:opacity-40"
            disabled={busy || !project || chosen.size === 0}
            onClick={() => void run()}
          >
            {busy ? "Running the tests…" : "Run the tests"}
          </button>
        </div>
      </div>
    </div>
  );
}

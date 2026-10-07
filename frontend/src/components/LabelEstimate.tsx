import { useEffect, useState } from "react";
import * as api from "../api";
import { useProjectStore } from "../store/projectStore";

const fmt = (v: number) => v.toLocaleString("en", { maximumFractionDigits: v >= 100 ? 1 : 2 });

/** CON-32: the US window-sticker estimate. Runs the model on EPA's city and
 *  highway cycles, then shows each step from the lab figures to the label
 *  figures, always marked as not certified. */
export function LabelEstimateDialog({ onClose }: { onClose: () => void }) {
  const project = useProjectStore((s) => s.project);
  const activeCaseId = useProjectStore((s) => s.activeCaseId);
  const [modelYear, setModelYear] = useState(2017);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<api.LabelEstimate | null>(null);
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
    setResult(null);
    try {
      setResult(await api.labelEstimate(project, activeCaseId, modelYear));
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
        aria-label="US label estimate"
        className="flex max-h-[90vh] w-[640px] max-w-[94vw] flex-col overflow-hidden rounded-md border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] shadow-2xl"
      >
        <div className="border-b border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] px-4 py-2.5 text-[13px] font-semibold">
          US label estimate (not certified)
        </div>
        <div className="flex flex-col gap-2 overflow-y-auto px-4 py-3 text-[12px]">
          <p className="leading-snug text-[color:var(--ss-text-dim)]">
            Runs this model on EPA&apos;s city cycle (UDDS) and highway cycle (HWFET), then adjusts the two lab figures
            with EPA&apos;s derived five-cycle equations, as FASTSim does. A case that already drives UDDS or HWFET is
            used as it is; otherwise the active case is copied onto the cycle.
          </p>
          <label className="flex items-center gap-2">
            <span>EPA coefficients for model years</span>
            <select
              className="ss-input"
              aria-label="EPA coefficients"
              value={modelYear}
              onChange={(e) => setModelYear(Number(e.target.value))}
            >
              <option value={2017}>2017 and later</option>
              <option value={2016}>2008 to 2016</option>
            </select>
          </label>
          {error && (
            <p role="alert" className="text-red-600">
              {error}
            </p>
          )}
          {result && (
            <>
              <p className="font-semibold" data-testid="label-not-certified">
                {result.notCertified}
              </p>
              <p className="text-[color:var(--ss-text-dim)]">
                Runs: {Object.values(result.cases).join(" and ")}.
                {result.electric && result.chargerEfficiency != null &&
                  ` Charger efficiency ${(result.chargerEfficiency * 100).toFixed(0)} %.`}
              </p>
              {result.problems.map((p) => (
                <p key={p} role="alert" className="text-amber-600">
                  {p}
                </p>
              ))}
              <table className="w-full border-collapse text-left">
                <thead>
                  <tr className="text-[11px] text-[color:var(--ss-text-dim)]">
                    <th className="py-1 pr-2 font-normal">Figure</th>
                    <th className="py-1 pr-2 text-right font-normal">Value</th>
                    <th className="py-1 font-normal">How it is worked out</th>
                  </tr>
                </thead>
                <tbody>
                  {result.steps.map((s, i) => (
                    <tr key={i} className="border-t border-[color:var(--ss-border)]">
                      <td className="py-1 pr-2">{s.what}</td>
                      <td className="whitespace-nowrap py-1 pr-2 text-right tabular-nums">
                        {fmt(s.value)} {s.unit}
                      </td>
                      <td className="py-1 text-[11px] text-[color:var(--ss-text-dim)]">{s.how}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </>
          )}
        </div>
        <div className="flex justify-end gap-2 border-t border-[color:var(--ss-border)] px-4 py-2.5">
          <button className="ss-toolbtn border border-[color:var(--ss-border)] px-3" onClick={onClose}>
            Close
          </button>
          <button
            className="rounded bg-[color:var(--ss-accent-fill)] px-3 py-1 text-[12px] font-semibold text-white hover:brightness-110 disabled:opacity-40"
            disabled={busy || !project}
            onClick={() => void run()}
          >
            {busy ? "Running UDDS and HWFET…" : result ? "Run again" : "Run UDDS and HWFET"}
          </button>
        </div>
      </div>
    </div>
  );
}

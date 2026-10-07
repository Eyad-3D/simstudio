import { useEffect, useState } from "react";
import * as api from "../api";
import { useProjectStore } from "../store/projectStore";

const fmt = (v: number | undefined, d = 2, unit = "") =>
  v == null ? "—" : `${v.toLocaleString("en", { maximumFractionDigits: d })}${unit}`;

/** Calibrate lap mode's grip and downforce on one logged lap and check the
 *  prediction on another (VAL-38). */
export function CalibrateDialog({ onClose }: { onClose: () => void }) {
  const project = useProjectStore((s) => s.project);
  const apply = useProjectStore((s) => s.applyLapCalibration);
  const [cal, setCal] = useState<{ name: string; text: string } | null>(null);
  const [check, setCheck] = useState<{ name: string; text: string } | null>(null);
  const [unit, setUnit] = useState("km/h");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<api.CalibrationResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && !busy && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, busy]);

  const pick = (set: (f: { name: string; text: string }) => void) => async (e: React.ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0];
    if (f) set({ name: f.name, text: await f.text() });
    setResult(null);
  };
  const run = async () => {
    if (!project || !cal) return;
    setBusy(true);
    setError(null);
    try {
      setResult(
        await api.calibrateLap(
          project,
          { text: cal.text, speedUnit: unit },
          check ? { text: check.text, speedUnit: unit } : undefined,
        ),
      );
    } catch (e) {
      setError((e as Error).message.replace(/^\d+ /, ""));
    } finally {
      setBusy(false);
    }
  };
  const row = "flex items-center justify-between gap-2 text-[11px] text-[color:var(--ss-text-dim)]";
  const pred = (title: string, p?: api.LapPrediction) =>
    p && (
      <tr className="border-t border-[color:var(--ss-border)]">
        <td className="py-0.5 pr-2">{title}</td>
        <td className="py-0.5 pr-2 text-right tabular-nums">
          {fmt(p.lap_time_log_s, 3)} / {fmt(p.lap_time_model_s, 3)}
        </td>
        <td className="py-0.5 pr-2 text-right tabular-nums">{fmt(p.lap_time_error_pct, 2, " %")}</td>
        <td className="py-0.5 pr-2 text-right tabular-nums">{fmt(p.speed_rms_kmh, 2)}</td>
        <td className="py-0.5 text-right tabular-nums">{fmt(p.energy_error_pct, 2, " %")}</td>
      </tr>
    );

  return (
    <div
      className="fixed inset-0 z-[110] flex items-center justify-center bg-black/35"
      onMouseDown={(e) => e.target === e.currentTarget && !busy && onClose()}
    >
      <div
        role="dialog"
        aria-label="Calibrate lap mode"
        className="max-h-[92vh] w-[560px] max-w-[94vw] overflow-y-auto rounded-md border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] shadow-2xl"
      >
        <div className="border-b border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] px-4 py-2.5 text-[13px] font-semibold">
          Calibrate lap mode on a logged lap
        </div>
        <div className="grid gap-2 px-4 py-3">
          <p className="text-[11px] leading-snug text-[color:var(--ss-text-dim)]">
            Pick a logged lap (CSV with time or distance, speed and lateral acceleration; pack power if you have it).
            LightSim builds the track from it and finds the grip scale and downforce (CzA) whose lap mode speed fits the
            logged speed best. A second lap from another session checks the prediction, blind. The files stay on your
            computer; the fit takes about 30 s.
          </p>
          <label className={row}>
            Lap to calibrate on
            <input type="file" accept=".csv,.txt,text/csv" aria-label="Calibration lap file" onChange={pick(setCal)} />
          </label>
          <label className={row}>
            Lap to check (optional)
            <input type="file" accept=".csv,.txt,text/csv" aria-label="Check lap file" onChange={pick(setCheck)} />
          </label>
          <label className={row}>
            Speed unit
            <select className="ss-input w-[120px]" value={unit} onChange={(e) => setUnit(e.target.value)}>
              {["km/h", "m/s", "mph"].map((u) => (
                <option key={u} value={u}>
                  {u}
                </option>
              ))}
            </select>
          </label>
          {error && (
            <p role="alert" className="text-[11px] text-[color:var(--ss-err)]">
              {error}
            </p>
          )}
          {result && (
            <>
              <p className="text-[11px]">
                Best fit: grip × {fmt(result.fit.mu_scale, 4)}, CzA {fmt(result.fit.cza, 3)} m², speed RMS{" "}
                {fmt(result.fit.rms_kmh, 2)} km/h on the calibration lap.
              </p>
              <table className="w-full border-collapse text-[11px]" aria-label="Calibration results">
                <thead>
                  <tr className="text-left text-[color:var(--ss-text-dim)]">
                    <th className="py-0.5 pr-2 font-normal">Lap</th>
                    <th className="py-0.5 pr-2 text-right font-normal">Time logged / model (s)</th>
                    <th className="py-0.5 pr-2 text-right font-normal">Time error</th>
                    <th className="py-0.5 pr-2 text-right font-normal">Speed RMS (km/h)</th>
                    <th className="py-0.5 text-right font-normal">Energy error</th>
                  </tr>
                </thead>
                <tbody>
                  {pred("Calibration", result.calibration_lap)}
                  {pred("Check (blind)", result.check_lap)}
                </tbody>
              </table>
              <p className="text-[10px] text-[color:var(--ss-text-dim)]">
                Grip and downforce both raise the cornering speed, so one lap cannot always tell them apart: trust the
                check lap&apos;s errors more than the two values. The energy error needs the pack power in the log.
              </p>
            </>
          )}
        </div>
        <div className="flex justify-end gap-2 border-t border-[color:var(--ss-border)] px-4 py-2.5">
          <button className="ss-toolbtn border border-[color:var(--ss-border)]" disabled={busy} onClick={onClose}>
            Close
          </button>
          {result && (
            <button
              className="ss-toolbtn border border-[color:var(--ss-border)]"
              title="Scale every wheel's μ, lateral μ and load sensitivity by the grip factor and set the Vehicle's CzA (one undo)"
              onClick={() => {
                apply(result.fit.mu_scale, result.fit.cza);
                onClose();
              }}
            >
              Apply to the model
            </button>
          )}
          <button
            className="ss-toolbtn border border-[color:var(--ss-border)] disabled:opacity-40"
            disabled={!cal || busy || !project}
            onClick={() => void run()}
          >
            {busy ? "Calibrating…" : "Calibrate"}
          </button>
        </div>
      </div>
    </div>
  );
}

import { useEffect, useState } from "react";
import * as api from "../api";
import { useProjectStore } from "../store/projectStore";

type Role = "time" | "distance" | "speed" | "lap";
const ROLES: { role: Role; label: string }[] = [
  { role: "time", label: "Time" },
  { role: "distance", label: "Distance" },
  { role: "speed", label: "Speed" },
  { role: "lap", label: "Lap number" },
];

async function sha256(text: string): Promise<string> {
  const buf = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(buf)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** Speed against time, as a small line chart. */
function Preview({ points }: { points: [number, number][] }) {
  if (points.length < 2) return null;
  const tMax = points[points.length - 1][0] || 1;
  const vMax = Math.max(...points.map((p) => p[1]), 1);
  const d = points
    .map(([t, v], i) => `${i ? "L" : "M"}${((t / tMax) * 400).toFixed(1)},${(120 - (v / vMax) * 110).toFixed(1)}`)
    .join("");
  return (
    <svg
      viewBox="0 0 400 124"
      className="h-[124px] w-full"
      role="img"
      aria-label="Speed of the imported lap against time"
    >
      <path d={d} fill="none" stroke="var(--ss-accent)" strokeWidth={1.2} />
      <text x={2} y={10} fontSize={9} fill="var(--ss-text-dim)">
        {Math.round(vMax)} km/h
      </text>
      <text x={398} y={122} fontSize={9} textAnchor="end" fill="var(--ss-text-dim)">
        {Math.round(tMax)} s
      </text>
    </svg>
  );
}

/** Import a lap from a data logger or lap simulator CSV as a drive cycle
 *  case (STD-35). */
export function LapImportDialog({ onClose }: { onClose: () => void }) {
  const addImportedLap = useProjectStore((s) => s.addImportedLap);
  const [presets, setPresets] = useState<api.LapLogPreset[]>([]);
  const [preset, setPreset] = useState("Generic");
  const [file, setFile] = useState<{
    name: string;
    text: string;
    hash: string;
  } | null>(null);
  const [columns, setColumns] = useState<Partial<Record<Role, string | null>>>({});
  const [unit, setUnit] = useState<string | null>(null);
  const [lap, setLap] = useState<number | null>(null);
  const [endurance, setEndurance] = useState(false);
  const [stopS, setStopS] = useState(180);
  const [name, setName] = useState("");
  const [result, setResult] = useState<api.LapLogResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.laplogPresets().then(setPresets, () => setPresets([]));
  }, []);

  useEffect(() => {
    if (!file) return;
    let live = true;
    api
      .readLapLog({
        text: file.text,
        preset,
        columns,
        speedUnit: unit,
        lap,
        repeatToKm: endurance ? 22 : 0,
        driverChangeS: endurance ? stopS : 0,
      })
      .then(
        (r) => {
          if (!live) return;
          setResult(r);
          setError(null);
        },
        (e: Error) => {
          if (!live) return;
          setResult(null);
          setError(e.message.replace(/^\d+ /, ""));
        },
      );
    return () => {
      live = false;
    };
  }, [file, preset, columns, unit, lap, endurance, stopS]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const note = presets.find((p) => p.name === preset)?.note;
  const row = "flex items-center justify-between gap-2 text-[11px] text-[color:var(--ss-text-dim)]";
  return (
    <div
      className="fixed inset-0 z-[110] flex items-center justify-center bg-black/35"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div
        role="dialog"
        aria-label="Import a lap"
        className="max-h-[92vh] w-[520px] max-w-[94vw] overflow-y-auto rounded-md border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] shadow-2xl"
      >
        <div className="border-b border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] px-4 py-2.5 text-[13px] font-semibold">
          Import a lap from a logger or lap simulator
        </div>
        <div className="grid gap-2 px-4 py-3">
          <p className="text-[11px] leading-snug text-[color:var(--ss-text-dim)]">
            Pick a CSV file with a lap&apos;s speed against time or distance. LightSim drives that speed as a drive
            cycle and gives the energy, battery and motor loads for it; it does not work out the cornering speed (use a
            Lap case for that). The file stays on your computer.
          </p>
          <label className={row}>
            Layout
            <select className="ss-input w-[220px]" value={preset} onChange={(e) => setPreset(e.target.value)}>
              {(presets.length ? presets.map((p) => p.name) : ["Generic"]).map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </label>
          {note && <p className="text-[10px] text-[color:var(--ss-text-dim)]">{note}</p>}
          <label className={row}>
            File
            <input
              type="file"
              accept=".csv,.txt,text/csv"
              aria-label="Lap file"
              className="text-[11px]"
              onChange={async (e) => {
                const f = e.target.files?.[0];
                if (!f) return;
                const text = await f.text();
                setColumns({});
                setLap(null);
                setUnit(null);
                setName(f.name.replace(/\.[^.]+$/, ""));
                setFile({ name: f.name, text, hash: await sha256(text) });
              }}
            />
          </label>
          {result && (
            <>
              {ROLES.map(({ role, label }) => (
                <label key={role} className={row}>
                  {label} column
                  <select
                    className="ss-input w-[220px]"
                    value={result.picked[role] ?? ""}
                    onChange={(e) => setColumns({ ...columns, [role]: e.target.value || null })}
                  >
                    <option value="">(none)</option>
                    {result.columns.map((c) => (
                      <option key={c} value={c}>
                        {c}
                      </option>
                    ))}
                  </select>
                </label>
              ))}
              <label className={row}>
                Speed unit
                <select
                  className="ss-input w-[220px]"
                  value={unit ?? ""}
                  onChange={(e) => setUnit(e.target.value || null)}
                >
                  <option value="">from the file or layout</option>
                  {["km/h", "m/s", "mph", "kn"].map((u) => (
                    <option key={u} value={u}>
                      {u}
                    </option>
                  ))}
                </select>
              </label>
              {result.laps.length > 0 && (
                <label className={row}>
                  Lap
                  <select
                    className="ss-input w-[220px]"
                    value={lap ?? ""}
                    onChange={(e) => setLap(e.target.value === "" ? null : Number(e.target.value))}
                  >
                    <option value="">the fastest full lap</option>
                    {result.laps.map((l) => (
                      <option key={l.lap} value={l.lap}>
                        Lap {l.lap} (
                        {l.span.toLocaleString("en", {
                          maximumFractionDigits: 1,
                        })}
                        )
                      </option>
                    ))}
                  </select>
                </label>
              )}
              <label className={row}>
                <span>
                  <input type="checkbox" checked={endurance} onChange={(e) => setEndurance(e.target.checked)} /> Repeat
                  to a 22 km endurance
                </span>
                {endurance && (
                  <span className="flex items-center gap-1">
                    driver change stop (s)
                    <input
                      type="number"
                      className="ss-input w-[64px]"
                      min={0}
                      value={stopS}
                      onChange={(e) => setStopS(Math.max(0, Number(e.target.value) || 0))}
                    />
                  </span>
                )}
              </label>
              <Preview points={result.preview} />
              <p className="text-[11px]">
                {result.lap != null ? `Lap ${result.lap}: ` : ""}
                {result.duration_s.toLocaleString("en", {
                  maximumFractionDigits: 1,
                })}{" "}
                s,{" "}
                {result.distance_m.toLocaleString("en", {
                  maximumFractionDigits: 1,
                })}{" "}
                m{result.repeated > 1 ? ` (${result.repeated} laps)` : ""}
                {result.source_distance_m != null
                  ? `; the file's own distance ${result.source_distance_m.toLocaleString("en", { maximumFractionDigits: 1 })} m`
                  : ""}
              </p>
              {result.warnings.map((w) => (
                <p key={w} className="text-[11px] text-[color:var(--ss-warn)]">
                  {w}
                </p>
              ))}
              <label className={row}>
                Case name
                <input className="ss-input w-[220px]" value={name} onChange={(e) => setName(e.target.value)} />
              </label>
            </>
          )}
          {error && (
            <p role="alert" className="text-[11px] text-[color:var(--ss-err)]">
              {error}
            </p>
          )}
        </div>
        <div className="flex justify-end gap-2 border-t border-[color:var(--ss-border)] px-4 py-2.5">
          <button className="ss-toolbtn border border-[color:var(--ss-border)]" onClick={onClose}>
            Cancel
          </button>
          <button
            className="ss-toolbtn border border-[color:var(--ss-border)] disabled:opacity-40"
            disabled={!result || !file}
            onClick={() => {
              if (!result || !file) return;
              addImportedLap(result, name.trim() || file.name, file.hash);
              onClose();
            }}
          >
            Add as case
          </button>
        </div>
      </div>
    </div>
  );
}

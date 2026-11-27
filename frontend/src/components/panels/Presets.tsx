// A part's presets (the battery's Formula Student Electric, the Ambient's
// weather): one small "Presets" control at the top of its values. Opening it
// lists the presets; choosing one shows what it sets and where the values come
// from, and only Apply changes them, in one undo step.
import { useId, useState } from "react";
import { ChevronDown } from "lucide-react";
import { useProjectStore } from "../../store/projectStore";
import { optionLabel } from "../../notYet";
import type { ComponentDef, ParamValue } from "../../types";

type Preset = NonNullable<ComponentDef["presets"]>[number];

/** "Max Discharge Power: 80 kW", in the library's words and units. */
export function presetLines(def: ComponentDef, preset: Preset): string[] {
  return Object.entries(preset.values).map(([key, v]) => {
    const p = def.parameters.find((x) => x.key === key);
    const unit = p?.unit && p.unit !== "-" ? ` ${p.unit}` : "";
    const shown = (x: ParamValue) =>
      typeof x === "boolean" ? (x ? "on" : "off") : typeof x === "string" ? optionLabel(key, x).label : `${x}${unit}`;
    return `${p?.label ?? key}: ${shown(v)}`;
  });
}

export function Presets({ def, elementId }: { def: ComponentDef; elementId: string }) {
  const setParameters = useProjectStore((s) => s.setParameters);
  const [open, setOpen] = useState(false);
  const [chosen, setChosen] = useState<Preset | null>(null);
  const id = useId();
  const presets = def.presets ?? [];
  if (!presets.length) return null;
  const close = () => {
    setOpen(false);
    setChosen(null);
  };
  return (
    <div
      className="flex flex-col items-start gap-1"
      onKeyDown={(e) => {
        if (e.key === "Escape" && open) {
          e.stopPropagation();
          close();
          e.currentTarget.querySelector<HTMLButtonElement>("button")?.focus();
        }
      }}
    >
      <button
        className="ss-toolbtn border border-[color:var(--ss-border)] px-2 text-meta"
        aria-expanded={open}
        aria-controls={id}
        onClick={() => (open ? close() : setOpen(true))}
      >
        Presets <ChevronDown size={11} />
      </button>
      {open && (
        <div id={id} className="w-full max-w-[640px] rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] p-2 text-meta">
          {!chosen ? (
            <div role="group" aria-label="Presets" className="flex flex-col items-start gap-0.5">
              {presets.map((p) => (
                <button key={p.name} className="ss-toolbtn" onClick={() => setChosen(p)}>
                  {p.name}
                </button>
              ))}
            </div>
          ) : (
            <div role="group" aria-label={`Preset: ${chosen.name}`} className="flex flex-col gap-1.5">
              <span className="font-semibold text-[color:var(--ss-text)]">{chosen.name} sets</span>
              <ul className="list-disc pl-5 text-[color:var(--ss-text)]">
                {presetLines(def, chosen).map((line) => (
                  <li key={line}>{line}</li>
                ))}
              </ul>
              {chosen.note && <p className="text-[color:var(--ss-text-dim)]">{chosen.note}</p>}
              <div className="flex gap-2">
                <button
                  className="ss-toolbtn border border-[color:var(--ss-accent)] px-2 font-medium text-[color:var(--ss-accent)]"
                  onClick={() => {
                    setParameters(elementId, chosen.values);
                    close();
                  }}
                >
                  Apply
                </button>
                <button className="ss-toolbtn border border-[color:var(--ss-border)] px-2" onClick={close}>
                  Cancel
                </button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

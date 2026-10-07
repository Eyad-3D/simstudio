import { useState } from "react";
import { useProjectStore } from "../../store/projectStore";
import type { ComponentDef, ElementInstance, ParameterDef, ParameterSource, SourceKind } from "../../types";

export const SOURCE_KINDS: SourceKind[] = ["measured", "datasheet", "estimated", "generated", "library default"];
export const CONFIDENCE = ["0 · not checked", "1 · agrees with its source", "2 · source and method checked"];

/** Values that can carry a source: numbers and tables (settings, text and
 *  scripts are choices, not data). */
export const sourced = (p: ParameterDef) => p.type === "number" || p.type === "table1d" || p.type === "table2d";

/** A short tag for a recorded source, its full text in the tooltip. */
export function SourceBadge({ src }: { src?: ParameterSource }) {
  if (!src) return null;
  const tip = `${src.kind} · confidence ${CONFIDENCE[src.confidence]}${src.source ? ` · ${src.source}` : ""}${
    src.note ? ` · ${src.note}` : ""
  }`;
  return (
    <span
      className="ml-1 shrink-0 rounded border border-[color:var(--ss-border)] px-1 text-[9px] leading-3 text-[color:var(--ss-text-dim)]"
      title={tip}
      aria-label={`Source: ${tip}`}
      data-testid="source-badge"
    >
      {src.kind === "library default" ? "default" : src.kind}
      {src.confidence > 0 ? ` ${src.confidence}` : ""}
    </span>
  );
}

/** How many of a part's values have a recorded source, and how many still
 *  hold the library's default. */
export function sourceCounts(el: ElementInstance, def: ComponentDef) {
  const params = def.parameters.filter(sourced);
  const withSource = params.filter((p) => el.parameterSources?.[p.key]).length;
  const atDefault = params.filter((p) => !(p.key in el.parameterOverrides) && !el.parameterSources?.[p.key]).length;
  return { total: params.length, withSource, atDefault };
}

/** CON-13: where each value of the part comes from, how sure it is, and a
 *  form to record or change that. */
export function ValueSources({ element, def }: { element: ElementInstance; def: ComponentDef }) {
  const setParameterSource = useProjectStore((s) => s.setParameterSource);
  const params = def.parameters.filter(sourced);
  const [key, setKey] = useState("");
  const [source, setSource] = useState("");
  const [kind, setKind] = useState<SourceKind>("datasheet");
  const [confidence, setConfidence] = useState<0 | 1 | 2>(0);
  if (params.length === 0) return null;
  const { total, withSource, atDefault } = sourceCounts(element, def);
  const recorded = params.filter((p) => element.parameterSources?.[p.key]);
  const pick = (k: string) => {
    setKey(k);
    const src = element.parameterSources?.[k];
    setSource(src?.source ?? "");
    setKind(src?.kind ?? (k in element.parameterOverrides ? "datasheet" : "library default"));
    setConfidence(src?.confidence ?? 0);
  };
  return (
    <details className="text-[11px]" data-testid="value-sources">
      <summary className="cursor-pointer text-[color:var(--ss-text-dim)]">
        Value sources: {withSource} of {total} recorded, {atDefault} at the library default
      </summary>
      <div className="mt-1 flex flex-col gap-1">
        {recorded.map((p) => {
          const src = element.parameterSources![p.key];
          return (
            <div key={p.key} className="flex items-start gap-1">
              <span className="w-[96px] shrink-0 truncate" title={p.label}>
                {p.label}
              </span>
              <span className="min-w-0 flex-1 text-[color:var(--ss-text-dim)]">
                {src.kind}, confidence {src.confidence}
                {src.source && <span className="block truncate" title={src.source}>{src.source}</span>}
              </span>
              <button
                className="ss-toolbtn px-1"
                title={`Forget the source of ${p.label}`}
                aria-label={`Forget the source of ${p.label}`}
                onClick={() => setParameterSource(element.id, p.key, null)}
              >
                ×
              </button>
            </div>
          );
        })}
        <div className="flex flex-col gap-1 border-t border-[color:var(--ss-border)] pt-1">
          <select className="ss-input" aria-label="Parameter to record a source for" value={key} onChange={(e) => pick(e.target.value)}>
            <option value="">Record a source for…</option>
            {params.map((p) => (
              <option key={p.key} value={p.key}>
                {p.label}
              </option>
            ))}
          </select>
          {key && (
            <>
              <input
                className="ss-input"
                aria-label="Source"
                placeholder="Document, URL or test, with its date"
                value={source}
                onChange={(e) => setSource(e.target.value)}
              />
              <div className="flex gap-1">
                <select className="ss-input flex-1" aria-label="Kind of source" value={kind} onChange={(e) => setKind(e.target.value as SourceKind)}>
                  {SOURCE_KINDS.map((k) => (
                    <option key={k} value={k}>
                      {k}
                    </option>
                  ))}
                </select>
                <select
                  className="ss-input flex-1"
                  aria-label="Confidence"
                  value={confidence}
                  onChange={(e) => setConfidence(Number(e.target.value) as 0 | 1 | 2)}
                >
                  {CONFIDENCE.map((c, i) => (
                    <option key={c} value={i}>
                      {c}
                    </option>
                  ))}
                </select>
              </div>
              <button
                className="ss-toolbtn justify-center border border-[color:var(--ss-border)]"
                onClick={() => {
                  setParameterSource(element.id, key, { source: source.trim(), kind, confidence });
                  setKey("");
                }}
              >
                Save source
              </button>
            </>
          )}
        </div>
      </div>
    </details>
  );
}

import { useEffect, useState } from "react";
import { Download } from "lucide-react";
import { fetchRunSources } from "../../api";
import type { RunSources, SimRun } from "../../types";

const CONFIDENCE = ["source unknown", "known source, not validated", "validated"];

function save(text: string, name: string, type: string) {
  const url = URL.createObjectURL(new Blob([text], { type }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  URL.revokeObjectURL(url);
}

/** Sources & credits (VAL-37): the data and methods a run rests on, their
 *  licences and credits, how far each can be trusted, and citations to save
 *  as BibTeX or CSL-JSON. Asked from the engine with the run's snapshot when
 *  opened. */
export function RunSourcesView({ run }: { run: SimRun }) {
  const [open, setOpen] = useState(false);
  const [data, setData] = useState<RunSources | null>(null);
  const [error, setError] = useState<string | null>(null);
  const snap = run.snapshot;
  useEffect(() => {
    if (!open || !snap || data) return;
    let live = true;
    fetchRunSources(snap.project, snap.case.id)
      .then((d) => live && setData(d))
      .catch((e: unknown) => live && setError(e instanceof Error ? e.message : String(e)));
    return () => {
      live = false;
    };
  }, [open, snap, data]);
  if (!snap) return null;
  const file = `lightsim-sources-${run.caseName.replace(/[^\w-]+/g, "-")}`;
  return (
    <details
      className="border-t border-[color:var(--ss-border)] px-1.5 py-1"
      onToggle={(e) => setOpen((e.currentTarget as HTMLDetailsElement).open)}
    >
      <summary className="cursor-pointer text-[color:var(--ss-text-dim)]">Sources &amp; credits</summary>
      {error && <div className="text-[color:var(--ss-err)]">The engine could not list the sources: {error}</div>}
      {!data && !error && <div className="text-[color:var(--ss-text-dim)]">Asking the engine…</div>}
      {data && (
        <div role="region" aria-label="Sources and credits" className="mt-1 flex flex-col gap-1">
          {data.unknownProvenance && (
            <div className="text-[color:var(--ss-warn)]">
              This run uses values whose source is unknown (library defaults marked
              &ldquo;source unknown&rdquo;): its results rest on them.
            </div>
          )}
          <ul className="m-0 flex list-none flex-col gap-0.5 p-0">
            {data.sources.map((s) => (
              <li key={s.id} className="break-words" title={[s.source, s.licence && `Licence: ${s.licence}`].filter(Boolean).join("\n")}>
                <span className="font-mono text-[color:var(--ss-text-dim)]">{s.kind === "own" ? "own" : s.id}</span>{" "}
                {s.title}
                {s.kind !== "method" && (
                  <span className={s.confidence === 0 ? "text-[color:var(--ss-warn)]" : "text-[color:var(--ss-text-dim)]"}>
                    {" "}
                    · {CONFIDENCE[s.confidence] ?? ""}
                  </span>
                )}
                {s.usedBy.length > 0 && (
                  <span className="text-[color:var(--ss-text-dim)]">
                    {" "}
                    · used by {s.usedBy.slice(0, 3).join(", ")}
                    {s.usedBy.length > 3 && ` and ${s.usedBy.length - 3} more`}
                  </span>
                )}
              </li>
            ))}
          </ul>
          {data.credits.length > 0 && (
            <div>
              <div className="text-[10px] text-[color:var(--ss-text-dim)]">Credits the data asks for</div>
              {data.credits.map((c) => (
                <div key={c}>{c}</div>
              ))}
            </div>
          )}
          <div className="flex gap-1">
            <button
              className="ss-toolbtn border border-[color:var(--ss-border)] px-1.5"
              title="Save the citations (LightSim and every source) as a BibTeX file"
              onClick={() => save(data.bibtex, `${file}.bib`, "application/x-bibtex")}
            >
              <Download size={12} /> BibTeX
            </button>
            <button
              className="ss-toolbtn border border-[color:var(--ss-border)] px-1.5"
              title="Save the citations as CSL-JSON (Zotero, Pandoc)"
              onClick={() => save(JSON.stringify(data.cslJson, null, 2), `${file}.json`, "application/json")}
            >
              <Download size={12} /> CSL-JSON
            </button>
          </div>
        </div>
      )}
    </details>
  );
}

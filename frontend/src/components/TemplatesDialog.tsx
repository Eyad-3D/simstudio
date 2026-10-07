import { useEffect, useMemo, useState } from "react";
import * as api from "../api";
import { confirmReplaceProject, useProjectStore } from "../store/projectStore";
import type { ParamValue } from "../types";

const SLOTS = ["Battery", "E-Drive 1", "E-Drive 2", "Engine", "Transmission", "Driveline", "Chassis", "Brakes", "Driver",
  "Accessories", "Controller", "Fuel Cell"];

/** CON-18: start a project from a pre-wired template by answering its short
 *  form, or save the open model as a template of your own. */
export function TemplatesDialog({ onClose }: { onClose: () => void }) {
  const [tab, setTab] = useState<"new" | "save">("new");
  const [list, setList] = useState<api.VehicleTemplate[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    void api.listTemplates().then(setList);
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/40">
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Vehicle templates"
        className="flex max-h-[90vh] w-[620px] max-w-[94vw] flex-col overflow-hidden rounded-md border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] shadow-2xl"
      >
        <div className="flex items-center gap-2 border-b border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] px-4 py-2 text-[13px]">
          <span className="font-semibold">Vehicle templates</span>
          <span className="ml-auto flex gap-1" role="tablist">
            {(
              [
                ["new", "New from a template"],
                ["save", "Save this model as a template"],
              ] as const
            ).map(([id, label]) => (
              <button
                key={id}
                role="tab"
                aria-selected={tab === id}
                className={`ss-toolbtn px-2 ${tab === id ? "border border-[color:var(--ss-border)]" : ""}`}
                onClick={() => {
                  setTab(id);
                  setError(null);
                }}
              >
                {label}
              </button>
            ))}
          </span>
        </div>
        <div className="flex flex-col gap-2 overflow-y-auto px-4 py-3 text-[12px]">
          {error && (
            <p role="alert" className="text-red-600">
              {error}
            </p>
          )}
          {tab === "new" ? (
            <NewFromTemplate list={list} onError={setError} onDone={onClose} onListChange={setList} />
          ) : (
            <SaveAsTemplate
              onError={setError}
              onSaved={(t) => {
                setList((l) => [...(l ?? []).filter((x) => x.id !== t.id), t]);
                setTab("new");
              }}
            />
          )}
        </div>
        <div className="flex justify-end border-t border-[color:var(--ss-border)] px-4 py-2.5">
          <button className="ss-toolbtn border border-[color:var(--ss-border)] px-3" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

function NewFromTemplate({
  list,
  onError,
  onDone,
  onListChange,
}: {
  list: api.VehicleTemplate[] | null;
  onError: (e: string | null) => void;
  onDone: () => void;
  onListChange: (l: api.VehicleTemplate[]) => void;
}) {
  const openNewProject = useProjectStore((s) => s.openNewProject);
  const [chosen, setChosen] = useState<string>("");
  const [values, setValues] = useState<Record<string, string>>({});
  const [name, setName] = useState("");
  const t = list?.find((x) => x.id === chosen);
  if (!list) return <p className="text-[color:var(--ss-text-dim)]">Loading the templates…</p>;
  if (list.length === 0) return <p>No templates: the engine is not running.</p>;

  const create = async () => {
    if (!t) return;
    onError(null);
    if (!(await confirmReplaceProject("Starting a project from a template"))) return;
    const vals: Record<string, ParamValue> = {};
    t.form.forEach((f, i) => {
      const raw = values[String(i)];
      if (raw !== undefined && raw !== "") vals[String(i)] = typeof f.default === "number" ? Number(raw) : raw;
    });
    try {
      const project = await api.newFromTemplate(t.id, vals, name.trim() || `${t.name} (new)`);
      openNewProject(project, `New project '${project.name}' from the template '${t.name}' (version ${t.version}); save it to keep it.`);
      onDone();
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <>
      <div className="flex flex-col gap-1" role="radiogroup" aria-label="Template">
        {list.map((x) => (
          <label key={x.id} className="flex items-start gap-2 rounded border border-[color:var(--ss-border)] p-1.5">
            <input
              type="radio"
              name="template"
              checked={chosen === x.id}
              onChange={() => {
                setChosen(x.id);
                setValues({});
                setName("");
              }}
            />
            <span className="min-w-0 flex-1">
              <span className="font-semibold">{x.name}</span>
              {!x.builtin && <span className="text-[color:var(--ss-text-dim)]"> · yours, version {x.version}</span>}
              <span className="block text-[11px] text-[color:var(--ss-text-dim)]">{x.description}</span>
              <span className="block text-[11px] text-[color:var(--ss-text-dim)]">
                Slots: {Object.keys(x.slots).join(", ") || "none"}
              </span>
            </span>
            {!x.builtin && (
              <button
                className="ss-toolbtn px-1"
                aria-label={`Delete the template ${x.name}`}
                title="Delete this template of yours"
                onClick={async (e) => {
                  e.preventDefault();
                  try {
                    await api.deleteTemplate(x.id);
                    onListChange(list.filter((y) => y.id !== x.id));
                  } catch (err) {
                    onError(err instanceof Error ? err.message : String(err));
                  }
                }}
              >
                ×
              </button>
            )}
          </label>
        ))}
      </div>
      {t && (
        <div className="flex flex-col gap-1 border-t border-[color:var(--ss-border)] pt-2">
          <label className="flex items-center gap-2">
            <span className="w-[180px] shrink-0">Project name</span>
            <input
              className="ss-input flex-1"
              aria-label="Project name"
              value={name}
              placeholder={`${t.name} (new)`}
              onChange={(e) => setName(e.target.value)}
            />
          </label>
          {t.form.map((f, i) => (
            <label key={i} className="flex items-center gap-2" title={f.help || undefined}>
              <span className="w-[180px] shrink-0">{f.label}</span>
              <input
                className="ss-input w-[110px]"
                aria-label={f.label}
                inputMode="decimal"
                value={values[String(i)] ?? String(f.default)}
                onChange={(e) => setValues((v) => ({ ...v, [String(i)]: e.target.value }))}
              />
              <span className="text-[color:var(--ss-text-dim)]">{f.unit}</span>
            </label>
          ))}
          <button
            className="mt-1 self-end rounded bg-[color:var(--ss-accent-fill)] px-3 py-1 text-[12px] font-semibold text-white hover:brightness-110"
            onClick={() => void create()}
          >
            Create project
          </button>
        </div>
      )}
    </>
  );
}

function SaveAsTemplate({
  onError,
  onSaved,
}: {
  onError: (e: string | null) => void;
  onSaved: (t: api.VehicleTemplate) => void;
}) {
  const project = useProjectStore((s) => s.project);
  const libraryById = useProjectStore((s) => s.libraryById);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [asked, setAsked] = useState<Set<string>>(new Set());
  const [slots, setSlots] = useState<Record<string, string>>({});
  // every number parameter of every part: what the form can ask
  const candidates = useMemo(() => {
    const out: { id: string; field: api.TemplateField }[] = [];
    for (const s of project?.systems ?? []) {
      for (const el of s.elements) {
        for (const p of libraryById[el.componentDefId]?.parameters ?? []) {
          if (p.type !== "number") continue;
          out.push({
            id: `${el.id}.${p.key}`,
            field: {
              elementId: el.id,
              key: p.key,
              label: `${el.label} · ${p.label}`,
              unit: p.unit ?? "",
              default: el.parameterOverrides[p.key] ?? p.default,
              minimum: p.minimum ?? null,
              maximum: p.maximum ?? null,
              help: p.description ?? "",
            },
          });
        }
      }
    }
    return out;
  }, [project, libraryById]);
  const parts = project?.systems.flatMap((s) => s.elements) ?? [];
  if (!project) return <p>Open a model first.</p>;

  const save = async () => {
    onError(null);
    try {
      const t = await api.saveTemplate({
        project,
        name: name.trim(),
        description: description.trim(),
        form: candidates.filter((c) => asked.has(c.id)).map((c) => c.field),
        slots: Object.fromEntries(Object.entries(slots).filter(([, v]) => v)),
      });
      onSaved(t);
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="flex flex-col gap-1">
      <p className="text-[color:var(--ss-text-dim)]">
        The template keeps a copy of this model. A new project from it asks only the values you tick below. Templates are
        saved in the projects folder, under templates/, so you can hand one out with its file.
      </p>
      <input className="ss-input" aria-label="Template name" placeholder="Template name" value={name} onChange={(e) => setName(e.target.value)} />
      <input
        className="ss-input"
        aria-label="Template description"
        placeholder="What it is for, in a sentence"
        value={description}
        onChange={(e) => setDescription(e.target.value)}
      />
      <details>
        <summary className="cursor-pointer">Slots: which part plays which role</summary>
        {SLOTS.map((slot) => (
          <label key={slot} className="flex items-center gap-2">
            <span className="w-[120px] shrink-0">{slot}</span>
            <select
              className="ss-input flex-1"
              aria-label={`Slot ${slot}`}
              value={slots[slot] ?? ""}
              onChange={(e) => setSlots((s) => ({ ...s, [slot]: e.target.value }))}
            >
              <option value="">(none)</option>
              {parts.map((el) => (
                <option key={el.id} value={el.id}>
                  {el.label}
                </option>
              ))}
            </select>
          </label>
        ))}
      </details>
      <details open>
        <summary className="cursor-pointer">Values the form asks ({asked.size})</summary>
        <div className="max-h-[220px] overflow-y-auto">
          {candidates.map((c) => (
            <label key={c.id} className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={asked.has(c.id)}
                onChange={(e) =>
                  setAsked((prev) => {
                    const next = new Set(prev);
                    if (e.target.checked) next.add(c.id);
                    else next.delete(c.id);
                    return next;
                  })
                }
              />
              {c.field.label}
              <span className="text-[color:var(--ss-text-dim)]">
                {String(c.field.default)} {c.field.unit}
              </span>
            </label>
          ))}
        </div>
      </details>
      <button
        className="mt-1 self-end rounded bg-[color:var(--ss-accent-fill)] px-3 py-1 text-[12px] font-semibold text-white hover:brightness-110 disabled:opacity-40"
        disabled={!name.trim()}
        onClick={() => void save()}
      >
        Save as template
      </button>
    </div>
  );
}

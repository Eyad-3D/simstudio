// Results that no longer match the model on screen (UX-41): what changed
// since the run shown in Results, from the model it stored (RES-09), so the
// diagram can mark the parts, wires and cases and Results can say "these
// results are from before 3 changes".

import { canonicalJson, diffSnapshots, type ModelChange } from "./provenance";
import type { ComponentDef, Project, RunSnapshot, SimRun } from "./types";

export interface Staleness {
  /** what changed, as Run info lists it: part, parameter, old → new with units */
  changes: ModelChange[];
  /** parts edited or added since the run */
  elementIds: Set<string>;
  /** canvas wires added since the run */
  wireIds: Set<string>;
  /** cases edited or added since the run */
  caseIds: Set<string>;
}

const NONE: Staleness = { changes: [], elementIds: new Set(), wireIds: new Set(), caseIds: new Set() };

// one answer per (run, project, library): the diagram's nodes and Results ask
// for the same one on every render
let last: { run: SimRun; project: Project; lib: Record<string, ComponentDef>; out: Staleness } | null = null;

/** What changed in `project` since `run` was made; nothing for a run that
 *  kept no model (made before 0.2) or a run still going. Live edits made
 *  during the run are not changes to the model on screen. */
export function changesSince(run: SimRun | null, project: Project | null, lib: Record<string, ComponentDef>): Staleness {
  if (!run?.snapshot || !project || run.status === "running") return NONE;
  if (last && last.run === run && last.project === project && last.lib === lib) return last.out;
  const snap = run.snapshot;
  // the case as the project held it (a sweep's value is the run's own, not a change)
  const caseThen = snap.project.cases.find((c) => c.id === snap.case.id) ?? snap.case;
  const caseNow = project.cases.find((c) => c.id === snap.case.id) ?? caseThen;
  const base: RunSnapshot = { ...snap, case: caseThen, liveEdits: [] };
  const next: RunSnapshot = { project, case: caseNow, appVersion: null, liveEdits: [] };
  const changes = diffSnapshots(base, next, lib);

  const elementIds = new Set(changes.flatMap((c) => (c.elementId ? [c.elementId] : [])));
  const ends = (s: Project) =>
    new Set(
      s.systems.flatMap((sy) =>
        sy.connections.map((c) =>
          [`${c.sourceElementId}:${c.sourcePortId}`, `${c.targetElementId}:${c.targetPortId}`].sort().join("|"),
        ),
      ),
    );
  const then = ends(snap.project);
  const wireIds = new Set(
    project.systems.flatMap((sy) =>
      sy.connections
        .filter(
          (c) => !then.has([`${c.sourceElementId}:${c.sourcePortId}`, `${c.targetElementId}:${c.targetPortId}`].sort().join("|")),
        )
        .map((c) => c.id),
    ),
  );
  // a case's settings and overrides (its name changes no result)
  const setting = (c: Project["cases"][number]) => canonicalJson({ ...c, name: undefined, realtimeFactor: undefined });
  const casesThen = new Map(snap.project.cases.map((c) => [c.id, setting(c)]));
  const caseIds = new Set(project.cases.filter((c) => casesThen.get(c.id) !== setting(c)).map((c) => c.id));

  const out = { changes, elementIds, wireIds, caseIds };
  last = { run, project, lib, out };
  return out;
}

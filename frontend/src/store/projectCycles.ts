/** Drive cycles of the user's own, kept in the project (CON-11).
 *
 *  The project file lists them in `cycles`; a Driving Task or Road Profile
 *  names one by id in its `cycle`, as it names a bundled cycle. This module
 *  keeps the store's cycle list (`cycles`, which the pickers, the library
 *  search and setDrivingCycle read) as the bundled cycles followed by the
 *  open project's own ones that have a speed, and adds, renames and removes
 *  a project's cycles as one undo step each. A cycle with a grade only
 *  serves a Road Profile: the pickers take it from the project. */
import type * as api from "../api";
import type { Project, ProjectCycle } from "../types";
import { useProjectStore } from "./projectStore";

/** No bundled cycle's id starts with this. */
export const OWN_PREFIX = "own:";

/** The figures the pickers show for a cycle of one's own, as the engine
 *  computes them (app/cycles.py own_info). Against distance, duration_s is
 *  the time its own speeds take to cover it. */
export function ownCycleInfo(c: ProjectCycle): api.CycleInfo {
  const x = c.x;
  const speed = c.speed ?? null;
  const span = x.length > 1 ? x[x.length - 1] - x[0] : 0;
  let km = 0;
  let duration = 0;
  if (c.axis === "distance") {
    km = span / 1000;
    if (speed)
      for (let i = 1; i < x.length; i++) {
        const v = (speed[i - 1] + speed[i]) / 2;
        if (v > 0) duration += (x[i] - x[i - 1]) / (v / 3.6);
      }
  } else {
    duration = span;
    if (speed) for (let i = 1; i < x.length; i++) km += ((x[i] - x[i - 1]) * (speed[i - 1] + speed[i])) / 2 / 3600;
  }
  return {
    id: c.id,
    name: c.name,
    region: "This project",
    register: "",
    phases: [],
    source: c.source ?? "",
    note: c.note ?? "",
    duration_s: Math.round(duration * 1000) / 1000,
    distance_km: Math.round(km * 1000) / 1000,
    vmax_kmh: speed && speed.length ? Math.max(...speed) : 0,
    grade: c.grade != null,
    speed: speed != null,
    axis: c.axis,
    own: true,
  };
}

/** The project's own cycles, as the pickers list them. */
export const ownCycleInfos = (project: Project | null): api.CycleInfo[] => (project?.cycles ?? []).map(ownCycleInfo);

/** A new id for a cycle called `name`: "own:" and its name in lower case,
 *  numbered when the project already has it. */
export function newCycleId(name: string, project: Project | null): string {
  const slug =
    name
      .normalize("NFKD")
      .replace(/[\u0300-\u036f]/g, "")
      .toLowerCase()
      .replace(/[^a-z0-9._-]+/g, "-")
      .replace(/^[-.]+|-+$/g, "")
      .slice(0, 56) || "cycle";
  const taken = new Set((project?.cycles ?? []).map((c) => c.id));
  let id = `${OWN_PREFIX}${slug}`;
  for (let n = 2; taken.has(id); n++) id = `${OWN_PREFIX}${slug}-${n}`;
  return id;
}

/** Where a cycle is used: "Driving Task 'Vehicle Task'", "case 'Commute'". */
export function cycleUsers(project: Project, id: string): string[] {
  const out: string[] = [];
  const labels = new Map<string, string>();
  for (const s of project.systems)
    for (const e of s.elements) {
      labels.set(e.id, e.label);
      if (e.parameterOverrides?.cycle === id) out.push(`'${e.label}'`);
    }
  for (const c of project.cases)
    for (const [elId, ov] of Object.entries(c.parameterOverrides ?? {}))
      if (ov?.cycle === id) out.push(`'${labels.get(elId) ?? elId}' in case '${c.name}'`);
  return out;
}

/** Change the open project as one undo step (the store's own history). */
function edit(fn: (draft: Project) => void): boolean {
  const s = useProjectStore.getState();
  if (!s.project || s.readOnly) return false;
  const draft = structuredClone(s.project);
  fn(draft);
  s.beginHistory();
  useProjectStore.setState({ project: draft, dirty: true });
  return true;
}

/** Keep a cycle in the open project; returns its id (null: read-only). */
export function addProjectCycle(cycle: Omit<ProjectCycle, "id"> & { id?: string }): string | null {
  const s = useProjectStore.getState();
  const id = cycle.id ?? newCycleId(cycle.name, s.project);
  const kept: ProjectCycle = { ...cycle, id };
  if (!edit((p) => (p.cycles = [...(p.cycles ?? []).filter((c) => c.id !== id), kept]))) return null;
  syncCycleList();
  s.log(
    "info",
    `Drive cycle '${cycle.name}' (${cycle.x.length.toLocaleString("en")} points) is kept in the project; save the project to keep it.`,
  );
  return id;
}

export function renameProjectCycle(id: string, name: string): void {
  const trimmed = name.trim();
  if (!trimmed) return;
  edit((p) => {
    const c = p.cycles?.find((cc) => cc.id === id);
    if (c) c.name = trimmed;
  });
  syncCycleList();
}

/** Remove a cycle no part or case uses; says where it is used otherwise. */
export function removeProjectCycle(id: string): boolean {
  const s = useProjectStore.getState();
  const c = s.project?.cycles?.find((cc) => cc.id === id);
  if (!s.project || !c) return false;
  const users = cycleUsers(s.project, id);
  if (users.length) {
    s.log("warning", `Drive cycle '${c.name}' is used by ${users.join(", ")}: pick another cycle there first.`);
    return false;
  }
  const done = edit((p) => {
    p.cycles = (p.cycles ?? []).filter((cc) => cc.id !== id);
    if (p.cycles.length === 0) delete p.cycles;
  });
  syncCycleList();
  if (done) s.log("info", `Drive cycle '${c.name}' removed from the project.`);
  return done;
}

let lastKey = "";

/** Make the store's cycle list the bundled cycles plus the open project's
 *  own ones with a speed, when that changed. */
export function syncCycleList(): void {
  const s = useProjectStore.getState();
  const own = ownCycleInfos(s.project).filter((c) => c.speed);
  const key = JSON.stringify(own);
  const listed = s.cycles.filter((c) => c.own);
  if (key === lastKey && JSON.stringify(listed) === key) return;
  lastKey = key;
  useProjectStore.setState({
    cycles: [...s.cycles.filter((c) => !c.own), ...own],
  });
}

let installed = false;

/** Follow the open project (opened, edited, undone) from now on. */
export function followProjectCycles(): void {
  if (installed) return;
  installed = true;
  useProjectStore.subscribe((state, prev) => {
    if (state.project?.cycles !== prev.project?.cycles || state.cycles !== prev.cycles) syncCycleList();
  });
  syncCycleList();
}

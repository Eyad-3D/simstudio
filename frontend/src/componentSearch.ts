// How a typed search finds library parts: the Components panel's search and
// the diagram's add-a-part box use this one matching.
import type { ComponentDef } from "./types";

// Other names people search for. The library names follow its own vocabulary
// ("E-Motor", "HV Battery Pack"); these are matched like the descriptions.
const SYNONYMS: Record<string, string> = {
  "battery.generic": "accumulator energy storage ess cell",
  "motor.emotor": "inverter electric machine traction motor generator",
  "controller.dcdc": "dcdc converter",
  "engine.combustion": "ice internal combustion petrol gasoline diesel",
  "fuel.tank": "petrol gasoline diesel",
  "mech.gearbox": "transmission",
  "mech.final_drive": "axle reduction",
  "mech.transfer_case": "awd 4wd",
  "electric.constant_drive": "load auxiliaries hvac",
  "electric.node": "bus busbar junction",
  "boundary.ground": "earth",
  "propulsion.wheel": "tyre tire",
  "vehicle.body": "chassis car",
  "signal.driving_task": "drive cycle speed profile wltp wltc nedc epa ftp",
  "signal.road_profile": "slope gradient hill",
  "signal.script": "python code",
  "signal.fmu": "fmi functional mock-up simulink dymola gt-suite modelica co-simulation import",
  "signal.monitor": "scope probe",
  "container.system": "subsystem group",
  "fuel.h2_tank": "h2",
};

/** The search's words, lower case. */
export const searchTerms = (query: string): string[] => query.trim().toLowerCase().split(/\s+/).filter(Boolean);

/** Every search term must hit: anywhere in the name, category or id, or at
 *  the start of a word in the description or synonyms (so "ice" finds the
 *  engine but not "device"). */
export function matchesSearch(def: ComponentDef, terms: string[]): boolean {
  const direct = `${def.name} ${def.category} ${def.id}`.toLowerCase();
  const words = `${def.description ?? ""} ${SYNONYMS[def.id] ?? ""}`.toLowerCase().split(/[^a-z0-9]+/);
  return terms.every((t) => direct.includes(t) || words.some((w) => w.startsWith(t)));
}

/** The parts a search finds, in the library's order (all of them for an empty search). */
export function searchLibrary(library: ComponentDef[], query: string): ComponentDef[] {
  const terms = searchTerms(query);
  return terms.length ? library.filter((c) => matchesSearch(c, terms)) : library;
}

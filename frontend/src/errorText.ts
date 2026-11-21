// The engine's error answers as sentences a person reads. FastAPI answers
// with `detail` as a string, as a list of validation errors (`loc`, `msg`)
// or, from a few handlers, as an object; each reaches the user as plain
// text that names the part and the parameter where the error says which.

import fallbackLibrary from "./data/componentLibrary.json";
import type { ComponentDef } from "./types";

const PARAMS: Map<string, Map<string, { label: string; type: string }>> = new Map(
  (fallbackLibrary as { components: ComponentDef[] }).components.map((d) => [
    d.id,
    new Map(d.parameters.map((p) => [p.key, { label: p.label, type: p.type }])),
  ]),
);

/** How many validation errors a message lists before "and N more". */
const SHOWN = 5;

type Loc = (string | number)[];
type Obj = Record<string, unknown>;

const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);

/** Where a validation error is: the words that name it, and the part and
 *  parameter it is about when the path leads to one. */
interface Where {
  words: string[];
  elementId?: string;
  part?: string;
  defId?: string;
  paramKey?: string;
}

/** The parts of a request's body, by id (a case's values name them by id). */
function partsOf(body: unknown): Map<string, Obj> {
  const project = isObj(body) && isObj(body.project) ? body.project : body;
  const systems = isObj(project) && Array.isArray(project.systems) ? project.systems : [];
  const out = new Map<string, Obj>();
  for (const s of systems) {
    for (const e of isObj(s) && Array.isArray(s.elements) ? s.elements : []) {
      if (isObj(e) && typeof e.id === "string") out.set(e.id, e);
    }
  }
  return out;
}

/** Where a validation error is, walking `loc` through the request's own
 *  body: a part is named by its label and a parameter by the library's label
 *  ("HV Battery · Capacity"), a place it cannot resolve by its keys ("cases 2
 *  · duration"). The path ends at a parameter: what follows is inside its
 *  value (a table's row, or which of the value's allowed types it was tried
 *  as), which adds nothing a person reads. */
function whereOf(loc: Loc, body: unknown): Where {
  const path = loc[0] === "body" ? loc.slice(1) : loc;
  const where: Where = { words: [] };
  const { words } = where;
  let node: unknown = body;
  const param = (el: Obj | undefined, key: string | number | undefined) => {
    if (key === undefined) return;
    if (el && typeof el.componentDefId === "string") where.defId = el.componentDefId;
    if (el && typeof el.id === "string") where.elementId = el.id;
    if (el && typeof el.label === "string") where.part = el.label;
    where.paramKey = String(key);
    words.push(PARAMS.get(where.defId ?? "")?.get(where.paramKey)?.label ?? where.paramKey);
  };
  for (let i = 0; i < path.length; i++) {
    const key = path[i];
    const next: unknown = Array.isArray(node) ? node[Number(key)] : isObj(node) ? node[String(key)] : undefined;
    if (key === "parameterOverrides" && isObj(node) && typeof node.componentDefId === "string") {
      param(node, path[i + 1]); // a part's own value
      break;
    } else if (key === "parameterOverrides" && path[i + 1] !== undefined) {
      // a case's value: {element id: {key: value}}
      const el = partsOf(body).get(String(path[i + 1]));
      if (el && typeof el.label === "string") words.push(el.label);
      param(el, path[i + 2]);
      break;
    } else if (isObj(next) && typeof next.label === "string" && typeof next.componentDefId === "string") {
      words.length = 0; // a part: its label says where it is
      words.push(next.label);
      where.elementId = typeof next.id === "string" ? next.id : undefined;
      where.part = next.label;
    } else if (isObj(next) && typeof next.name === "string" && typeof key === "number") {
      words.push(next.name);
    } else if (typeof key === "number") {
      if (next === undefined) words.push(`item ${key + 1}`);
      // an index inside a value (a table's row) adds nothing a person reads
    } else if (!["project", "systems", "elements", "cases"].includes(key)) {
      words.push(key);
    }
    node = next;
  }
  return where;
}

const placeOf = (loc: Loc, body: unknown): string => whereOf(loc, body).words.join(" · ");

/** One refusal of a project, as a row of the Problems list: the part and the
 *  parameter it is about (when the engine's path leads to one), what is
 *  wrong and the value it got; `code` is the engine's own type and path, for
 *  a report. */
export interface Refusal {
  elementId?: string;
  /** the part's label, as the request named it */
  part?: string;
  paramKey?: string;
  text: string;
  code: string;
}

/** What a parameter of this type must be, for a value none of its allowed
 *  types took (the engine says it once per type). */
function mustBe(type: string | undefined): string | null {
  if (type === "number") return "must be a number";
  if (type === "boolean") return "must be on or off";
  if (type === "enum") return "must be one of its choices";
  if (type === "string" || type === "code" || type === "file") return "must be text";
  if (type && ["table1d", "table2d", "table", "maps"].includes(type)) return "must be a table";
  return null;
}

/** The value an error got, in words: "" is shown as '', nothing as nothing. */
function gotText(e: Obj): string {
  if (!("input" in e)) return "";
  const v = e.input;
  if (v === null || v === undefined) return " (got nothing)";
  if (typeof v === "string") return ` (got '${v.length > 40 ? `${v.slice(0, 40)}…` : v}')`;
  if (typeof v === "number" || typeof v === "boolean") return ` (got ${v})`;
  return "";
}

/** The rows a validation refusal lists: one per place, the engine's several
 *  errors for one value (one per type it may have) made one. Null when
 *  `detail` is not a list of validation errors. `body` is the request's own
 *  body (parsed or as sent). */
export function refusalsOf(detail: unknown, body?: unknown): Refusal[] | null {
  if (!Array.isArray(detail) || detail.length === 0 || !detail.every((d) => isObj(d) && Array.isArray(d.loc))) return null;
  const parsed = parseBody(body);
  const groups = new Map<string, { where: Where; errors: Obj[] }>();
  for (const e of detail as Obj[]) {
    const where = whereOf(e.loc as Loc, parsed);
    const key = where.paramKey ? `${where.elementId}|${where.paramKey}` : (e.loc as Loc).join(".");
    const g = groups.get(key);
    if (g) g.errors.push(e);
    else groups.set(key, { where, errors: [e] });
  }
  return [...groups.values()].map(({ where, errors }) => {
    const first = errors[0];
    const msg = typeof first.msg === "string" ? first.msg : JSON.stringify(first);
    const type = where.defId && where.paramKey ? PARAMS.get(where.defId)?.get(where.paramKey)?.type : undefined;
    const what = (errors.length > 1 && mustBe(type)) || msg;
    // the part is named on its own (a column of the list): the other words
    const at = where.part ? where.words.indexOf(where.part) : -1;
    const place = where.words.filter((_, i) => i !== at).join(" · ");
    return {
      elementId: where.elementId,
      part: where.part,
      paramKey: where.paramKey,
      text: `${place ? `${place}: ` : ""}${what}${gotText(first)}`,
      code: [...new Set(errors.map((e) => String(e.type ?? "")))].join(", ") + ` at ${(first.loc as Loc).join(".")}`,
    };
  });
}

function parseBody(body: unknown): unknown {
  if (typeof body !== "string") return body;
  try {
    return JSON.parse(body);
  } catch {
    return undefined;
  }
}

/** One validation error as a sentence: "HV Battery · Capacity: Input should be a valid number". */
function validationText(e: Obj, body: unknown): string {
  const msg = typeof e.msg === "string" ? e.msg : typeof e.message === "string" ? e.message : JSON.stringify(e);
  const place = Array.isArray(e.loc) ? placeOf(e.loc as Loc, body) : "";
  return place ? `${place}: ${msg}` : msg;
}

/** The text of an error answer's `detail`. `body` is the request's own body
 *  (parsed or as sent), used to name the parts and fields an error is about. */
export function detailText(detail: unknown, body?: unknown): string {
  const parsed = parseBody(body);
  if (typeof detail === "string") return detail;
  if (Array.isArray(detail)) {
    const rows = refusalsOf(detail, parsed);
    const lines = rows
      ? rows.map((r) => (r.part ? `${r.part} · ${r.text}` : r.text))
      : detail.map((d) => (isObj(d) ? validationText(d, parsed) : String(d)));
    if (lines.length === 0) return "The engine refused the request without saying why.";
    if (lines.length === 1) return lines[0];
    const head = `${lines.length} problems: `;
    const more = lines.length > SHOWN ? `; and ${lines.length - SHOWN} more` : "";
    return `${head}${lines.slice(0, SHOWN).join("; ")}${more}.`.replace(/\.\.$/, ".");
  }
  if (isObj(detail)) {
    if ("loc" in detail || "msg" in detail) return validationText(detail, parsed);
    if (typeof detail.message === "string") return detail.message;
    return Object.entries(detail)
      .map(([k, v]) => `${k}: ${typeof v === "string" ? v : JSON.stringify(v)}`)
      .join("; ");
  }
  if (detail == null) return "";
  return String(detail);
}

/** The text of a failed response: its `detail` as sentences, else the
 *  status's own words. Reads the response's body. */
export async function responseText(res: Response, body?: unknown): Promise<string> {
  return (await readError(res, body)).text;
}

/** A failed response's text, and its rows when it refuses a project's values
 *  (a list of validation errors). Reads the response's body. */
export async function readError(res: Response, body?: unknown): Promise<{ text: string; refusals: Refusal[] | null }> {
  try {
    const json: unknown = await res.json();
    const detail = isObj(json) && "detail" in json ? json.detail : json;
    const text = detailText(detail, body);
    if (text) return { text, refusals: refusalsOf(detail, body) };
  } catch {
    /* not JSON: the status's own words */
  }
  return { text: res.statusText || `The engine answered ${res.status}.`, refusals: null };
}

/** The engine answered 404 (the thing is not there: already gone). By the
 *  error's status (an ApiError), or the "404 …" an older message began with. */
export function isNotFound(e: unknown): boolean {
  const status = (e as { status?: unknown } | null)?.status;
  if (typeof status === "number") return status === 404;
  return e instanceof Error && /^404\b/.test(e.message);
}

/** Why a request failed, for a person: the engine's own words, or that it
 *  could not be reached (fetch fails with a TypeError when nothing answers). */
export function failureText(e: unknown): string {
  if (e instanceof TypeError) return "the engine could not be reached";
  return e instanceof Error ? e.message : String(e);
}

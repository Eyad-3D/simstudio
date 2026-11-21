// The engine's error answers as sentences a person reads. FastAPI answers
// with `detail` as a string, as a list of validation errors (`loc`, `msg`)
// or, from a few handlers, as an object; each reaches the user as plain
// text that names the part and the parameter where the error says which.

import fallbackLibrary from "./data/componentLibrary.json";
import type { ComponentDef } from "./types";

const LABELS: Map<string, Map<string, string>> = new Map(
  (fallbackLibrary as { components: ComponentDef[] }).components.map((d) => [
    d.id,
    new Map(d.parameters.map((p) => [p.key, p.label])),
  ]),
);

/** How many validation errors a message lists before "and N more". */
const SHOWN = 5;

type Loc = (string | number)[];
type Obj = Record<string, unknown>;

const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);

/** Where a validation error is, in words: walks `loc` through the request's
 *  own body, so a part is named by its label and a parameter by the
 *  library's label ("HV Battery · Capacity"); a place it cannot resolve is
 *  named by its keys ("cases 2 · duration"). */
function placeOf(loc: Loc, body: unknown): string {
  const path = loc[0] === "body" ? loc.slice(1) : loc;
  const words: string[] = [];
  let node: unknown = body;
  let defId: string | null = null;
  for (let i = 0; i < path.length; i++) {
    const key = path[i];
    const next: unknown = Array.isArray(node) ? node[Number(key)] : isObj(node) ? node[String(key)] : undefined;
    if (isObj(next) && typeof next.label === "string" && typeof next.componentDefId === "string") {
      words.length = 0; // a part: its label says where it is
      words.push(next.label);
      defId = next.componentDefId;
    } else if (isObj(next) && typeof next.name === "string" && typeof key === "number") {
      words.push(next.name);
    } else if (typeof key === "number") {
      if (next === undefined) words.push(`item ${key + 1}`);
      // an index inside a value (a table's row) adds nothing a person reads
    } else if (key === "parameterOverrides" && defId) {
      const param = path[i + 1];
      if (param !== undefined) {
        words.push(LABELS.get(defId)?.get(String(param)) ?? String(param));
        i++;
      }
    } else if (!["project", "systems", "elements", "cases"].includes(key)) {
      words.push(key);
    }
    node = next;
  }
  return words.join(" · ");
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
  let parsed = body;
  if (typeof body === "string") {
    try {
      parsed = JSON.parse(body);
    } catch {
      parsed = undefined;
    }
  }
  if (typeof detail === "string") return detail;
  if (Array.isArray(detail)) {
    const lines = detail.map((d) => (isObj(d) ? validationText(d, parsed) : String(d)));
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
  try {
    const json: unknown = await res.json();
    const text = isObj(json) && "detail" in json ? detailText(json.detail, body) : detailText(json, body);
    if (text) return text;
  } catch {
    /* not JSON: the status's own words */
  }
  return res.statusText || `The engine answered ${res.status}.`;
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

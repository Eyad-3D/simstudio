// Code a project carries that runs on the user's computer when the model
// runs (STD-02): attached FMUs, AI models or programs. Before such a
// project's first run the app asks once whether the user trusts it; the
// engine remembers the answer by the fingerprint below (see
// backend/app/trust.py), so the same code is not asked about again and
// changed code is. Script blocks have a review of their own, which shows
// their code and which the engine enforces (PLT-35, backend/app/script_trust.py):
// codeOf leaves them out unless asked.
import type { ComponentDef, Project } from "./types";

/** File extensions of attachments that hold code (backend/app/attachments.py). */
const EXECUTABLE = /\.(fmu|onnx|py|dll|so|dylib|exe)$/i;
const SCRIPT = "signal.script";

export interface ProjectCode {
  /** what to name in the question, e.g. "2 Script blocks", "motor.fmu" */
  items: string[];
  /** canonical text of the code, hashed into the fingerprint */
  text: string;
  /** attached code files whose bytes on disk are not the ones recorded when
   *  they were attached (a teammate's new file, a git pull) */
  changed: string[];
}

export interface CodeOptions {
  /** fingerprint Script blocks too (they have a review of their own) */
  scripts?: boolean;
  /** the SHA-256 of each attached file as it is on disk now, by its path
   *  (GET /api/projects/{id}/attachments): the fingerprint takes these, not
   *  the hashes the project recorded, so a file changed on disk asks again.
   *  A file not listed (missing, or the list unavailable) keeps its recorded
   *  hash. */
  onDisk?: Record<string, string>;
}

/** The code in `project`, or null when it carries none. */
export function codeOf(
  project: Project,
  libraryById: Record<string, ComponentDef>,
  { scripts: withScripts = false, onDisk }: CodeOptions = {},
): ProjectCode | null {
  const def = libraryById[SCRIPT];
  const fallback = String(def?.parameters.find((p) => p.key === "code")?.default ?? "");
  // every Script code that can run, tagged with its element and case: the
  // block's own and any a case sets for it (a case's parameterOverrides are
  // layered on the block's when it runs)
  const scripts: [string, string, string][] = [];
  if (withScripts) {
    const ids = new Set<string>();
    for (const e of project.systems.flatMap((s) => s.elements)) {
      if (e.componentDefId !== SCRIPT) continue;
      ids.add(e.id);
      scripts.push([e.id, "", String(e.parameterOverrides.code ?? fallback)]);
    }
    for (const c of project.cases ?? []) {
      for (const [elId, overrides] of Object.entries(c.parameterOverrides ?? {})) {
        if (ids.has(elId) && overrides && "code" in overrides) scripts.push([elId, c.id, String(overrides.code)]);
      }
    }
    const key = (entry: string[]) => entry.join("\u0000");
    scripts.sort((a, b) => (key(a) < key(b) ? -1 : key(a) > key(b) ? 1 : 0));
  }
  const files = (project.attachments ?? [])
    .filter((a) => EXECUTABLE.test(a.path))
    .sort((a, b) => a.path.localeCompare(b.path))
    .map((a) => {
      const now = onDisk?.[a.path];
      return { path: a.path, sha256: now ?? a.sha256, changed: now !== undefined && now !== a.sha256 };
    });
  if (scripts.length === 0 && files.length === 0) return null;
  const blocks = new Set(scripts.map(([elId]) => elId)).size;
  const items = [
    ...(blocks ? [`${blocks} Script block${blocks > 1 ? "s" : ""} (Python code)`] : []),
    ...files.map((f) => f.path.replace(/^resources\//, "") + (f.changed ? " (changed since it was attached)" : "")),
  ];
  const text = JSON.stringify({ scripts, files: files.map((f) => [f.path, f.sha256]) });
  return { items, text, changed: files.filter((f) => f.changed).map((f) => f.path) };
}

/** SHA-256 of the code, hex (what the engine stores). */
export async function fingerprintOf(code: ProjectCode): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(code.text));
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
}

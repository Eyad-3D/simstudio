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
}

/** The code in `project`, or null when it carries none. */
export function codeOf(
  project: Project,
  libraryById: Record<string, ComponentDef>,
  { scripts: withScripts = false }: { scripts?: boolean } = {},
): ProjectCode | null {
  const def = libraryById[SCRIPT];
  const fallback = String(def?.parameters.find((p) => p.key === "code")?.default ?? "");
  const scripts = project.systems
    .flatMap((s) => s.elements)
    .filter((e) => e.componentDefId === SCRIPT)
    .map((e) => String(e.parameterOverrides.code ?? fallback))
    .filter(() => withScripts)
    .sort();
  const files = (project.attachments ?? [])
    .filter((a) => EXECUTABLE.test(a.path))
    .sort((a, b) => a.path.localeCompare(b.path));
  if (scripts.length === 0 && files.length === 0) return null;
  const items = [
    ...(scripts.length ? [`${scripts.length} Script block${scripts.length > 1 ? "s" : ""} (Python code)`] : []),
    ...files.map((f) => f.path.replace(/^resources\//, "")),
  ];
  const text = JSON.stringify({ scripts, files: files.map((f) => [f.path, f.sha256]) });
  return { items, text };
}

/** SHA-256 of the code, hex (what the engine stores). */
export async function fingerprintOf(code: ProjectCode): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(code.text));
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
}

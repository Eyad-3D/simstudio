// Fingerprints for run snapshots (RES-09): a run records a hash of the model
// it was made with, so two runs of the same model can be told from runs of a
// changed one.

import type { Project } from "./types";

/** JSON with every object's keys sorted, so equal values give equal text
 *  whatever order their fields were written in. */
export function canonicalJson(value: unknown): string {
  return JSON.stringify(value, (_key, v: unknown) =>
    v && typeof v === "object" && !Array.isArray(v)
      ? Object.fromEntries(
          Object.keys(v)
            .sort()
            .map((k) => [k, (v as Record<string, unknown>)[k]]),
        )
      : v,
  );
}

/** SHA-256 of the project's canonical JSON, as hex; undefined where the
 *  browser offers no Web Crypto (a page served from neither localhost nor
 *  https). */
export async function modelFingerprint(project: Project): Promise<string | undefined> {
  const subtle = globalThis.crypto?.subtle;
  if (!subtle) return undefined;
  const digest = await subtle.digest("SHA-256", new TextEncoder().encode(canonicalJson(project)));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

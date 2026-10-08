import { useEffect, useState } from "react";
import * as api from "../api";
import { desktop } from "../desktop";

const row = "flex items-start gap-2 border-t border-[color:var(--ss-border)] py-1";
const button = "ss-toolbtn shrink-0 border border-[color:var(--ss-border)] px-2 disabled:opacity-40";
const heading = "mt-3 mb-1 text-[12px] font-semibold";
const message = (e: unknown) => String((e as Error).message ?? e).replace(/^Error invoking remote method '[^']*': (Error: )?/, "");

/** Connect AI → AI access (AI-01): the settings `lightsim ai …` changes,
 *  kept in the same file and enforced by the engine for every AI tool: the
 *  switch, the folders AI tools may see, the examples, the projects whose
 *  Script blocks they may run, the run time cap, and the latest calls.
 *  Folders and trust can only be taken away here: a folder is added in the
 *  desktop app's folder dialog, a project trusted with `lightsim ai trust`.
 *  When the organisation's policy turns AI access off, it cannot be turned
 *  on here. */
export function AiAccessPanel() {
  const [state, setState] = useState<api.AiAccess | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [cap, setCap] = useState("");
  const [folder, setFolder] = useState("");
  const shell = desktop();

  const show = (s: api.AiAccess) => {
    setState(s);
    setCap(String(s.maxRunSeconds));
  };
  useEffect(() => {
    let live = true;
    api
      .aiAccess()
      .then((s) => live && show(s))
      .catch((e: unknown) => live && setError(message(e)));
    return () => {
      live = false;
    };
  }, []);

  const act = async (call: () => Promise<api.AiAccess | null>) => {
    setBusy(true);
    setError(null);
    try {
      const s = await call();
      if (s) show(s);
      return Boolean(s);
    } catch (e) {
      setError(message(e));
      api.aiAccess().then(show, () => {}); // as the settings are, after a change shown ahead
      return false;
    } finally {
      setBusy(false);
    }
  };
  const change = (c: api.AiAccessChange) => {
    // a ticked box shows its new state at once, as the engine is asked
    if (state && (c.enabled !== undefined || c.examples !== undefined))
      setState({ ...state, enabled: c.enabled ?? state.enabled, examples: c.examples ?? state.examples });
    return act(() => api.changeAiAccess(c));
  };
  const saveCap = () => {
    const seconds = Number(cap);
    if (!state || seconds === state.maxRunSeconds) return;
    if (!Number.isFinite(seconds) || seconds < 1 || seconds > 86400) {
      setError("The run time cap is a number of seconds from 1 to 86400.");
      setCap(String(state.maxRunSeconds));
      return;
    }
    void change({ maxRunSeconds: seconds });
  };

  if (!state)
    return (
      <div className="text-[color:var(--ss-text-dim)]">
        {error ? <p className="text-red-600">{error}</p> : "Reading the AI access settings…"}
      </div>
    );
  const blocked = Boolean(state.managed);

  return (
    <div data-testid="ai-access">
      <p className="text-[color:var(--ss-text-dim)]">
        These rules hold for every AI tool that uses LightSim: an AI app you connected, or the <code>lightsim</code>{" "}
        Python package. LightSim enforces them itself, and they are the same settings as{" "}
        <code>lightsim ai …</code> on the command line.
      </p>
      {state.managed && (
        <p className="mt-2 text-amber-600" data-testid="ai-access-managed">
          {state.managed} AI access cannot be turned on here.
        </p>
      )}
      <label className="mt-2 flex items-center gap-2 font-semibold">
        <input
          type="checkbox"
          checked={state.enabled && !blocked}
          disabled={busy || (blocked && !state.enabled)}
          onChange={(e) => void change({ enabled: e.target.checked })}
        />
        Let AI tools use LightSim
      </label>
      <p className="text-[color:var(--ss-text-dim)]" data-testid="ai-access-state">
        {state.on
          ? "AI access is on: AI tools may open the projects below and the examples you let them see."
          : "AI access is off: every call from an AI tool is refused."}
      </p>

      <div className={heading}>Folders AI tools may see</div>
      <ul aria-label="Allowed folders">
        {state.folders.map((f) => (
          <li key={f.path} className={row}>
            <span className="min-w-0 flex-1 break-all">
              {f.path}
              {f.projects && <span className="text-[color:var(--ss-text-dim)]"> (your projects folder)</span>}
              {!f.exists && <span className="text-amber-600"> (not found)</span>}
            </span>
            <button
              className={button}
              disabled={busy}
              aria-label={`Remove ${f.path}`}
              onClick={() => void change({ removeFolders: [f.path] })}
            >
              Remove
            </button>
          </li>
        ))}
        {state.folders.length === 0 && (
          <li className={`${row} text-[color:var(--ss-text-dim)]`}>None: AI tools see no project of yours.</li>
        )}
      </ul>
      {shell?.allowAiFolder ? (
        <button className={`${button} mt-1`} disabled={busy} onClick={() => void act(() => shell.allowAiFolder!())}>
          Add folder…
        </button>
      ) : (
        <form
          className="mt-1 flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void act(() => api.allowAiFolder(folder.trim())).then((ok) => ok && setFolder(""));
          }}
        >
          <input
            className="ss-input min-w-0 flex-1"
            aria-label="Folder to allow"
            placeholder="Folder path (the desktop app shows a folder dialog)"
            value={folder}
            onChange={(e) => setFolder(e.target.value)}
          />
          <button className={button} disabled={busy || !folder.trim()} type="submit">
            Add
          </button>
        </form>
      )}
      <p className="text-[color:var(--ss-text-dim)]">
        AI tools can list and open the project files in these folders, except a project hidden from them with{" "}
        <code>lightsim ai block &lt;file&gt;</code>.
      </p>

      <label className="mt-3 flex items-center gap-2">
        <input
          type="checkbox"
          checked={state.examples}
          disabled={busy}
          onChange={(e) => void change({ examples: e.target.checked })}
        />
        AI tools may open the examples that come with LightSim
      </label>

      <div className={heading}>Projects whose Script blocks AI tools may run</div>
      <ul aria-label="Trusted projects">
        {state.trusted.map((t) => (
          <li key={t.path} className={row}>
            <span className="min-w-0 flex-1 break-all">
              {t.name && <b>{t.name} </b>}
              <span className="text-[color:var(--ss-text-dim)]">{t.path}</span>
              {!t.exists ? (
                <span className="text-amber-600"> (not found)</span>
              ) : (
                !t.current && <span className="text-amber-600"> (its scripts changed since: not trusted now)</span>
              )}
            </span>
            <button
              className={button}
              disabled={busy}
              aria-label={`Untrust ${t.name ?? t.path}`}
              onClick={() => void change({ untrust: [t.path] })}
            >
              Untrust
            </button>
          </li>
        ))}
        {state.trusted.length === 0 && <li className={`${row} text-[color:var(--ss-text-dim)]`}>None.</li>}
      </ul>
      <p className="text-[color:var(--ss-text-dim)]">
        An AI tool runs a project with Script blocks (Python code) only once you trust it with{" "}
        <code>lightsim ai trust &lt;file&gt;</code>, and asks you before each run. Changing a script ends the trust.
      </p>

      <label className="mt-3 flex items-center gap-2">
        Stop an AI tool's run after
        <input
          className="ss-input w-[80px]"
          type="number"
          min={1}
          max={86400}
          aria-label="Run time cap in seconds"
          value={cap}
          disabled={busy}
          onChange={(e) => setCap(e.target.value)}
          onBlur={saveCap}
          onKeyDown={(e) => e.key === "Enter" && saveCap()}
        />
        s
      </label>

      <div className={heading}>Latest calls from AI tools</div>
      {state.audit.length ? (
        <table className="w-full" aria-label="Latest calls from AI tools">
          <tbody>
            {state.audit.map((a, i) => (
              <tr key={i} className="border-t border-[color:var(--ss-border)] align-top">
                <td className="whitespace-nowrap py-0.5 pr-2 text-[color:var(--ss-text-dim)]">
                  {new Date(a.time * 1000).toLocaleString()}
                </td>
                <td className="py-0.5 pr-2">
                  {a.tool}
                  {a.client && <span className="text-[color:var(--ss-text-dim)]"> by {a.client}</span>}
                  {a.project && <div className="break-all text-[color:var(--ss-text-dim)]">{a.project}</div>}
                </td>
                <td className={`py-0.5 ${a.outcome === "ok" ? "" : "text-amber-600"}`}>{a.outcome}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : (
        <p className="text-[color:var(--ss-text-dim)]" data-testid="ai-audit-empty">
          No AI tool has used LightSim yet.
        </p>
      )}
      <p className="mt-2 text-[11px] text-[color:var(--ss-text-dim)]">
        Kept on this computer only. Settings: <span className="break-all">{state.settingsPath}</span>
      </p>
      {error && <p className="text-red-600">{error}</p>}
    </div>
  );
}

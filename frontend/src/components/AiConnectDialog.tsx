import { useEffect, useState } from "react";
import * as api from "../api";
import { useUIStore } from "../store/uiStore";
import { AiAccessPanel } from "./AiAccessPanel";

/** Quote a command-line argument for a shell when it needs it. */
function shellArg(a: string): string {
  return /^[\w./:=@-]+$/.test(a) ? a : `"${a.replace(/"/g, '\\"')}"`;
}

/** Project → AI assistants → Connect (AI-29): add LightSim's MCP server to
 *  an AI app with one click, or copy the command that does it. Its AI access
 *  tab (AI-01) shows and changes what AI tools may do. */
export function AiConnectDialog() {
  const open = useUIStore((s) => s.aiConnectOpen);
  const setOpen = useUIStore((s) => s.setAiConnectOpen);
  const [state, setState] = useState<api.AiConnection | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [tab, setTab] = useState<"apps" | "access">("apps");
  const close = () => {
    setError(null);
    setOpen(false);
  };

  useEffect(() => {
    if (!open) return;
    let live = true;
    api
      .aiConnection()
      .then((s) => live && setState(s))
      .catch((e: unknown) => live && setError(String((e as Error).message ?? e)));
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("keydown", onKey);
    return () => {
      live = false;
      window.removeEventListener("keydown", onKey);
    };
  }, [open, setOpen]);

  if (!open) return null;

  const toggle = async (id: string, add: boolean) => {
    setBusy(id);
    setError(null);
    try {
      setState(await api.aiConnect(id, add));
    } catch (e) {
      setError(String((e as Error).message ?? e));
    } finally {
      setBusy(null);
    }
  };
  const command = state ? state.command.map(shellArg).join(" ") : "";
  const last = state?.lastUsed;

  return (
    <div
      className="fixed inset-0 z-[110] flex items-center justify-center bg-black/35"
      onMouseDown={(e) => e.target === e.currentTarget && close()}
    >
      <div
        role="dialog"
        aria-label="Connect an AI assistant"
        className="w-[560px] max-w-[94vw] overflow-hidden rounded-md border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] shadow-2xl"
      >
        <div className="border-b border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] px-4 pt-2.5 text-[13px] font-semibold">
          Connect an AI assistant
          <div role="tablist" aria-label="Sections" className="mt-1.5 flex gap-1 text-[12px] font-normal">
            {(
              [
                ["apps", "AI apps"],
                ["access", "AI access"],
              ] as const
            ).map(([id, label]) => (
              <button
                key={id}
                role="tab"
                id={`ai-tab-${id}`}
                aria-selected={tab === id}
                aria-controls="ai-tab-panel"
                className={`-mb-px border-b-2 px-2 pb-1 ${
                  tab === id
                    ? "border-[color:var(--ss-accent)] text-[color:var(--ss-text)]"
                    : "border-transparent text-[color:var(--ss-text-dim)]"
                }`}
                onClick={() => setTab(id)}
              >
                {label}
              </button>
            ))}
          </div>
        </div>
        <div
          id="ai-tab-panel"
          role="tabpanel"
          aria-labelledby={`ai-tab-${tab}`}
          className="max-h-[70vh] space-y-3 overflow-auto px-4 py-3 text-[12px] leading-snug"
        >
          {tab === "access" ? (
            <AiAccessPanel />
          ) : (
            <>
              <p className="text-[color:var(--ss-text-dim)]">
                An AI assistant on this computer can open your LightSim projects, run their checks and simulations, and
                read the results. It starts LightSim itself and talks to it directly, with no network connection. It asks
                you before it saves a change or runs a project that contains Script blocks.
              </p>
              {state?.warning && <p className="text-amber-600">{state.warning}</p>}
              {state?.managed && (
                <p className="text-amber-600" data-testid="ai-managed">
                  {state.managed}
                </p>
              )}
              <table className="w-full">
                <tbody>
                  {(state?.clients ?? []).map((c) => (
                    <tr key={c.id} className="border-t border-[color:var(--ss-border)]">
                      <td className="py-1.5 pr-2">{c.title}</td>
                      <td className="py-1.5 pr-2 text-[color:var(--ss-text-dim)]">{c.installed ? "Connected" : ""}</td>
                      <td className="py-1.5 text-right">
                        <button
                          className="ss-toolbtn border border-[color:var(--ss-border)] px-3"
                          disabled={busy !== null || (!c.installed && !!state?.managed)}
                          title={c.configPath}
                          onClick={() => void toggle(c.id, !c.installed)}
                        >
                          {c.installed ? "Remove" : "Add"}
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <p className="text-[color:var(--ss-text-dim)]">
                After adding, restart the AI app. Or, in a terminal:{" "}
                <code className="select-all break-all">{command} install --client claude</code> (also{" "}
                <code>vscode</code>, <code>codex</code>, <code>gemini</code>, <code>copilot</code>, <code>cursor</code>,{" "}
                <code>claude-code</code>).
              </p>
              <p data-testid="ai-last-used">
                {last
                  ? `Last used${last.client ? ` by ${last.client}` : ""}: ${new Date(last.t * 1000).toLocaleString()}${
                      last.tool ? ` (${last.tool})` : ""
                    }`
                  : "No assistant has used LightSim yet."}
              </p>
              {error && <p className="text-red-600">{error}</p>}
            </>
          )}
        </div>
        <div className="flex justify-end border-t border-[color:var(--ss-border)] px-4 py-2.5">
          <button className="ss-toolbtn border border-[color:var(--ss-border)] px-3" onClick={close}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

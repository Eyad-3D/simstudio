// The first-steps bar under the ribbon (UX-26): five steps that tick
// themselves off (tour.ts), each a click from the panel or command that does
// it, and a one-time hint the first time a panel that needs one opens.
import { useEffect } from "react";
import { Check, X } from "lucide-react";
import { openHelp } from "../help";
import { STEPS, automated, autoTour, dismissHint, doStep, hideStepBar, useTourStore, watchSteps } from "../tour";

const NONE: Partial<Record<string, true>> = {};

export function FirstSteps() {
  const hidden = useTourStore((s) => s.hidden);
  const done = useTourStore((s) => s.done) ?? NONE;
  const hint = useTourStore((s) => s.hint);
  const auto = useTourStore((s) => s.auto);
  const off = hidden || (automated() && !auto);
  useEffect(() => {
    if (off) return;
    const unWatch = watchSteps();
    const unTour = autoTour();
    return () => {
      unWatch();
      unTour();
    };
  }, [off]);
  if (off) return null;
  const all = STEPS.every((s) => done[s.id]);
  return (
    <div className="ss-zoom border-b border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] text-[11px]">
      <nav className="flex flex-wrap items-center gap-1 px-2 py-0.5" aria-label="First steps" data-tour="steps">
        <span className="mr-1 font-semibold text-[color:var(--ss-text-dim)]">First steps</span>
        {STEPS.map((s, i) => (
          <button
            key={s.id}
            className={`flex items-center gap-1 rounded px-1.5 py-0.5 hover:bg-[color:var(--ss-hover)] ${
              done[s.id] ? "text-[color:var(--ss-ok)]" : "text-[color:var(--ss-text)]"
            }`}
            title={s.hint}
            aria-label={`Step ${i + 1}, ${s.label}: ${done[s.id] ? "done" : s.hint}`}
            onClick={() => doStep(s.id)}
          >
            <span
              className={`inline-flex h-4 w-4 items-center justify-center rounded-full border text-[10px] ${
                done[s.id]
                  ? "border-[color:var(--ss-ok)] bg-[color:var(--ss-ok)] text-white"
                  : "border-[color:var(--ss-field-border)]"
              }`}
              aria-hidden="true"
            >
              {done[s.id] ? <Check size={10} /> : i + 1}
            </span>
            {s.label}
          </button>
        ))}
        {all && (
          <span className="ml-1">
            All done. Next:{" "}
            <button className="text-[color:var(--ss-accent)] underline" onClick={() => openHelp("tutorials/first-electric-car.html")}>
              your first electric car, in 15 minutes
            </button>
          </span>
        )}
        <button className="ss-toolbtn ml-auto" title="Hide the first steps (Help menu shows them again)" aria-label="Hide the first steps" onClick={hideStepBar}>
          <X size={12} />
        </button>
      </nav>
      {hint && (
        <div className="flex items-start gap-2 border-t border-[color:var(--ss-border)] px-2 py-1" role="status">
          <span className="min-w-0 flex-1">{hint}</span>
          <button className="ss-toolbtn shrink-0" onClick={dismissHint}>
            Got it
          </button>
        </div>
      )}
    </div>
  );
}

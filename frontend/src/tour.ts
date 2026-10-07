// First steps for new users (UX-26): a short, skippable tour of the screen
// and a slim step bar that ticks itself off as the user builds, sets values,
// picks a test, runs and reads the results. The tour uses driver.js (MIT;
// shepherd.js and intro.js are AGPL and refused by the licence gate). The
// lesson behind it is the tutorial "Your first electric car" (LRN-07).
//
// Progress is kept per user in localStorage (TOUR_KEY). Automated browsers
// (navigator.webdriver: the e2e and screenshot tests) get neither the tour
// nor the bar unless the stored state asks for them ({ auto: true }).
import { driver } from "driver.js";
import "driver.js/dist/driver.css";
import { create } from "zustand";
import { problemCounts, useProjectStore } from "./store/projectStore";
import { useUIStore } from "./store/uiStore";
import { TOUR_KEY } from "./storageKeys";
import type { Project } from "./types";

export const STEPS = [
  { id: "build", label: "Build", hint: "Problems lists no errors", panel: "data-checks" },
  { id: "values", label: "Set values", hint: "Change a value in Properties", panel: "properties" },
  { id: "tests", label: "Choose tests", hint: "A case drives a cycle, a test or a lap", panel: "cases" },
  { id: "run", label: "Run", hint: "Press Run: the run ends as success", panel: null },
  { id: "results", label: "Read results", hint: "Open the Results tab", panel: null },
] as const;
export type StepId = (typeof STEPS)[number]["id"];

/** The panels and pages that explain themselves once, the first time. */
export const HINTS: Record<string, string> = {
  "data-bus":
    "Data Bus Connections: each row is a signal input. Click its box and pick the output that feeds it, such as the Driver's Target Speed from a Driving Task.",
  cases:
    "Cases: each case is one simulation job (a cycle, a test or a lap). Values set here apply to that case only; a sweep runs it over a range of one value.",
  results:
    "Results: the headline numbers sum up the run; rest the pointer on a row of All summary values to see what it means. Tick channels on the left to plot them.",
};

export interface TourState {
  /** the tour was finished or skipped */
  toured?: boolean;
  /** the step bar was hidden */
  hidden?: boolean;
  /** steps ticked, kept once done */
  done?: Partial<Record<StepId, true>>;
  /** hints already shown */
  hints?: Record<string, true>;
  /** show the tour and bar even in an automated browser (tests) */
  auto?: boolean;
}

function load(): TourState {
  try {
    const saved: unknown = JSON.parse(window.localStorage.getItem(TOUR_KEY) ?? "{}");
    if (saved && typeof saved === "object" && !Array.isArray(saved)) return saved as TourState;
  } catch {
    /* storage unavailable or unreadable */
  }
  return {};
}

export const useTourStore = create<TourState & { hint: string | null }>(() => ({ ...load(), hint: null }));

function save(change: Partial<TourState>) {
  useTourStore.setState(change);
  const { hint: _hint, ...state } = useTourStore.getState();
  try {
    window.localStorage.setItem(TOUR_KEY, JSON.stringify(state));
  } catch {
    /* storage unavailable: progress lasts for this session */
  }
}

/** Whether this browser gets the first-steps help by itself. */
export const automated = () => typeof navigator !== "undefined" && navigator.webdriver === true && !useTourStore.getState().auto;

export function hideStepBar() {
  save({ hidden: true });
}

export function tick(id: StepId) {
  const { done = {} } = useTourStore.getState();
  if (!done[id]) save({ done: { ...done, [id]: true } });
}

// a panel the layout opens by itself is not one the user opened
let acted = false;
for (const type of ["pointerdown", "keydown"]) {
  window.addEventListener(type, () => (acted = true), { capture: true, once: true });
}

/** Show a panel's hint the first time the user opens it. */
export function hintFor(key: string) {
  const { hints = {}, hidden } = useTourStore.getState();
  if (!acted || hidden || automated() || hints[key] || !HINTS[key]) return;
  save({ hints: { ...hints, [key]: true } });
  useTourStore.setState({ hint: HINTS[key] });
}

export const dismissHint = () => useTourStore.setState({ hint: null });

const overrides = (p: Project | null) =>
  JSON.stringify(p?.systems.flatMap((s) => s.elements.map((e) => [e.id, e.parameterOverrides])) ?? []);

/** Does the active case drive something to judge: a cycle or profile, a test or a lap? */
function hasTest(): boolean {
  const { project, activeCaseId } = useProjectStore.getState();
  const c = project?.cases.find((x) => x.id === activeCaseId);
  if (!project || !c) return false;
  if (c.kind && c.kind !== "cycle") return true;
  return project.systems.some((s) => s.elements.some((e) => e.componentDefId === "signal.driving_task"));
}

/** Follow the stores and tick the steps as the user does them. */
export function watchSteps(): () => void {
  const check = () => {
    const s = useProjectStore.getState();
    const elements = s.project?.systems.reduce((n, x) => n + x.elements.length, 0) ?? 0;
    if (elements > 0 && s.dataChecks !== null && problemCounts({ dataChecks: s.dataChecks, runs: [] }).errors === 0) tick("build");
    if (hasTest()) tick("tests");
    const last = s.runs.find((r) => r.status !== "running");
    if (last?.status === "success") tick("run");
    if (last?.status === "success" && useUIStore.getState().ribbonTab === "results") tick("results");
  };
  const unProject = useProjectStore.subscribe((s, prev) => {
    // a value changed on the same project (not a project opened)
    if (s.project && prev.project && s.project.id === prev.project.id && s.project !== prev.project
      && overrides(s.project) !== overrides(prev.project)) tick("values");
    check();
  });
  const unUI = useUIStore.subscribe((s, prev) => {
    if (s.ribbonTab !== prev.ribbonTab && s.ribbonTab === "results") hintFor("results");
    check();
  });
  check();
  return () => {
    unProject();
    unUI();
  };
}

/** Open the panel, page or command that does a step. */
export function doStep(id: StepId) {
  const ui = useUIStore.getState();
  const step = STEPS.find((s) => s.id === id)!;
  if (id === "run") {
    void useProjectStore.getState().run();
    return;
  }
  if (id === "results") {
    ui.setRibbonTab("results");
    return;
  }
  ui.setRibbonTab("home");
  if (step.panel) ui.focusPanel(step.panel);
}

/** The tour: five stops, each a popover on a part of the screen. */
export function startTour() {
  save({ hidden: false, toured: true });
  const ui = useUIStore.getState();
  if (ui.ribbonTab !== "home") ui.setRibbonTab("home");
  const store = useProjectStore.getState();
  const motor = store.project?.systems[0]?.elements.find((e) => e.componentDefId === "motor.emotor");
  const node = (label: string) =>
    [...document.querySelectorAll(".react-flow__node")].find((n) => n.textContent?.includes(label)) as Element;
  const tour = driver({
    showProgress: true,
    progressText: "{{current}} of {{total}}",
    nextBtnText: "Next",
    prevBtnText: "Back",
    doneBtnText: "Done",
    popoverClass: "ss-tour",
    steps: [
      {
        element: "[aria-label='Model workspace panels']",
        popover: {
          title: "Your model",
          description:
            "The diagram in the middle shows the car's parts and the wires between them. The library on the left adds parts; Properties on the right shows the selected part's values.",
        },
      },
      ...(motor
        ? [{
            element: () => node(motor.label),
            onHighlightStarted: () => useProjectStore.getState().select(motor.id),
            popover: {
              title: "A part and its values",
              description: `This is the <b>${motor.label}</b>. Its values are on the right, in Properties. Rest the pointer on one to see what it means; F1 opens the part's help page.`,
            },
          }]
        : []),
      {
        element: "[data-tour='run']",
        popover: {
          title: "Run",
          description: "Pick a case in the list (a drive cycle, a test or a lap) and press Run, or Ctrl+Enter. A run takes seconds.",
          side: "bottom" as const,
        },
      },
      {
        element: "[data-tour='tab-results']",
        popover: {
          title: "Read the results",
          description: "The Results tab shows the run: consumption, distance and charge on top, a chart of any channel under them.",
          side: "bottom" as const,
        },
      },
      {
        element: "[data-tour='steps']",
        popover: {
          title: "Your first steps",
          description:
            "This bar ticks itself off as you build, set values, pick a test, run and read the results. Change a value such as the Vehicle Mass and run again: Results compares the two runs. Help → <i>Your first electric car</i> is the 15-minute lesson.",
          side: "bottom" as const,
        },
      },
    ],
  });
  tour.drive();
}

/** Start the tour by itself once, the first time a model is open on Home. */
export function autoTour(): () => void {
  const maybe = () => {
    const t = useTourStore.getState();
    if (t.toured || t.hidden || automated()) return false;
    if (useUIStore.getState().ribbonTab !== "home" || !useProjectStore.getState().project) return false;
    // a frame later, once the dock has laid the diagram out
    requestAnimationFrame(() => startTour());
    return true;
  };
  if (maybe()) return () => {};
  const unsub = useUIStore.subscribe(() => maybe() && unsub());
  return unsub;
}

declare global {
  interface Window {
    /** For the desktop app's Help menu: show the tour. */
    lightsimTour?: () => void;
  }
}
window.lightsimTour = startTour;

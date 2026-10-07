import { create } from "zustand";

/** The run reports' view choices (RES-22, RES-38): kept for the session. */
interface ReportsState {
  /** the diagram's Energy labels and bar chart */
  showEnergy: boolean;
  setShowEnergy: (on: boolean) => void;
  /** the bar chart hides parts below this share of the sources' energy, % */
  energyMinPct: number;
  setEnergyMinPct: (pct: number) => void;
  /** the band under the Results chart that says what held the car back */
  showLimits: boolean;
  setShowLimits: (on: boolean) => void;
}

export const useReportsStore = create<ReportsState>((set) => ({
  showEnergy: false,
  setShowEnergy: (on) => set({ showEnergy: on }),
  energyMinPct: 1,
  setEnergyMinPct: (pct) => set({ energyMinPct: pct }),
  showLimits: true,
  setShowLimits: (on) => set({ showLimits: on }),
}));

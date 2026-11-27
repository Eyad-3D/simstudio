import { create } from "zustand";
import { ENERGY_BOX_KEY } from "../storageKeys";

/** The run reports' view choices (RES-22, RES-38): kept for the session,
 *  but for whether the Energy view's "Energy lost per part" box is open,
 *  which is kept for the user (the owner's choice G: it starts folded). */
interface ReportsState {
  /** the diagram's Energy labels and bar chart */
  showEnergy: boolean;
  setShowEnergy: (on: boolean) => void;
  /** with the Energy view on, the "Energy lost per part" box is open; folded
   *  it is a small button in the diagram's top-right corner */
  energyBoxOpen: boolean;
  setEnergyBoxOpen: (open: boolean) => void;
  /** the bar chart hides parts below this share of the sources' energy, % */
  energyMinPct: number;
  setEnergyMinPct: (pct: number) => void;
  /** the band under the Results chart that says what held the car back */
  showLimits: boolean;
  setShowLimits: (on: boolean) => void;
}

function storedBoxOpen(): boolean {
  try {
    return window.localStorage.getItem(ENERGY_BOX_KEY) === "1";
  } catch {
    return false;
  }
}

export const useReportsStore = create<ReportsState>((set) => ({
  showEnergy: false,
  setShowEnergy: (on) => set({ showEnergy: on }),
  energyBoxOpen: storedBoxOpen(),
  setEnergyBoxOpen: (open) => {
    try {
      window.localStorage.setItem(ENERGY_BOX_KEY, open ? "1" : "0");
    } catch {
      /* storage unavailable: this session only */
    }
    set({ energyBoxOpen: open });
  },
  energyMinPct: 1,
  setEnergyMinPct: (pct) => set({ energyMinPct: pct }),
  showLimits: true,
  setShowLimits: (on) => set({ showLimits: on }),
}));

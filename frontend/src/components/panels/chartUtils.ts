// Shared chart helpers used by the Results panel and the dockable mini-chart.
import { useCallback, useRef, useState } from "react";
import type { Channel } from "../../types";

export const PALETTE = [
  "#2f6fb3",
  "#d97706",
  "#059669",
  "#dc2626",
  "#7c3aed",
  "#0e7490",
  "#be185d",
  "#4d7c0f",
  "#b45309",
  "#1d4ed8",
];

export const MAX_PLOT_POINTS = 2000;

export function channelKey(c: Channel): string {
  return `${c.elementId}:${c.portId}`;
}

/** Stride-decimate a series so charts/tables stay responsive (keeps the endpoints). */
export function decimate<T>(arr: T[], max = MAX_PLOT_POINTS): T[] {
  if (arr.length <= max) return arr;
  const stride = Math.ceil(arr.length / max);
  const out: T[] = [];
  for (let i = 0; i < arr.length; i += stride) out.push(arr[i]);
  if (out[out.length - 1] !== arr[arr.length - 1]) out.push(arr[arr.length - 1]);
  return out;
}

/** True once the element has a real size — dockview keeps hidden tabs at 0×0,
 *  and recharts warns loudly if asked to render there. `ref` is a callback ref,
 *  so the observer attaches whenever the chart area mounts: the panels render
 *  it only after their "no runs yet" placeholder, or swap it between views.
 *  `element` holds the currently attached node (for PNG export). */
export function useHasSize<T extends HTMLElement>() {
  const element = useRef<T | null>(null);
  const [hasSize, setHasSize] = useState(false);
  const ref = useCallback((node: T | null) => {
    if (!node) return;
    element.current = node;
    const obs = new ResizeObserver((entries) => {
      const r = entries[0]?.contentRect;
      setHasSize(!!r && r.width > 0 && r.height > 0);
    });
    obs.observe(node);
    return () => {
      obs.disconnect();
      element.current = null;
      setHasSize(false);
    };
  }, []);
  return { ref, hasSize, element };
}

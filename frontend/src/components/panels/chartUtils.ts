// Shared chart helpers used by the Results panel and the dockable mini-chart.
import { useCallback, useState } from "react";
import type { Channel } from "../../types";

/** Series colours for chart lines and swatches (at least 3:1 on the panel).
 *  Several are too faint for text, so names are drawn in the text colour. */
export const PALETTE = [
  "#2f6fb3",
  "#d97706",
  "#059669",
  "#dc2626",
  "#8b5cf6",
  "#0e7490",
  "#d0457f",
  "#4d7c0f",
  "#b45309",
  "#4d7ce8",
];

export function channelKey(c: Channel): string {
  return `${c.elementId}:${c.portId}`;
}

/** True once the element has a real size — dockview keeps hidden tabs at 0×0,
 *  where a chart cannot be drawn. `ref` is a callback ref, so the observer
 *  attaches whenever the chart area mounts: the panels render it only after
 *  their "no runs yet" placeholder, or swap it between views. */
export function useHasSize<T extends HTMLElement>() {
  const [hasSize, setHasSize] = useState(false);
  const ref = useCallback((node: T | null) => {
    if (!node) return;
    const obs = new ResizeObserver((entries) => {
      const r = entries[0]?.contentRect;
      setHasSize(!!r && r.width > 0 && r.height > 0);
    });
    obs.observe(node);
    return () => {
      obs.disconnect();
      setHasSize(false);
    };
  }, []);
  return { ref, hasSize };
}

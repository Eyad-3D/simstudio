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

/** Most points drawn per chart (about one min/max pair per pixel column). */
export const MAX_PLOT_POINTS = 2000;

type Sample = { value: number | null };

export function channelKey(c: Channel): string {
  return `${c.elementId}:${c.portId}`;
}

/**
 * Sample indices to draw for series that share one time grid (sample i is at
 * the same t in each). The samples are split into equal buckets and each
 * bucket keeps the samples where any series is at its lowest or highest
 * (MinMax thinning), plus the first and last sample, so no peak or dip is
 * ever dropped the way keeping every Nth sample drops them. Using the same
 * indices for every series keeps rows aligned and X-Y pairs real samples.
 * Returns at most `max` indices (unless one bucket alone needs more), in order.
 */
export function minMaxIndices(series: Sample[][], max = MAX_PLOT_POINTS): number[] {
  if (series.length === 0) return [];
  const n = Math.min(...series.map((s) => s.length));
  if (n <= max) return Array.from({ length: n }, (_, i) => i);
  const pick = (buckets: number) => {
    const keep = new Set<number>([0, n - 1]);
    for (let b = 0; b < buckets; b++) {
      const start = Math.floor((b * n) / buckets);
      const end = Math.floor(((b + 1) * n) / buckets);
      for (const s of series) {
        let lo = -1;
        let hi = -1;
        for (let i = start; i < end; i++) {
          const v = s[i].value;
          if (v === null) continue;
          if (lo < 0 || v < s[lo].value!) lo = i;
          if (hi < 0 || v > s[hi].value!) hi = i;
        }
        if (lo >= 0) {
          keep.add(lo);
          keep.add(hi);
        }
      }
    }
    return keep;
  };
  // several series often peak at the same samples, so start with max/2
  // buckets and only use fewer (wider) buckets if the union is too large
  let buckets = Math.max(1, Math.floor(max / 2));
  for (;;) {
    const keep = pick(buckets);
    if (keep.size <= max || buckets === 1) return [...keep].sort((a, b) => a - b);
    buckets = Math.max(1, Math.min(buckets - 1, Math.floor((buckets * max) / keep.size)));
  }
}

/** Thin one series for drawing without losing its peaks (see minMaxIndices). */
export function decimate<T extends Sample>(arr: T[], max = MAX_PLOT_POINTS): T[] {
  if (arr.length <= max) return arr;
  return minMaxIndices([arr], max).map((i) => arr[i]);
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

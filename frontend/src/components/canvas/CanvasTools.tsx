// The diagram's toolbar, floating at its top right: the view only (zoom in,
// out, the zoom list, Fit; grid, snap, Energy, the overview map). Undo, Redo
// and Delete are the ribbon's (and Ctrl+Z, Ctrl+Y, Del, and the right-click
// menu's Delete): not shown twice (the owner's choice D, 27 Nov).
import type { ReactNode } from "react";
import { Grid2x2, Magnet, Map, Maximize, Zap, ZoomIn, ZoomOut } from "lucide-react";

const on = (active: boolean) => `ss-toolbtn ${active ? "bg-[color:var(--ss-active)]" : ""}`;

export function CanvasTools(p: {
  onZoomIn: () => void;
  onZoomOut: () => void;
  /** the zoom list (it reads the view itself) */
  zoomMenu: ReactNode;
  onFit: () => void;
  grid: boolean;
  onGrid: () => void;
  snap: boolean;
  onSnap: () => void;
  energy: boolean;
  onEnergy: () => void;
  miniMap: boolean;
  onMiniMap: () => void;
}) {
  return (
    <div className="pointer-events-auto flex flex-wrap items-center justify-end gap-0.5 rounded border border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] px-1 py-0.5 shadow-sm">
      <button className="ss-toolbtn" title="Zoom in" onClick={p.onZoomIn}>
        <ZoomIn size={14} />
      </button>
      <button className="ss-toolbtn" title="Zoom out" onClick={p.onZoomOut}>
        <ZoomOut size={14} />
      </button>
      {p.zoomMenu}
      <button className="ss-toolbtn" title="Fit to screen (press . to frame the selection)" onClick={p.onFit}>
        <Maximize size={14} />
      </button>
      <div className="mx-1 h-4 w-px bg-[color:var(--ss-border)]" />
      <button className={on(p.grid)} title="Toggle background grid" onClick={p.onGrid}>
        <Grid2x2 size={14} />
      </button>
      <button className={on(p.snap)} title="Snap elements to the grid while dragging" onClick={p.onSnap}>
        <Magnet size={14} />
      </button>
      <button
        className={on(p.energy)}
        title="Energy: label each part with its energy in, out and lost in the run shown in Results, and chart the losses"
        aria-pressed={p.energy}
        onClick={p.onEnergy}
      >
        <Zap size={14} />
      </button>
      <button className={on(p.miniMap)} title="Toggle minimap" onClick={p.onMiniMap}>
        <Map size={14} />
      </button>
    </div>
  );
}

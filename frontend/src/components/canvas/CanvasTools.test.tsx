import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { CanvasTools } from "./CanvasTools";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("the diagram's toolbar has the view only: no Undo, Redo or Delete (they are the ribbon's, choice D)", () => {
  const host = document.body.appendChild(document.createElement("div"));
  const f = vi.fn();
  act(() =>
    createRoot(host).render(
      <CanvasTools
        onZoomIn={f}
        onZoomOut={f}
        zoomMenu={<select aria-label="Zoom" />}
        onFit={f}
        grid
        onGrid={f}
        snap={false}
        onSnap={f}
        energy={false}
        onEnergy={f}
        miniMap={false}
        onMiniMap={f}
      />,
    ),
  );
  expect([...host.querySelectorAll("button")].map((b) => b.title)).toEqual([
    "Zoom in",
    "Zoom out",
    "Fit to screen (press . to frame the selection)",
    "Toggle background grid",
    "Snap elements to the grid while dragging",
    "Energy: label each part with its energy in, out and lost in the run shown in Results, and chart the losses",
    "Toggle minimap",
  ]);
  expect(host.querySelector("select[aria-label=Zoom]")).not.toBeNull();
  // one separator, between Fit and the view's switches
  expect(host.querySelectorAll("div.w-px")).toHaveLength(1);
  host.remove();
});

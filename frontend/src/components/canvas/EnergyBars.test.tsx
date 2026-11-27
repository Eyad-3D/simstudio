import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it } from "vitest";
import { useReportsStore } from "../../store/reportsStore";
import { EnergyBars } from "./EnergyOverlay";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  document.body.innerHTML = "";
  localStorage.clear();
});

const toggle = (host: HTMLElement) =>
  [...host.querySelectorAll("button")].find((b) => b.textContent?.includes("Energy lost per part"))!;
const box = (host: HTMLElement) => host.querySelector("[role=region][aria-label='Energy lost per part']");

it("the Energy box starts folded to a button; a click opens it, the button or its close folds it; the choice is kept (choice G)", () => {
  useReportsStore.setState({ showEnergy: true, energyBoxOpen: false });
  const host = document.body.appendChild(document.createElement("div"));
  act(() => createRoot(host).render(<EnergyBars top={52} />));

  // folded: only the button, in the top-right corner under the toolbar, saying what it opens
  expect(box(host)).toBeNull();
  const b = toggle(host);
  expect(b.getAttribute("aria-expanded")).toBe("false");
  expect(b.title).toBe("Open the chart of the energy each part lost");
  expect((b.parentElement as HTMLElement).style.top).toBe("52px");

  act(() => b.click());
  expect(box(host)).not.toBeNull();
  expect((box(host) as HTMLElement).style.top).toBe("52px");
  expect(toggle(host).getAttribute("aria-expanded")).toBe("true");
  expect(localStorage.getItem("lightsim-energy-box")).toBe("1");

  // the button again folds it
  act(() => toggle(host).click());
  expect(box(host)).toBeNull();
  expect(localStorage.getItem("lightsim-energy-box")).toBe("0");

  // and so does its close; the Energy labels stay on
  act(() => toggle(host).click());
  act(() => (host.querySelector("button[aria-label='Fold this chart']") as HTMLButtonElement).click());
  expect(box(host)).toBeNull();
  expect(useReportsStore.getState().showEnergy).toBe(true);

  // nothing at all with the Energy view off
  act(() => useReportsStore.setState({ showEnergy: false }));
  expect(toggle(host)).toBeUndefined();
});

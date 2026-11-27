// The Start page's example cards show two lines of their description; More
// opens the rest in place and Less folds it, one card at a time (calm C).
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, expect, it, vi } from "vitest";

const LONG = "Compact electric car modelled on the 2021 Cupra Born. What to expect: 13-17 kWh/100 km on WLTC.";
vi.mock("../api", async (real) => ({
  ...(await real<typeof import("../api")>()),
  listProjects: async () => [],
  listFiles: async () => [],
  listExamples: async () => [
    { id: "bev-car", name: "Battery Electric Car", description: LONG, elements: 24, hidden: false },
    { id: "fs", name: "FS Electric", description: "Formula Student car. What to expect: 3.5-4.5 s.", elements: 19, hidden: false },
  ],
}));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
// no layout in jsdom: a folded text is taller than its two lines
Object.defineProperty(HTMLElement.prototype, "scrollHeight", { configurable: true, get() { return this.classList.contains("line-clamp-2") ? 60 : 30; } });
Object.defineProperty(HTMLElement.prototype, "clientHeight", { configurable: true, get() { return 30; } });

let StartPage: typeof import("./StartPage").StartPage;
beforeAll(async () => ({ StartPage } = await import("./StartPage")));
let root: Root | null = null;
afterEach(() => {
  act(() => root?.unmount());
  document.body.innerHTML = "";
});
async function page() {
  const host = document.body.appendChild(document.createElement("div"));
  root = createRoot(host);
  await act(async () => root!.render(<StartPage />));
  return host;
}
const more = (host: HTMLElement, title: string) => host.querySelector<HTMLButtonElement>(`button[aria-label$="about ${title}"]`)!;
const text = (host: HTMLElement, id: string) => host.querySelector(`#start-example-${id}-text`)!;

it("a card shows its name, part count, opens as a copy and two lines of its description; More opens the rest, Less folds it", async () => {
  const host = await page();
  const card = host.querySelector<HTMLButtonElement>('button[aria-label="Battery Electric Car, 24 parts · opens as a copy"]')!;
  expect(card).not.toBeNull();
  // folded, not cut: the whole text is there, shown in two lines
  expect(text(host, "bev-car").textContent).toBe(LONG);
  expect(text(host, "bev-car").className).toContain("line-clamp-2");
  const m = more(host, "Battery Electric Car");
  expect(m.textContent).toBe("More");
  expect(m.getAttribute("aria-expanded")).toBe("false");
  // (a button of its own, not inside the card's: the card still opens the example)
  expect(card.contains(m)).toBe(false);
  await act(async () => m.click());
  expect(text(host, "bev-car").className).not.toContain("line-clamp-2");
  expect(more(host, "Battery Electric Car").textContent).toBe("Less");
  expect(more(host, "Battery Electric Car").getAttribute("aria-expanded")).toBe("true");
  await act(async () => more(host, "Battery Electric Car").click());
  expect(text(host, "bev-car").className).toContain("line-clamp-2");
});

it("one card open at a time: More on another folds the first", async () => {
  const host = await page();
  await act(async () => more(host, "Battery Electric Car").click());
  await act(async () => more(host, "FS Electric").click());
  expect(text(host, "fs").className).not.toContain("line-clamp-2");
  expect(text(host, "bev-car").className).toContain("line-clamp-2");
});

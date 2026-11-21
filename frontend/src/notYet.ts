// Things the app has a control for that this version's engine does not run
// yet (docs/KNOWN-LIMITS.md, "Does not run in this version"). Such a control
// stays visible, so the feature can be found, but cannot be pressed, and says
// so on hover and on keyboard focus: `<button {...notYet("FS events")}>`.
// It is aria-disabled, not disabled, so it can still take focus and be read.

import type React from "react";

/** The words a not-yet control shows on hover and focus. */
export function notYetText(what: string): string {
  return `${what}: not in this version of LightSim yet`;
}

/** Props for a control whose feature does not run in this version: shown
 *  dimmed, the reason on hover and focus (CSS `[data-not-yet]`), a press
 *  does nothing. Spread them last, over the control's own onClick. */
export function notYet(what: string) {
  const text = notYetText(what);
  return {
    "aria-disabled": true as const,
    "aria-description": text,
    "data-not-yet": text,
    onClick: (e: React.MouseEvent) => e.preventDefault(),
  };
}

/** Choices of a list parameter that do not run in this version, by
 *  parameter key: shown with the words, and not offered (a project that
 *  has one keeps it, and its run says why it is refused). None now: a
 *  Battery's Defined By: Cells runs since the cells were ported. */
export const NOT_YET_OPTIONS: Record<string, string[]> = {};

/** An option's label: its own, with "not in this version yet" when it is one. */
export function optionLabel(key: string, option: string): { label: string; notYet: boolean } {
  const notYet = NOT_YET_OPTIONS[key]?.includes(option) ?? false;
  return { label: notYet ? `${option} (not in this version yet)` : option, notYet };
}

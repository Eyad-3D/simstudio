"use strict";

/**
 * The words of the About box (Help → About LightSim), kept apart from the
 * window code so a test can read them. The claim about validation is checked
 * against docs/VALIDATION-STATUS.md and the reference suite by the backend's
 * docs tests (backend/tests/test_docs.py).
 */

/** The help page the box's second button opens: Help → What Is Validated. */
const VALIDATION_PAGE = "validation.html";

/** The box's buttons; the second opens VALIDATION_PAGE. */
const BUTTONS = ["OK", "What Is Validated"];

/**
 * The box's text under the version. `projects` is the projects folder.
 * Only the claim about validation in the second paragraph is new; the rest
 * of that paragraph and the copyright and licence lines are the owner's
 * wording and stay as they are.
 */
function detail(projects) {
  return (
    "A desktop app for simulating vehicle energy use, range and powertrains.\n\n" +
    "This is an early version: the physics are simplified, only the energy use " +
    "of four electric cars on EPA's two test cycles has been compared with " +
    "official results (within 8 %, with no tuning; nothing else is validated), " +
    "and some results are known to be wrong. Read Known Limits before " +
    "relying on a number.\n\n" +
    `Projects folder:\n${projects}\n\n` +
    "Copyright © 2026 Eyad Abualkhair. All rights reserved.\n" +
    "Free for non-commercial use; commercial use needs a licence (see EULA.txt).\n" +
    "Includes open-source software and data under their own licences (Help → Third-Party Notices)."
  );
}

module.exports = { BUTTONS, VALIDATION_PAGE, detail };

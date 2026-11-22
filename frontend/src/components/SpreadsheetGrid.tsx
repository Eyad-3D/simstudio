import { useEffect, useRef, useState, type ReactNode } from "react";
import { ClipboardPaste, Copy, Palette } from "lucide-react";
import { useOwnerWindow } from "../ownerWindow";
import { useUIStore, type TableShade } from "../store/uiStore";
import { useProjectStore } from "../store/projectStore";

/** `fn` as an undo step of its own: not merged with a value typed just
 *  before it, nor with the next edit (the store merges quick edits of one
 *  field, as typing does). */
function ownStep<T>(fn: () => T): T {
  useProjectStore.getState().endEdit();
  const out = fn();
  useProjectStore.getState().endEdit();
  return out;
}
import { GridMenu, type GridMenuItem } from "./GridMenu";
import { interpolateBlock, offsetBlock, scaleBlock, shadeStyle, type Block } from "./gridTools";

export type CellKind = "corner" | "colHeader" | "rowHeader" | "body";

export interface GridCell {
  text: string;
  kind: CellKind;
  readOnly?: boolean;
  /** a breakpoint in a body column (a single curve's first column): not a
   *  value, and its right-click inserts or deletes its row */
  axis?: boolean;
}

/** A row or a column of the grid, as the right-click on its header names it. */
export type GridLine = "row" | "col";

export interface GridRange {
  r0: number;
  c0: number;
  r1: number;
  c1: number;
}

/** A problem with text typed into a cell: an error is never committed; a
 *  warning is shown while typing and the value is still accepted. */
export interface GridIssue {
  level: "error" | "warning";
  text: string;
}

interface Sel {
  anchor: { r: number; c: number };
  active: { r: number; c: number };
}

/** A cell being edited; `typed` when the edit began with a typed character. */
interface Edit {
  r: number;
  c: number;
  value: string;
  typed?: boolean;
}

function normRange(s: Sel): GridRange {
  return {
    r0: Math.min(s.anchor.r, s.active.r),
    c0: Math.min(s.anchor.c, s.active.c),
    r1: Math.max(s.anchor.r, s.active.r),
    c1: Math.max(s.anchor.c, s.active.c),
  };
}

/** Parse spreadsheet clipboard text (TSV rows) into a block of strings. */
export function parseClipboardMatrix(text: string): string[][] {
  const cleaned = text.replace(/\r\n/g, "\n").replace(/\r/g, "\n").replace(/\n+$/, "");
  if (cleaned === "") return [];
  return cleaned.split("\n").map((row) => row.split("\t"));
}

/** Why a pasted block needs the import's preview instead of going into the
 *  cells as it is, or null: a cell that is not a number (a title, a unit,
 *  a decimal comma), or a table with its own axes (an empty top-left cell,
 *  breakpoints along the first row and down the first column) pasted
 *  anywhere but the grid's corner. */
export function importReason(block: string[][], at: { r: number; c: number }): string | null {
  const bad = block
    .flat()
    .map((t) => t.trim())
    .find((t) => t !== "" && !Number.isFinite(Number(t)));
  if (bad !== undefined)
    return /[A-Za-z°%[\]()/]/.test(bad)
      ? `the clipboard has titles or units ("${bad.slice(0, 40)}").`
      : `the clipboard has numbers written another way ("${bad.slice(0, 20)}": a decimal comma or thousands separators).`;
  const t = (s: string | undefined) => (s ?? "").trim();
  if (
    (at.r !== 0 || at.c !== 0) &&
    block.length >= 2 &&
    block[0].length >= 2 &&
    t(block[0][0]) === "" &&
    block[0].slice(1).every((s) => t(s) !== "") &&
    block.slice(1).every((row) => t(row[0]) !== "")
  )
    return "the clipboard holds a table with its own axes (an empty top-left cell, breakpoints along the first row and down the first column).";
  return null;
}

/** The cells of `range` as clipboard text (TSV rows), as Excel copies them. */
export function matrixText(matrix: GridCell[][], range: GridRange): string {
  const lines: string[] = [];
  for (let r = range.r0; r <= range.r1; r++) {
    const row: string[] = [];
    for (let c = range.c0; c <= range.c1; c++) row.push(matrix[r]?.[c]?.text ?? "");
    lines.push(row.join("\t"));
  }
  return lines.join("\n");
}

/**
 * Excel-like grid over a matrix of cells. Supports single/range selection
 * (click, shift-click, drag, arrow keys), block copy (Ctrl+C → TSV) and
 * block paste (Ctrl+V from Excel), in-cell editing, and Delete to clear.
 * Copy and Paste are buttons too, above the grid, beside the editor's own
 * `tools`. Text that `validate` rejects is never committed: the cell turns
 * red and a message under the grid says why, so no value is dropped silently.
 *
 * The table tools: the values can be shaded low to high (`shade`, a switch
 * in the toolbar); a right-click on selected values (or Shift+F10) sets,
 * multiplies, offsets or interpolates them, and typing a value and
 * Ctrl+Enter sets every selected value to it; a right-click on a row or
 * column header inserts or deletes one (`onInsert`, `onDelete`). Every
 * change goes through `onPasteBlock` as one block, so it is one undo step
 * and is read in the units shown, as a typed value is.
 */
export function SpreadsheetGrid({
  matrix,
  onCommit,
  onPasteBlock,
  onClearRange,
  onSelectionChange,
  validate,
  columnClass,
  onHeaderOpen,
  tools,
  caption,
  onImportText,
  shade,
  onInsert,
  onDelete,
  focusCell,
}: {
  matrix: GridCell[][];
  onCommit: (r: number, c: number, text: string) => void;
  /** returns a message when the block cannot be pasted (nothing changes) */
  onPasteBlock: (r: number, c: number, block: string[][]) => string | void;
  onClearRange?: (cells: { r: number; c: number }[]) => void;
  onSelectionChange?: (range: GridRange) => void;
  validate?: (r: number, c: number, text: string) => GridIssue | null;
  /** optional per-column className (by column index) for width control */
  columnClass?: (c: number) => string | undefined;
  /** a double-click, Enter or F2 on a read-only cell (a header with a unit):
   *  true when it opened something (its unit chooser) */
  onHeaderOpen?: (r: number, c: number) => boolean;
  /** the editor's own buttons, in the toolbar after Copy and Paste */
  tools?: ReactNode;
  /** a line joined to the top of the grid (the table's units) */
  caption?: ReactNode;
  /** pasted text that is not a plain block of numbers fitting here: opens
   *  the import's preview with it, and why (ImportDataDialog) */
  onImportText?: (text: string, why: string) => void;
  /** what kind of table this is, for the shading switch (kept per kind) */
  shade?: keyof TableShade;
  /** a new row or column before or after the one at grid index `at` */
  onInsert?: (line: GridLine, at: number, where: "before" | "after") => void;
  /** the rows or columns at these grid indices go */
  onDelete?: (line: GridLine, at: number[]) => void;
  /** select this cell (a point clicked on the plot); a new object each time */
  focusCell?: { r: number; c: number } | null;
}) {
  const rows = matrix.length;
  const cols = matrix[0]?.length ?? 0;
  const containerRef = useRef<HTMLDivElement | null>(null);
  const boxRef = useRef<HTMLDivElement | null>(null);
  const win = useOwnerWindow();
  const shadeOn = useUIStore((s) => (shade ? s.tableShade[shade] : false));
  const setShade = useUIStore((s) => s.setTableShade);
  const [menu, setMenu] = useState<{ x: number; y: number; r: number; c: number } | null>(null);
  const [sel, setSel] = useState<Sel>({ anchor: { r: 1, c: 0 }, active: { r: 1, c: 0 } });
  const [editing, setEditingState] = useState<Edit | null>(null);
  // mirrors `editing` so a blur that fires as the editor goes away sees the
  // edit already finished instead of committing it a second time
  const editRef = useRef<Edit | null>(null);
  const setEditing = (e: Edit | null) => {
    editRef.current = e;
    setEditingState(e);
  };
  const [notice, setNotice] = useState<GridIssue | { level: "info"; text: string } | null>(null);
  const dragging = useRef(false);

  const range = normRange(sel);
  const inRange = (r: number, c: number) =>
    r >= range.r0 && r <= range.r1 && c >= range.c0 && c <= range.c1;

  useEffect(() => {
    const up = () => (dragging.current = false);
    win.addEventListener("mouseup", up);
    return () => win.removeEventListener("mouseup", up);
  }, [win]);

  useEffect(() => {
    onSelectionChange?.(range);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sel.anchor.r, sel.anchor.c, sel.active.r, sel.active.c]);

  const clamp = (r: number, c: number) => ({
    r: Math.min(rows - 1, Math.max(0, r)),
    c: Math.min(cols - 1, Math.max(0, c)),
  });

  // a line saying what an action did ("Multiplied 4 cells by 2.") goes when
  // the table changes another way (Undo, another window), so it never names
  // a change that is no longer there; `ownChange` lets the action's own through
  const sig = matrix.map((row) => row.map((cell) => cell.text).join("\t")).join("\n");
  const [shownSig, setShownSig] = useState(sig);
  const [ownChange, setOwnChange] = useState(false);
  if (sig !== shownSig) {
    setShownSig(sig);
    if (ownChange) setOwnChange(false);
    else if (notice?.level === "info") setNotice(null);
  }
  const say = (text: string) => {
    setNotice({ level: "info", text });
    setOwnChange(true);
  };

  const cellAt = (r: number, c: number): GridCell | undefined => matrix[r]?.[c];
  const isEditable = (r: number, c: number) => {
    const cell = cellAt(r, c);
    return cell && !cell.readOnly;
  };

  // (without scrolling: a grid half in view would jump to its top under the
  // pointer, and the press that began a selection would end it elsewhere)
  const focusGrid = () => containerRef.current?.focus({ preventScroll: true });

  // a point clicked on the plot selects its cell, scrolled into view
  const [focused, setFocused] = useState<typeof focusCell>(null);
  if (focusCell !== focused) {
    setFocused(focusCell);
    if (focusCell) setSel({ anchor: focusCell, active: focusCell });
  }
  useEffect(() => {
    if (focusCell)
      containerRef.current?.querySelector<HTMLElement>("td.ss-active")?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
  }, [focusCell]);

  // ---- the table tools ----------------------------------------------------
  /** A cell that holds one of the table's values (not a breakpoint). */
  const isValue = (r: number, c: number) => {
    const cell = cellAt(r, c);
    return !!cell && cell.kind === "body" && !cell.axis && !cell.readOnly;
  };
  /** The header of a row or a column, as its right-click names it. */
  const lineOf = (r: number, c: number): GridLine | null => {
    const cell = cellAt(r, c);
    if (!cell || cell.readOnly) return null;
    if (cell.kind === "rowHeader" || cell.axis) return "row";
    return cell.kind === "colHeader" ? "col" : null;
  };
  /** The values in the selection: the rows and columns of it that hold any. */
  const selectedValues = () => {
    const rs: number[] = [];
    const cs: number[] = [];
    for (let r = range.r0; r <= range.r1; r++) for (let c = range.c0; c <= range.c1; c++)
      if (isValue(r, c)) {
        if (!rs.includes(r)) rs.push(r);
        if (!cs.includes(c)) cs.push(c);
      }
    return { rs, cs };
  };
  const numberAt = (r: number, c: number) => {
    const t = cellAt(r, c)?.text.trim() ?? "";
    return t === "" || !Number.isFinite(Number(t)) ? null : Number(t);
  };
  /** Where a row and a column sit on their axes (their breakpoints as
   *  shown), for interpolating; their places in order when not numbers. */
  const coords = (idx: number[], at: (i: number) => GridCell | undefined) => {
    const n = idx.map((i) => Number(at(i)?.text.trim() || "x"));
    return n.every(Number.isFinite) ? n : idx.map((_, k) => k);
  };
  const rowCoords = (rs: number[]) =>
    coords(rs, (r) => matrix[r]?.find((cell) => cell.kind === "rowHeader" || cell.axis));
  const colCoords = (cs: number[]) => coords(cs, (c) => (cellAt(0, c)?.kind === "colHeader" ? cellAt(0, c) : undefined));

  /** Write cells as one block (one change, one undo step); `done` says what
   *  happened when it went in. */
  const writeCells = (changes: { r: number; c: number; text: string }[], done: string) => {
    if (changes.length === 0) return;
    const r0 = Math.min(...changes.map((x) => x.r));
    const c0 = Math.min(...changes.map((x) => x.c));
    const r1 = Math.max(...changes.map((x) => x.r));
    const c1 = Math.max(...changes.map((x) => x.c));
    const block = Array.from({ length: r1 - r0 + 1 }, (_, i) =>
      Array.from({ length: c1 - c0 + 1 }, (_, j) => cellAt(r0 + i, c0 + j)?.text ?? ""),
    );
    for (const x of changes) block[x.r - r0][x.c - c0] = x.text;
    const problem = ownStep(() => onPasteBlock(r0, c0, block));
    if (problem) setNotice({ level: "error", text: problem.replace(/^Paste not applied/, "Not applied") });
    else say(done);
  };
  /** Every selected value set to `text` (typed in the unit shown). */
  const fillWith = (text: string): string | void => {
    const { rs, cs } = selectedValues();
    const t = text.trim();
    if (t === "") return "Enter a value.";
    const bad = rs.length ? validate?.(rs[0], cs[0], t) : null;
    if (bad?.level === "error") return bad.text;
    const cells = rs.flatMap((r) => cs.filter((c) => isValue(r, c)).map((c) => ({ r, c, text: t })));
    writeCells(cells, `Set ${cells.length} cells to ${t}.`);
  };
  /** The selected values through `fn` (empty cells stay empty). */
  const mapValues = (fn: (b: Block) => Block | string, done: string): string | void => {
    const { rs, cs } = selectedValues();
    const out = fn(rs.map((r) => cs.map((c) => numberAt(r, c))));
    if (typeof out === "string") return out.charAt(0).toUpperCase() + out.slice(1);
    const cells = rs.flatMap((r, i) =>
      cs.flatMap((c, j) => {
        const v = out[i][j];
        return v === null || v === numberAt(r, c) || !isValue(r, c) ? [] : [{ r, c, text: String(v) }];
      }),
    );
    if (cells.length === 0) return;
    writeCells(cells, done.replace("{n}", String(cells.length)));
  };
  const numberOrSay = (t: string, what: string): number | string =>
    t !== "" && Number.isFinite(Number(t)) ? Number(t) : `Enter a ${what} as a number (a point for decimals).`;

  /** The menu at the pointer (or under the cell, from the keyboard), for the
   *  cell right-clicked: a header's or the selected values'. The cell is
   *  selected first when it is outside the selection, as in a spreadsheet. */
  const openMenu = (r: number, c: number, at?: { x: number; y: number }) => {
    const line = lineOf(r, c);
    if (line ? !onInsert && !onDelete : !isValue(r, c)) return;
    const box = boxRef.current?.getBoundingClientRect();
    const cellBox = containerRef.current
      ?.querySelector<HTMLElement>(`tr:nth-child(${r + 1}) > td:nth-child(${c + 1})`)
      ?.getBoundingClientRect();
    const p = at ?? { x: (cellBox?.left ?? 0) + 8, y: cellBox?.bottom ?? 0 };
    const inSel = line === "row" ? r >= range.r0 && r <= range.r1 : line === "col" ? c >= range.c0 && c <= range.c1 : inRange(r, c);
    if (!inSel) setSel({ anchor: { r, c }, active: { r, c } });
    setMenu({ x: Math.max(0, p.x - (box?.left ?? 0)), y: Math.max(0, p.y - (box?.top ?? 0)), r, c });
  };
  const closeMenu = (refocus: boolean) => {
    setMenu(null);
    if (refocus) setTimeout(focusGrid, 0);
  };
  /** The open menu's items, for the selection as it is now. */
  const menuItems = (r: number, c: number): { label: string; items: GridMenuItem[] } => {
    const line = lineOf(r, c);
    if (line) {
      // row 0 holds the column headers and column 0 the row headers: the lines start at 1
      const at = line === "row" ? r : c;
      const [lo, hi] = line === "row" ? [range.r0, range.r1] : [range.c0, range.c1];
      const drop = at >= lo && at <= hi ? Array.from({ length: hi - Math.max(lo, 1) + 1 }, (_, k) => Math.max(lo, 1) + k) : [at];
      const word = line === "row" ? "row" : "column";
      const items: GridMenuItem[] = [];
      if (onInsert)
        items.push(
          { label: line === "row" ? "Insert row above" : "Insert column to the left", run: () => ownStep(() => onInsert(line, at, "before")) },
          { label: line === "row" ? "Insert row below" : "Insert column to the right", run: () => ownStep(() => onInsert(line, at, "after")) },
        );
      if (onDelete)
        items.push({
          label: drop.length > 1 ? `Delete ${drop.length} ${word}s` : `Delete ${word}`,
          // a table keeps at least one breakpoint on every axis
          disabled: drop.length >= (line === "row" ? rows : cols) - 1,
          run: () => ownStep(() => onDelete(line, drop)),
        });
      return { label: line === "row" ? "Row actions" : "Column actions", items };
    }
    const { rs, cs } = selectedValues();
    return {
      label: "Cell actions",
      items: [
        { label: "Set to…", ask: "Set the selected cells to", run: fillWith },
        {
          label: "Multiply by…",
          ask: "Multiply the selected cells by",
          run: (t) => {
            const f = numberOrSay(t, "factor");
            return typeof f === "string" ? f : mapValues((b) => scaleBlock(b, f), `Multiplied {n} cells by ${f}.`);
          },
        },
        {
          label: "Add…",
          ask: "Add to the selected cells",
          run: (t) => {
            const d = numberOrSay(t, "value to add");
            return typeof d === "string" ? d : mapValues((b) => offsetBlock(b, d), `Added ${d} to {n} cells.`);
          },
        },
        {
          label: "Interpolate between the edges",
          disabled: rs.length < 3 && cs.length < 3,
          run: () => {
            const bad = mapValues(
              (b) => interpolateBlock(b, rowCoords(rs), colCoords(cs)),
              "Interpolated {n} cells between the edges of the selection.",
            );
            if (bad) setNotice({ level: "error", text: `Not interpolated: ${bad.charAt(0).toLowerCase()}${bad.slice(1)}` });
          },
        },
      ],
    };
  };

  // the values' lowest and highest, for their shades
  let lo = Infinity;
  let hi = -Infinity;
  if (shadeOn)
    matrix.forEach((row, r) =>
      row.forEach((_, c) => {
        const v = isValue(r, c) ? numberAt(r, c) : null;
        if (v !== null) [lo, hi] = [Math.min(lo, v), Math.max(hi, v)];
      }),
    );

  const startEdit = (r: number, c: number, seed?: string) => {
    if (!isEditable(r, c)) {
      if (seed === undefined && cellAt(r, c)) onHeaderOpen?.(r, c);
      return;
    }
    setNotice(null);
    setEditing({ r, c, value: seed ?? cellAt(r, c)?.text ?? "", typed: seed !== undefined });
  };

  /** What is wrong with an edit's text, if anything (unchanged text never is). */
  const issueFor = (edit: Edit | null): GridIssue | null => {
    if (!edit || cellAt(edit.r, edit.c)?.text === edit.value) return null;
    return validate?.(edit.r, edit.c, edit.value) ?? null;
  };

  /** Finish the edit in progress. Rejected text keeps the editor open when
   *  the edit ends from the keyboard; when focus leaves the cell instead, the
   *  old value stays and the message says so. */
  const commitEdit = (how: "down" | "right" | "stay" | "blur") => {
    const edit = editRef.current;
    if (!edit) return;
    const { r, c, value } = edit;
    const issue = issueFor(edit);
    if (issue?.level === "error") {
      if (how !== "blur") {
        setNotice(issue);
        return;
      }
      setNotice({ level: "error", text: `${issue.text} Kept ${cellAt(r, c)?.text || "the empty cell"}.` });
      setEditing(null);
      return;
    }
    if (cellAt(r, c)?.text !== value) onCommit(r, c, value);
    setNotice(issue);
    setEditing(null);
    if (how === "blur") return; // focus has already gone elsewhere
    if (how !== "stay") {
      const n = how === "down" ? clamp(r + 1, c) : clamp(r, c + 1);
      setSel({ anchor: n, active: n });
    }
    setTimeout(focusGrid, 0);
  };

  /** Ctrl+Enter: the typed value goes into every selected value, as in
   *  Excel; with one cell selected, it is kept and the cell stays selected. */
  const fillFromEdit = () => {
    const edit = editRef.current;
    if (!edit) return;
    const { rs, cs } = selectedValues();
    if (rs.length * cs.length <= 1 || !isValue(edit.r, edit.c)) return commitEdit("stay");
    const bad = fillWith(edit.value);
    if (bad) return setNotice({ level: "error", text: bad });
    setEditing(null);
    setTimeout(focusGrid, 0);
  };

  const cancelEdit = () => {
    setEditing(null);
    setNotice(null);
    setTimeout(focusGrid, 0);
  };

  const moveActive = (dr: number, dc: number, extend: boolean) => {
    setSel((prev) => {
      const active = clamp(prev.active.r + dr, prev.active.c + dc);
      return { anchor: extend ? prev.anchor : active, active };
    });
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (editing) return;
    if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
      // the right-click menu from the keyboard, under the active cell
      e.preventDefault();
      openMenu(sel.active.r, sel.active.c);
      return;
    }
    const meta = e.ctrlKey || e.metaKey;
    if (meta && (e.key === "a" || e.key === "A")) {
      e.preventDefault();
      setSel({ anchor: { r: 0, c: 0 }, active: { r: rows - 1, c: cols - 1 } });
      return;
    }
    if (meta) return; // let copy/paste (onCopy/onPaste) run
    switch (e.key) {
      case "ArrowUp":
        e.preventDefault();
        moveActive(-1, 0, e.shiftKey);
        return;
      case "ArrowDown":
        e.preventDefault();
        moveActive(1, 0, e.shiftKey);
        return;
      case "ArrowLeft":
        e.preventDefault();
        moveActive(0, -1, e.shiftKey);
        return;
      case "ArrowRight":
        e.preventDefault();
        moveActive(0, 1, e.shiftKey);
        return;
      case "Enter":
        e.preventDefault();
        if (isEditable(sel.active.r, sel.active.c)) startEdit(sel.active.r, sel.active.c);
        else if (!onHeaderOpen?.(sel.active.r, sel.active.c)) moveActive(1, 0, false);
        return;
      case "Tab":
        e.preventDefault();
        moveActive(0, e.shiftKey ? -1 : 1, false);
        return;
      case "F2":
        e.preventDefault();
        startEdit(sel.active.r, sel.active.c);
        return;
      case "Delete":
      case "Backspace":
        e.preventDefault();
        if (onClearRange) {
          const cells: { r: number; c: number }[] = [];
          for (let r = range.r0; r <= range.r1; r++)
            for (let c = range.c0; c <= range.c1; c++) cells.push({ r, c });
          onClearRange(cells);
        }
        return;
    }
    // printable character → begin editing with it
    if (e.key.length === 1 && !e.altKey) {
      if (isEditable(sel.active.r, sel.active.c)) {
        e.preventDefault();
        startEdit(sel.active.r, sel.active.c, e.key);
      }
    }
  };

  const onCopy = (e: React.ClipboardEvent) => {
    if (editing) return;
    e.clipboardData.setData("text/plain", matrixText(matrix, range));
    e.preventDefault();
  };

  const onPaste = (e: React.ClipboardEvent) => {
    if (editing) return;
    const text = e.clipboardData.getData("text/plain");
    if (parseClipboardMatrix(text).length === 0) return;
    e.preventDefault();
    pasteText(text);
  };

  /** Clipboard text in at the active cell. A copy of this whole table (it
   *  starts with the table's own top-left header) goes back where it came
   *  from: at the top-left, less a header row no number can go in. */
  const pasteText = (text: string) => {
    let block = parseClipboardMatrix(text);
    let { r, c } = sel.active;
    const corner = cellAt(0, 0);
    const own = corner?.readOnly && block[0]?.[0] === corner.text;
    if (own) {
      [r, c] = [0, 0];
      if (matrix[0].every((cell) => cell.readOnly)) [block, r] = [block.slice(1), 1];
    }
    if (block.length === 0) return;
    const several = block.length > 1 || block[0].length > 1;
    const why = onImportText && several && !own ? importReason(block, { r, c }) : null;
    if (why) {
      setNotice(null);
      onImportText!(text, why);
      return;
    }
    if (block.length === 1 && block[0].length === 1) {
      if (!isEditable(r, c)) return;
      const issue = issueFor({ r, c, value: block[0][0] });
      if (issue?.level === "error") {
        setNotice({ level: "error", text: `Paste not applied: ${issue.text}` });
        return;
      }
      if (cellAt(r, c)?.text !== block[0][0]) ownStep(() => onCommit(r, c, block[0][0]));
      setNotice(issue);
    } else {
      const problem = ownStep(() => onPasteBlock(r, c, block));
      if (problem && onImportText && !own) {
        // a block that does not fit here: the preview shows how it can be read
        setNotice(null);
        onImportText(text, `${problem.replace(/^Paste not applied: /, "").replace(/\.?$/, ".")}`);
        return;
      }
      // a paste past the end grows the table (one change, one undo step), and says so
      const more = (n: number, word: string) => (n > 0 ? [`${n} ${word}${n > 1 ? "s" : ""}`] : []);
      const added = [
        ...more(r + block.length - rows, "row"),
        ...more(c + Math.max(...block.map((b) => b.length)) - cols, "column"),
      ];
      if (problem) setNotice({ level: "error", text: problem });
      else if (added.length)
        say(
          `The paste went past the end of the table: added ${added.join(" and ")}. A breakpoint the paste did not give continues the last step.`,
        );
      else setNotice(null);
    }
  };

  // the buttons: the selection when it is more than one cell, else the whole table
  const several = range.r0 !== range.r1 || range.c0 !== range.c1;
  const copyButton = async () => {
    const all = { r0: 0, c0: 0, r1: rows - 1, c1: cols - 1 };
    try {
      // the window the table is in: the clipboard is only the focused window's
      await win.navigator.clipboard.writeText(matrixText(matrix, several ? range : all));
      setNotice({ level: "info", text: several ? "Copied the selected cells." : "Copied the whole table." });
    } catch {
      setNotice({
        level: "error",
        text: "Could not copy: the clipboard is not available. Select the cells and press Ctrl+C instead.",
      });
    }
  };
  const pasteButton = async () => {
    let text: string;
    try {
      text = await win.navigator.clipboard.readText();
    } catch {
      setNotice({
        level: "error",
        text: "Could not read the clipboard: permission refused. Click a cell and press Ctrl+V instead.",
      });
      return;
    }
    if (parseClipboardMatrix(text).length === 0) {
      setNotice({ level: "info", text: "Nothing to paste: the clipboard has no text." });
      return;
    }
    pasteText(text);
  };

  const liveIssue = issueFor(editing);
  const shownNotice = liveIssue ?? notice;

  const cellClass = (cell: GridCell) => {
    switch (cell.kind) {
      case "corner":
        return "ss-cell ss-cell-corner";
      case "colHeader":
        return "ss-cell ss-cell-colhead";
      case "rowHeader":
        return "ss-cell ss-cell-rowhead";
      default:
        return "ss-cell ss-cell-body";
    }
  };

  return (
    <>
      <div role="group" aria-label="Table tools" className="ss-table-tools">
        <button
          type="button"
          className="ss-toolbtn"
          onClick={() => void copyButton()}
          title="Copy the selected cells, or the whole table when one cell or none is selected (Ctrl+C). Pastes into Excel."
        >
          <Copy size={12} aria-hidden /> Copy
        </button>
        <button
          type="button"
          className="ss-toolbtn"
          onClick={() => void pasteButton()}
          title="Paste cells copied here or from Excel, from the selected cell on (Ctrl+V). A whole table copied here goes back in place."
        >
          <ClipboardPaste size={12} aria-hidden /> Paste
        </button>
        {tools}
        {shade && (
          <button
            type="button"
            className="ss-toolbtn"
            aria-pressed={shadeOn}
            onClick={() => setShade(shade, !shadeOn)}
            title={`${shadeOn ? "Stop shading" : "Shade"} the cells by value, from purple for the lowest to yellow for the highest. Kept for every ${shade === "curve" ? "single curve" : "table of two or more axes"}.`}
          >
            <Palette size={12} aria-hidden /> Shade
          </button>
        )}
      </div>
      <div className="ss-table-box relative" ref={boxRef}>
        {menu && <GridMenu x={menu.x} y={menu.y} {...menuItems(menu.r, menu.c)} onClose={closeMenu} />}
        {caption && <div className="ss-grid-caption">{caption}</div>}
        <div
          // nokey: Delete and Backspace here clear cells, and must not reach
          // React Flow, which would delete the part selected on the diagram
          className={`ss-grid-frame nokey${caption ? " ss-grid-frame-captioned" : ""}`}
          ref={containerRef}
          tabIndex={0}
          onKeyDown={onKeyDown}
          onCopy={onCopy}
          onPaste={onPaste}
        >
          <table className="ss-grid">
            <tbody>
              {matrix.map((row, r) => (
                <tr key={r}>
                  {row.map((cell, c) => {
                    const active = sel.active.r === r && sel.active.c === c;
                    const isEditingCell = editing?.r === r && editing?.c === c;
                    const v = shadeOn && isValue(r, c) ? numberAt(r, c) : null;
                    // a block selected has a line round it, so it shows on any shade
                    const edge =
                      several && inRange(r, c)
                        ? [
                            r === range.r0 && "inset 0 2px 0 0",
                            r === range.r1 && "inset 0 -2px 0 0",
                            c === range.c0 && "inset 2px 0 0 0",
                            c === range.c1 && "inset -2px 0 0 0",
                          ]
                            .filter(Boolean)
                            .map((s) => `${s} var(--ss-accent)`)
                            .join(", ")
                        : "";
                    return (
                      <td
                        key={c}
                        style={{ ...(v === null ? {} : shadeStyle(v, lo, hi)), ...(edge ? { boxShadow: edge } : {}) }}
                        onContextMenu={(e) => {
                          if (isEditingCell) return;
                          e.preventDefault();
                          focusGrid();
                          openMenu(r, c, { x: e.clientX, y: e.clientY });
                        }}
                        className={`${inRange(r, c) ? "ss-selected " : ""}${active ? "ss-active " : ""}${
                          isEditingCell ? "ss-editing " : ""
                        }ss-td-${cell.kind} ${
                          isEditingCell && liveIssue ? `ss-cell-${liveIssue.level} ` : ""
                        }${columnClass?.(c) ?? ""}`}
                        onMouseDown={(e) => {
                          if (isEditingCell) return;
                          e.preventDefault();
                          focusGrid();
                          // a right-click keeps the selection: its menu acts on it
                          if (e.button === 2) return;
                          if (e.shiftKey) setSel((prev) => ({ ...prev, active: { r, c } }));
                          else {
                            setSel({ anchor: { r, c }, active: { r, c } });
                            dragging.current = true;
                          }
                        }}
                        onMouseEnter={() => {
                          if (dragging.current) setSel((prev) => ({ ...prev, active: { r, c } }));
                        }}
                        onDoubleClick={() => startEdit(r, c)}
                      >
                        {isEditingCell ? (
                          <input
                            className="ss-cell-input"
                            autoFocus
                            value={editing.value}
                            title={liveIssue?.text}
                            aria-invalid={liveIssue?.level === "error" || undefined}
                            onChange={(e) => setEditing({ ...editing, value: e.target.value })}
                            onFocus={(e) => {
                              // keep a typed first character: caret after it, not selected
                              const end = e.target.value.length;
                              if (editing.typed) e.target.setSelectionRange(end, end);
                              else e.target.select();
                            }}
                            onBlur={() => commitEdit("blur")}
                            onKeyDown={(e) => {
                              if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
                                e.preventDefault();
                                e.stopPropagation();
                                fillFromEdit();
                              } else if (e.key === "Enter") {
                                e.preventDefault();
                                commitEdit("down");
                              } else if (e.key === "Tab") {
                                e.preventDefault();
                                commitEdit("right");
                              } else if (e.key === "Escape") {
                                // cancel this edit only; the dialog must not see the key
                                e.preventDefault();
                                e.stopPropagation();
                                cancelEdit();
                              } else {
                                e.stopPropagation();
                              }
                            }}
                          />
                        ) : (
                          <div className={cellClass(cell)} title={cell.text}>
                            {cell.text || " "}
                          </div>
                        )}
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
      {shownNotice && (
        <div
          data-grid-notice
          role={shownNotice.level === "error" ? "alert" : "status"}
          className={`text-meta leading-tight ${
            shownNotice.level === "error"
              ? "text-[color:var(--ss-err)]"
              : shownNotice.level === "warning"
                ? "text-[color:var(--ss-warn)]"
                : "text-[color:var(--ss-text-dim)]"
          }`}
        >
          {shownNotice.text}
        </div>
      )}
    </>
  );
}

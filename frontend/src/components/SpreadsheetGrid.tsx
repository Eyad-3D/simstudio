import { useEffect, useRef, useState } from "react";

export type CellKind = "corner" | "colHeader" | "rowHeader" | "body";

export interface GridCell {
  text: string;
  kind: CellKind;
  readOnly?: boolean;
}

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

/**
 * Excel-like grid over a matrix of cells. Supports single/range selection
 * (click, shift-click, drag, arrow keys), block copy (Ctrl+C → TSV) and
 * block paste (Ctrl+V from Excel), in-cell editing, and Delete to clear.
 * Text that `validate` rejects is never committed: the cell turns red and a
 * message under the grid says why, so no value is dropped silently.
 */
export function SpreadsheetGrid({
  matrix,
  onCommit,
  onPasteBlock,
  onClearRange,
  onSelectionChange,
  validate,
  columnClass,
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
}) {
  const rows = matrix.length;
  const cols = matrix[0]?.length ?? 0;
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [sel, setSel] = useState<Sel>({ anchor: { r: 1, c: 0 }, active: { r: 1, c: 0 } });
  const [editing, setEditingState] = useState<Edit | null>(null);
  // mirrors `editing` so a blur that fires as the editor goes away sees the
  // edit already finished instead of committing it a second time
  const editRef = useRef<Edit | null>(null);
  const setEditing = (e: Edit | null) => {
    editRef.current = e;
    setEditingState(e);
  };
  const [notice, setNotice] = useState<GridIssue | null>(null);
  const dragging = useRef(false);

  const range = normRange(sel);
  const inRange = (r: number, c: number) =>
    r >= range.r0 && r <= range.r1 && c >= range.c0 && c <= range.c1;

  useEffect(() => {
    const up = () => (dragging.current = false);
    window.addEventListener("mouseup", up);
    return () => window.removeEventListener("mouseup", up);
  }, []);

  useEffect(() => {
    onSelectionChange?.(range);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sel.anchor.r, sel.anchor.c, sel.active.r, sel.active.c]);

  const clamp = (r: number, c: number) => ({
    r: Math.min(rows - 1, Math.max(0, r)),
    c: Math.min(cols - 1, Math.max(0, c)),
  });

  const cellAt = (r: number, c: number): GridCell | undefined => matrix[r]?.[c];
  const isEditable = (r: number, c: number) => {
    const cell = cellAt(r, c);
    return cell && !cell.readOnly;
  };

  const focusGrid = () => containerRef.current?.focus();

  const startEdit = (r: number, c: number, seed?: string) => {
    if (!isEditable(r, c)) return;
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
  const commitEdit = (how: "down" | "right" | "blur") => {
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
    const n = how === "down" ? clamp(r + 1, c) : clamp(r, c + 1);
    setSel({ anchor: n, active: n });
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
        else moveActive(1, 0, false);
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
    const lines: string[] = [];
    for (let r = range.r0; r <= range.r1; r++) {
      const row: string[] = [];
      for (let c = range.c0; c <= range.c1; c++) row.push(cellAt(r, c)?.text ?? "");
      lines.push(row.join("\t"));
    }
    e.clipboardData.setData("text/plain", lines.join("\n"));
    e.preventDefault();
  };

  const onPaste = (e: React.ClipboardEvent) => {
    if (editing) return;
    const text = e.clipboardData.getData("text/plain");
    const block = parseClipboardMatrix(text);
    if (block.length === 0) return;
    e.preventDefault();
    const { r, c } = sel.active;
    if (block.length === 1 && block[0].length === 1) {
      if (!isEditable(r, c)) return;
      const issue = issueFor({ r, c, value: block[0][0] });
      if (issue?.level === "error") {
        setNotice({ level: "error", text: `Paste not applied: ${issue.text}` });
        return;
      }
      if (cellAt(r, c)?.text !== block[0][0]) onCommit(r, c, block[0][0]);
      setNotice(issue);
    } else {
      const problem = onPasteBlock(r, c, block);
      setNotice(problem ? { level: "error", text: problem } : null);
    }
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
      <div
        className="ss-grid-frame"
        ref={containerRef}
        tabIndex={0}
        onKeyDown={onKeyDown}
        onCopy={onCopy}
        onPaste={onPaste}
        style={{ outline: "none" }}
      >
        <table className="ss-grid">
          <tbody>
            {matrix.map((row, r) => (
              <tr key={r}>
                {row.map((cell, c) => {
                  const active = sel.active.r === r && sel.active.c === c;
                  const isEditingCell = editing?.r === r && editing?.c === c;
                  return (
                    <td
                      key={c}
                      className={`${inRange(r, c) ? "ss-selected " : ""}${active ? "ss-active " : ""}${
                        isEditingCell && liveIssue ? `ss-cell-${liveIssue.level} ` : ""
                      }${columnClass?.(c) ?? ""}`}
                      onMouseDown={(e) => {
                        if (isEditingCell) return;
                        e.preventDefault();
                        focusGrid();
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
                            if (e.key === "Enter") {
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
      {shownNotice && (
        <div
          data-grid-notice
          role={shownNotice.level === "error" ? "alert" : "status"}
          className={`text-[10px] leading-tight ${
            shownNotice.level === "error" ? "text-red-600" : "text-amber-600"
          }`}
        >
          {shownNotice.text}
        </div>
      )}
    </>
  );
}

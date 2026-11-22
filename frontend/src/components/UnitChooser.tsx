import { Fragment, useEffect, useId, useRef } from "react";
import { alternatives, mapTable, reinterpret, type UnitSlot } from "../units";

/** A parameter's unit, as a control that opens its unit chooser: the unit's
 *  text alone (underlined, index.css), so the Unit column is no wider than
 *  the text and a narrow panel's value fields keep their room (GUI-34). */
export function UnitButton({
  what,
  unit,
  open,
  onOpen,
  buttonRef,
}: {
  what: string;
  unit: string;
  open: boolean;
  onOpen: () => void;
  buttonRef?: (el: HTMLButtonElement | null) => void;
}) {
  return (
    <button
      ref={buttonRef}
      type="button"
      className="ss-unit-button"
      aria-label={`${unit}, unit of ${what}`}
      aria-expanded={open}
      title="Show in another unit, or say the number is in another unit"
      // a double-click opens it too (it never toggles it shut)
      onClick={onOpen}
    >
      {unit}
    </button>
  );
}

/** The two things a unit can mean, kept apart: "Show in another unit" (the
 *  same value, converted; what is typed is then read in it) and "The number
 *  is in another unit" (the number on screen stays, so the value changes).
 *  `many`: for a whole axis or all of a table's values. Esc closes it, and
 *  only it. */
export function UnitChooser({
  what,
  lib,
  unit,
  many = false,
  onShow,
  onReinterpret,
  onClose,
}: {
  what: string;
  lib: string;
  unit: string;
  many?: boolean;
  onShow: (unit: string) => void;
  onReinterpret: (unit: string) => void;
  onClose: () => void;
}) {
  const box = useRef<HTMLDivElement>(null);
  const id = useId();
  useEffect(() => box.current?.querySelector<HTMLButtonElement>('button[aria-pressed="true"]')?.focus(), []);
  const units = alternatives(lib);
  const name = (u: string) => (u === lib ? `${u} (as saved)` : u);
  return (
    <div
      ref={box}
      role="group"
      aria-label={`Unit of ${what}`}
      className="ss-unit-chooser"
      onKeyDown={(e) => {
        if (e.key !== "Escape") return;
        // the chooser closes; the window or dialog stays
        e.preventDefault();
        e.stopPropagation();
        onClose();
      }}
    >
      <p id={`${id}-show`}>
        <b>Show in another unit</b>: the same value, converted. What you type is then read in that unit.
      </p>
      <div role="group" aria-labelledby={`${id}-show`} className="flex flex-wrap gap-1">
        {units.map((u) => (
          <button key={u} type="button" aria-pressed={u === unit} onClick={() => onShow(u)}>
            {name(u)}
          </button>
        ))}
      </div>
      <p id={`${id}-is`}>
        <b>The {many ? "numbers are" : "number is"} in another unit</b>: {many ? "they stay" : "it stays"} as on
        screen, and the {many ? "values change" : "value changes"} (one step, Undo takes it back).
      </p>
      <div role="group" aria-labelledby={`${id}-is`} className="flex flex-wrap gap-1">
        {units
          .filter((u) => u !== unit)
          .map((u) => (
            <button key={u} type="button" onClick={() => onReinterpret(u)}>
              {u}
            </button>
          ))}
      </div>
      <div>
        <button type="button" onClick={onClose}>
          Cancel (Esc)
        </button>
      </div>
    </div>
  );
}

/** One column of a table's numbers: an axis (by its index) or the values. */
export interface UnitColumn {
  slot: UnitSlot;
  /** "Rows: Speed", "Values: Torque" */
  label: string;
  /** "the Speed axis", for the chooser's name */
  what: string;
  /** the library's unit, and the one it is shown in */
  lib: string;
  unit: string;
}

/** A table's units, one control per axis and for the values (any number of
 *  axes), and the chooser of the one that is open. `set` stores the unit;
 *  with a table, that is the table's new value too ("the numbers are in
 *  another unit"). */
export function TableUnits({
  columns,
  table,
  open,
  setOpen,
  set,
}: {
  columns: UnitColumn[];
  table: unknown;
  open: UnitSlot | null;
  setOpen: (slot: UnitSlot | null) => void;
  set: (slot: UnitSlot, unit: string, table?: unknown) => void;
}) {
  const buttons = useRef(new Map<UnitSlot, HTMLButtonElement | null>());
  const col = columns.find((c) => c.slot === open && alternatives(c.lib).length > 0);
  const close = (slot: UnitSlot) => {
    setOpen(null);
    buttons.current.get(slot)?.focus();
  };
  return (
    <div className="flex flex-col gap-1">
      <div className="text-meta text-[color:var(--ss-text-dim)]">
        {columns.map((c, i) => (
          <Fragment key={String(c.slot)}>
            {i > 0 && " · "}
            {alternatives(c.lib).length > 0 ? (
              // the title opens its chooser on a double-click, as the button on a click
              <span onDoubleClick={() => setOpen(c.slot)}>
                {c.label} (
                <UnitButton
                  what={c.what}
                  unit={c.unit}
                  open={open === c.slot}
                  onOpen={() => setOpen(c.slot)}
                  buttonRef={(el) => void buttons.current.set(c.slot, el)}
                />
                )
              </span>
            ) : (
              `${c.label} (${c.unit})`
            )}
          </Fragment>
        ))}
      </div>
      {col && (
        <UnitChooser
          what={col.what}
          lib={col.lib}
          unit={col.unit}
          many
          onShow={(u) => {
            set(col.slot, u);
            close(col.slot);
          }}
          onReinterpret={(u) => {
            set(col.slot, u, mapTable(table, col.slot, (n) => reinterpret(n, col.lib, col.unit, u)));
            close(col.slot);
          }}
          onClose={() => close(col.slot)}
        />
      )}
    </div>
  );
}

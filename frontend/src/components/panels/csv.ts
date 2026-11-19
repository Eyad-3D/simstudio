/** Rows as CSV text. A field with a comma, a double quote or a line break goes
 *  in double quotes, its quotes doubled (RFC 4180), so an element label such
 *  as "Motor, rear" stays one column. */
export function csvText(rows: (string | number)[][]): string {
  const quote = (v: string | number) => {
    const s = String(v);
    return /[",\r\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
  };
  return rows.map((r) => r.map(quote).join(",")).join("\n");
}

/** Rows as a CSV file Excel opens as written: UTF-8 with a byte-order mark,
 *  so "N·m" and "°C" do not turn into "NÂ·m" and "Â°C". */
export function csvBlob(rows: (string | number)[][]): Blob {
  return new Blob(["\ufeff" + csvText(rows)], { type: "text/csv;charset=utf-8" });
}

// Every figure a person reads is formatted here, so they all follow one rule:
// a value that rounds to zero has no sign ("0", never "-0.0" or
// "-0.000 (-0.00 %)"). Same arguments as Number.toLocaleString.

const cache = new Map<string, Intl.NumberFormat>();

/** `v.toLocaleString(locale, options)`, except that a value rounding to zero
 *  reads without a sign. */
export function formatNumber(v: number, locale?: string, options: Intl.NumberFormatOptions = {}): string {
  const key = `${locale ?? ""}|${JSON.stringify(options)}`;
  let f = cache.get(key);
  if (!f) cache.set(key, (f = new Intl.NumberFormat(locale, options)));
  const s = f.format(v);
  // a minus with no nonzero digit is a rounded zero ("-0.00"): drop it
  return /^[-−]/.test(s) && !/[1-9]/.test(s) ? s.slice(1) : s;
}

/** `v.toFixed(digits)` for display: exactly `digits` decimals, grouped, no
 *  sign on a rounded zero. */
export const fixedNumber = (v: number, digits: number, locale?: string) =>
  formatNumber(v, locale, { minimumFractionDigits: digits, maximumFractionDigits: digits });

/** A difference: "+1.5", "-0.25", and "0" (no sign) when it rounds to zero. */
export const signedNumber = (v: number, digits: number, locale?: string) =>
  formatNumber(v, locale, { minimumFractionDigits: digits, maximumFractionDigits: digits, signDisplay: "exceptZero" });

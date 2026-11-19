import { useEffect, useState } from "react";

/** The clock (epoch ms), read again every `ms` while `on`: for a running
 *  sweep's time left, which counts down between its points (STU-17). */
export function useNow(ms: number, on: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!on) return;
    const id = window.setInterval(() => setNow(Date.now()), ms);
    return () => window.clearInterval(id);
  }, [ms, on]);
  return now;
}

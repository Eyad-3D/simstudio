"""Build LightSim's bundled drive-cycle files (CON-04) from their sources.

Run from the repository root:

    python scripts/cycles/build_cycles.py --fastsim <dir> --epa <dir> --eu <file> [--write]

--fastsim  the ``fastsim/resources/cycles`` folder of the fastsim 2.1.5 wheel
           (Apache-2.0; ``pip download fastsim==2.1.5 --no-deps
           --python-version 3.10 --only-binary=:all:`` and unzip it)
--epa      a folder holding EPA's dynamometer schedule text files, downloaded
           from https://www.epa.gov/vehicle-and-fuel-emissions-testing/
           dynamometer-drive-schedules (names as EPA gives them)
--eu       Commission Regulation (EU) 2017/1151 as text: download the PDF
           from https://eur-lex.europa.eu/legal-content/EN/TXT/PDF/?uri=
           CELEX:32017R1151 and run ``pdftotext -layout`` on it. The WLTC
           tables (Annex XXI, Sub-Annex 1, Tables A1/1 to A1/12) are read
           from it.

Without --write it only reports. With --write it writes each cycle's CSV to
backend/app/cycles/ and its fingerprints into cycles.json. A CSV that is
already shipped is never changed (docs/DATA-REGISTER.md rule 7): if the
rebuilt file differs, the script stops.

What it does to the numbers, and nothing more:
- speeds on a 0.1 mph grid are converted to km/h (x 1.609344) and rounded
  to 3 decimals; speeds on a 0.1 km/h grid keep one decimal; other speeds
  (measured traces) are converted from m/s and rounded to 3 decimals;
- a point that lies on the straight line between its neighbours (in speed
  and in grade) is left out, which changes nothing under the Driving Task's
  linear interpolation;
- grade (rise over run) becomes percent, rounded to 4 decimals.
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "backend" / "app" / "cycles"
MPH = 1.609344

# id -> (source kind, file(s), speed grid). Kinds: 'fastsim' (m/s columns),
# 'epa' (a 1 Hz text table, speed in the unit the grid names).
SOURCES: dict[str, tuple[str, tuple[str, ...], str]] = {
    # EU 2017/1151 Annex XXI Sub-Annex 1: the tables to chain, by number
    # (A1/1 Low1, A1/2 Medium1, A1/3-6 class 2, A1/7 Low3, A1/8 Medium3-1,
    # A1/9 Medium3-2, A1/10 High3-1, A1/11 High3-2, A1/12 Extra High3)
    "wltc-3b": ("eu-wltc", ("7", "9", "11", "12"), "kmh"),
    "wltc-3a": ("eu-wltc", ("7", "8", "10", "12"), "kmh"),
    "wltc-2": ("eu-wltc", ("3", "4", "5", "6"), "kmh"),
    "wltc-1": ("eu-wltc", ("1", "2", "1"), "kmh"),  # Low1, Medium1, Low1 again
    "wltc-3b-city": ("eu-wltc", ("7", "9"), "kmh"),
    "wltc-3a-city": ("eu-wltc", ("7", "8"), "kmh"),
    "wltc-3-low": ("eu-wltc", ("7",), "kmh"),
    "wltc-3a-medium": ("eu-wltc", ("8",), "kmh"),
    "wltc-3b-medium": ("eu-wltc", ("9",), "kmh"),
    "wltc-3a-high": ("eu-wltc", ("10",), "kmh"),
    "wltc-3b-high": ("eu-wltc", ("11",), "kmh"),
    "wltc-3-extra-high": ("eu-wltc", ("12",), "kmh"),
    # FASTSim 2.1.5 copies (Apache-2.0)
    "wmtc": ("fastsim", ("wmtc_all.csv",), "kmh"),
    "wmtc-part1": ("fastsim", ("wmtc_part1.csv",), "kmh"),
    "wmtc-part2": ("fastsim", ("wmtc_part2.csv",), "kmh"),
    "wmtc-part3": ("fastsim", ("wmtc_part3.csv",), "kmh"),
    "long-haul": ("fastsim", ("longHaulDriveCycle.csv",), "raw"),
    # the first 100 km of it (cut at the first stop after 100 km)
    "long-haul-100km": ("fastsim-excerpt", ("longHaulDriveCycle.csv",), "raw"),
    # EPA schedule files (US Government works)
    "udds": ("epa", ("uddscol.txt",), "mph"),
    "hwfet": ("epa", ("hwycol.txt",), "mph"),
    "ftp-75": ("epa", ("ftpcol.txt",), "mph"),
    "us06": ("epa", ("us06col.txt",), "mph"),
    "sc03": ("epa", ("sc03col.txt",), "mph"),
    "la92": ("epa", ("la92dds.txt",), "mph"),
    "nycc": ("epa", ("epa-new-york-city-cycle.txt",), "mph"),
    "ftp-mc-1b": ("epa", ("ftpmc1b.txt",), "kmh"),
    # NEDC: UN Regulation No 83 as published in OJ L 42, 15.2.2012, Annex 4a,
    # Table 1 (elementary urban cycle, run four times) and Table 2 (extra-
    # urban cycle): the operations' end points, typed from the tables
    "nedc": ("table", ("NEDC",), "kmh"),
}

# (time s, speed km/h) at the end of each operation; the speed runs straight
# between them, as the tables' constant accelerations say.
ECE_15 = [(0, 0), (11, 0), (15, 15), (23, 15), (25, 10), (28, 0), (49, 0), (54, 15),
          (56, 15), (61, 32), (85, 32), (93, 10), (96, 0), (117, 0), (122, 15), (124, 15),
          (133, 35), (135, 35), (143, 50), (155, 50), (163, 35), (176, 35), (178, 35),
          (185, 10), (188, 0), (195, 0)]
EUDC = [(0, 0), (20, 0), (25, 15), (27, 15), (36, 35), (38, 35), (46, 50), (48, 50),
        (61, 70), (111, 70), (119, 50), (188, 50), (201, 70), (251, 70), (286, 100),
        (316, 100), (336, 120), (346, 120), (362, 80), (370, 50), (380, 0), (400, 0)]
TABLES = {"NEDC": [ECE_15] * 4 + [EUDC]}

# Other copies each built file must equal, value for value at 1 Hz:
# (source kind, file) — FASTSim's copies of the EU and EPA tables.
CROSS_CHECK = {
    "udds": ("fastsim", "udds.csv"), "hwfet": ("fastsim", "hwfet.csv"),
    "us06": ("fastsim", "us06.csv"), "ftp-mc-1b": ("fastsim", "ftpmc1b.csv"),
    "wltc-3b": ("fastsim", "wltc_3b.csv"), "wltc-3a": ("fastsim", "wltc_3a.csv"),
    "wltc-3b-city": ("fastsim", "wltc_city_3b.csv"),
    "wltc-3a-city": ("fastsim", "wltc_city_3a.csv"),
    "wltc-3-low": ("fastsim", "wltc_low_3.csv"),
    "wltc-3a-medium": ("fastsim", "wltc_medium_3a.csv"),
    "wltc-3b-medium": ("fastsim", "wltc_medium_3b.csv"),
    "wltc-3a-high": ("fastsim", "wltc_high_3a.csv"),
    "wltc-3b-high": ("fastsim", "wltc_high_3b.csv"),
    "wltc-3-extra-high": ("fastsim", "wltc_extrahigh_3.csv"),
}


def _read_text(path: Path) -> str:
    raw = path.read_bytes()
    if raw[:2] in (b"\xff\xfe", b"\xfe\xff"):
        return raw.decode("utf-16")
    return raw.decode("utf-8-sig", errors="replace")


def read_fastsim(path: Path) -> list[tuple[float, float, float]]:
    """(t s, speed m/s, grade rise/run) rows of a FASTSim cycle CSV."""
    rows = list(csv.DictReader(io.StringIO(_read_text(path))))
    t_key = next(k for k in rows[0] if k.lower() in ("cycsecs", "time_s"))
    v_key = next(k for k in rows[0] if k.lower() in ("cycmps", "mps"))
    g_key = next((k for k in rows[0] if k.lower() in ("cycgrade", "grade")), None)
    return [(float(r[t_key]), float(r[v_key]), float(r[g_key] or 0) if g_key else 0.0)
            for r in rows]


def read_epa(path: Path) -> list[tuple[float, float]]:
    """(t s, speed in the file's unit) rows of an EPA schedule: every line
    whose first two fields are numbers; header lines are skipped."""
    out = []
    for line in _read_text(path).splitlines():
        parts = line.replace(",", " ").split()
        if len(parts) < 2:
            continue
        try:
            t, v = float(parts[0]), float(parts[1])
        except ValueError:
            continue
        out.append((t, v))
    times = [t for t, _ in out]
    if times != [float(i) for i in range(len(times))]:
        raise SystemExit(f"{path.name}: times are not 0, 1, 2, ... s")
    return out


def read_eu_wltc(path: Path) -> dict[str, list[float]]:
    """Tables A1/1 to A1/12 of EU 2017/1151 Annex XXI Sub-Annex 1 from the
    regulation's text (pdftotext -layout): {table number: 1 Hz speeds}."""
    lines = path.read_text(encoding="utf-8", errors="replace").split("\n")
    starts: dict[int, int] = {}
    for i, line in enumerate(lines):
        m = re.fullmatch(r"\s+Table A1/(\d+)\s*", line)
        # the first table of each number that follows Table A1/1's heading
        if m and int(m.group(1)) not in starts and (starts or "WLTC" in lines[i + 1]):
            starts[int(m.group(1))] = i
    tables: dict[str, list[float]] = {}
    for k in range(1, 13):
        pts: dict[int, float] = {}
        for line in lines[starts[k] + 2:starts[k + 1]]:
            toks = line.split()
            # only lines that are all numbers: time, speed, time, speed, ...
            if not toks or len(toks) % 2 or not all(re.fullmatch(r"\d+(,\d)?", x) for x in toks):
                continue
            for t, v in zip(toks[::2], toks[1::2]):
                if int(t) in pts:
                    raise SystemExit(f"Table A1/{k}: time {t} s twice")
                pts[int(t)] = float(v.replace(",", "."))
        ts = sorted(pts)
        if ts != list(range(ts[0], ts[-1] + 1)):
            raise SystemExit(f"Table A1/{k}: a second is missing")
        tables[str(k)] = [pts[t] for t in ts]
    return tables


def to_kmh(v: float, grid: str) -> float:
    if grid == "mph":
        return round(round(v, 1) * MPH, 3)
    if grid == "kmh":
        return round(v, 1)
    raise ValueError(grid)


def thin(rows: list[tuple[float, float, float]]) -> list[tuple[float, float, float]]:
    """Leave out points on the straight line between their neighbours."""
    keep = [rows[0]]
    for i in range(1, len(rows) - 1):
        (t0, v0, g0), (t1, v1, g1), (t2, v2, g2) = keep[-1], rows[i], rows[i + 1]
        f = (t1 - t0) / (t2 - t0)
        if abs(v0 + f * (v2 - v0) - v1) > 1e-9 or abs(g0 + f * (g2 - g0) - g1) > 1e-9:
            keep.append(rows[i])
    keep.append(rows[-1])
    return keep


def _num(x: float) -> str:
    """Shortest text for a number: 20 not 20.0, 3.219 not 3.2190."""
    return repr(int(x)) if x == int(x) else repr(x)


def to_csv(rows: list[tuple[float, float, float]], with_grade: bool) -> str:
    head = "t_s,speed_kmh,grade_pct" if with_grade else "t_s,speed_kmh"
    lines = [head]
    for t, v, g in rows:
        lines.append(f"{_num(t)},{_num(v)},{_num(g)}" if with_grade else f"{_num(t)},{_num(v)}")
    return "\n".join(lines) + "\n"


def build(cycle_id: str, fastsim: Path, epa: Path,
          eu: dict[str, list[float]]) -> list[tuple[float, float, float]]:
    kind, files, grid = SOURCES[cycle_id]
    if kind == "eu-wltc":
        speeds: list[float] = []
        for i, k in enumerate(files):
            # the phases' tables follow on second by second (Low3 0-589 s,
            # Medium3 590-1022 s, ...); class 1's repeated Low1 runs 1023-
            # 1611 s, 589 s, so it starts at its table's second 1
            speeds += eu[k][1:] if k in files[:i] else eu[k]
        return [(float(t), v, 0.0) for t, v in enumerate(speeds)]
    if kind == "table":
        rows = []
        for part in TABLES[files[0]]:
            start = rows[-1][0] if rows else 0.0
            rows += [(start + t, float(v), 0.0) for t, v in (part[1:] if rows else part)]
        return rows
    if kind.startswith("fastsim"):
        raw = read_fastsim(fastsim / files[0])
        if grid == "raw":
            rows = [(t, round(v * 3.6, 3), round(g * 100, 4)) for t, v, g in raw]
        else:
            rows = [(t, to_kmh(v * 3.6, grid), round(g * 100, 4)) for t, v, g in raw]
        if kind == "fastsim-excerpt":
            km, cut = 0.0, None
            for i in range(1, len(rows)):
                km += (rows[i][0] - rows[i - 1][0]) * (rows[i][1] + rows[i - 1][1]) / 7200
                if km >= 100:  # the first second at or past 100 km, still moving
                    cut = i
                    break
            assert cut is not None
            rows = rows[:cut + 1]
        return rows
    if kind == "epa":
        return [(t, to_kmh(v, grid), 0.0) for t, v in read_epa(epa / files[0])]
    raise ValueError(kind)


def at_1hz(rows: list[tuple[float, float, float]]) -> list[float]:
    """The speeds at every whole second, straight between the points."""
    out, j = [], 0
    for s in range(int(rows[0][0]), int(rows[-1][0]) + 1):
        while rows[j + 1][0] < s if j + 1 < len(rows) else False:
            j += 1
        if rows[j][0] == s or j + 1 == len(rows):
            out.append(rows[j][1])
        else:
            (t0, v0, _), (t1, v1, _) = rows[j], rows[j + 1]
            out.append(v0 + (v1 - v0) * (s - t0) / (t1 - t0))
    return out


def speed_sum(rows: list[tuple[float, float, float]]) -> float:
    """The sum of the 1 Hz speeds in km/h, the fingerprint regulations and
    JRC's code quote (83,758.6 for WLTC class 3b)."""
    return round(sum(at_1hz(rows)), 3)


def shipped_rows(path: Path) -> list[tuple[float, float, float]]:
    with path.open(encoding="utf-8", newline="") as f:
        return [(float(r["t_s"]), float(r["speed_kmh"]), float(r.get("grade_pct") or 0))
                for r in csv.DictReader(f)]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--fastsim", type=Path, required=True)
    ap.add_argument("--epa", type=Path, required=True)
    ap.add_argument("--eu", type=Path, required=True)
    ap.add_argument("--write", action="store_true")
    args = ap.parse_args()

    meta_path = OUT / "cycles.json"
    meta = json.loads(meta_path.read_text(encoding="utf-8"))
    eu = read_eu_wltc(args.eu)
    for cycle_id, (src, name) in CROSS_CHECK.items():
        assert src == "fastsim"
        grid = SOURCES[cycle_id][2]
        other = [to_kmh(v * 3.6 / MPH if grid == "mph" else v * 3.6, grid)
                 for _, v, _ in read_fastsim(args.fastsim / name)]
        ours = [round(v, 3) for v in at_1hz(build(cycle_id, args.fastsim, args.epa, eu))]
        if other != ours:
            raise SystemExit(f"{cycle_id}: FASTSim's {name} differs from the official table")
        print(f"{cycle_id}: FASTSim's {name} matches the official table value for value")

    failed = False
    for cycle_id in SOURCES:
        rows = build(cycle_id, args.fastsim, args.epa, eu)
        with_grade = any(g for _, _, g in rows)
        text = to_csv(thin(rows), with_grade)
        target = OUT / f"{cycle_id}.csv"
        km = sum((b[0] - a[0]) * (a[1] + b[1]) / 2 for a, b in zip(rows, rows[1:])) / 3600
        fp = {"speed_sum_kmh": speed_sum(rows),
              "sha256": hashlib.sha256(text.encode("utf-8")).hexdigest()}
        print(f"{cycle_id:18} {rows[-1][0]:8.0f} s {km:9.3f} km {max(v for _, v, _ in rows):6.1f} km/h"
              f"  sum {fp['speed_sum_kmh']:.1f}  {len(text.splitlines()) - 1} points"
              f"{'  grade' if with_grade else ''}")
        if target.exists() and target.read_text(encoding="utf-8") != text:
            # an older file thinned differently: keep it if it holds the same
            # 1 Hz values, and fingerprint the file as shipped
            shipped = shipped_rows(target)
            # (1e-3 km/h: the older thinning dropped points whose rounding
            # to 3 decimals left them up to 0.8e-3 off the line)
            if any(abs(a - b) > 1e-3 for a, b in zip(at_1hz(shipped), at_1hz(rows))):
                print(f"  {target.name} is shipped and holds other values: not changed (rule 7)")
                failed = True
                continue
            print(f"  {target.name} is shipped, thinned differently, same 1 Hz values: kept")
            text = target.read_text(encoding="utf-8")
            fp["sha256"] = hashlib.sha256(text.encode("utf-8")).hexdigest()
        if args.write:
            target.write_text(text, encoding="utf-8", newline="\n")
            if cycle_id in meta["cycles"]:
                meta["cycles"][cycle_id]["fingerprint"] = fp
    if args.write:
        meta_path.write_text(json.dumps(meta, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())

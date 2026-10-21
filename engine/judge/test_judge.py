#!/usr/bin/env python3
"""The judge's own scoring, on made-up series whose right scores are known
(standard library only; `engine/check.sh` runs it):

* OpenModelica's rows at an event a little after a grid time are no grid
  points, and two rows at one grid time are an event there;
* Stair is compared with its CSV by time, and a run that lacks one of the
  CSV's times is not scored;
* a pendulum is compared with its reference by time, a missing point
  counted, not shifting the rest;
* a closed-form run is right within RIGHT rtol, a count when it fired
  once per crossing;
* MSL's planar four-bar is compared with its reference by time, MSL's
  own result beside it at the grid's times.
"""
from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import judge  # noqa: E402

DT = 0.2


def stair_csv(folder: Path) -> Path:
    """Stair_out.csv as the Reference FMUs ship it: the counter at k 0.2 s
    (its float noise included), the value after the event at whole seconds."""
    rows = ["time,counter"]
    for k in range(46):
        rows.append(f"{k * DT!r},{1 + int(k * DT + 1e-9)}")
    path = folder / "reference-fmus" / "Stair" / "Stair_out.csv"
    path.parent.mkdir(parents=True)
    path.write_text("\n".join(rows) + "\n")
    return path


class OpenModelicaRows(unittest.TestCase):
    def test_event_rows_after_a_grid_time_are_not_grid_points(self):
        # each second: the grid row (before the event), then the event's two
        # rows 9e-11 s later, as OpenModelica writes them at rtol 1e-7
        lines = ["time,counter"]
        for k in range(46):
            t = k * DT
            whole = abs(t - round(t)) < 1e-9 and k > 0
            lines.append(f"{t!r},{1 + int(t + 1e-9) - (1 if whole else 0)}")
            if whole:
                lines.append(f"{t + 9e-11!r},{int(t + 1e-9)}")
                lines.append(f"{t + 9e-11!r},{1 + int(t + 1e-9)}")
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "out.csv"
            path.write_text("\n".join(lines) + "\n")
            s = judge.omc_csv(path, ["counter"], DT)
        self.assertEqual(len(s["times"]), 46)
        for k, t in enumerate(s["times"]):
            self.assertAlmostEqual(t, k * DT, delta=1e-12)
        self.assertEqual(len(s["event_times"]), 9)
        self.assertEqual(s["changes"]["counter"][0], 1.0 + 9e-11)

    def test_two_rows_at_one_grid_time_are_an_event_there(self):
        lines = ["time,x", "0,0", "0.2,1", "0.2,2", "0.4,2"]
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "out.csv"
            path.write_text("\n".join(lines) + "\n")
            s = judge.omc_csv(path, ["x"], DT)
        self.assertEqual(s["channels"]["x"], [0.0, 2.0, 2.0])
        self.assertEqual(s["before"]["x"], [(1, 1.0)])

    def test_a_grid_time_without_a_row_is_missing_not_filled(self):
        lines = ["time,x", "0,0", "0.2,1", "0.31,1.5", "0.6,3"]
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "out.csv"
            path.write_text("\n".join(lines) + "\n")
            s = judge.omc_csv(path, ["x"], DT)
        self.assertEqual(len(s["times"]), 3)
        self.assertAlmostEqual(s["times"][2], 0.6, delta=1e-12)


class Stair(unittest.TestCase):
    def run_of(self, times, counter, events):
        return {"series": {"times": times, "channels": {"counter": counter}},
                "event_times": events}

    def test_compared_by_time(self):
        case = {"dt": DT, "judge": {"csv": "reference-fmus/Stair/Stair_out.csv"}}
        with tempfile.TemporaryDirectory() as d, \
                mock.patch.object(judge, "cached", lambda cache, rel: Path(d) / rel):
            stair_csv(Path(d))
            times = [k * DT for k in range(46)]
            exact = [1 + int(t + 1e-9) for t in times]
            # one off the grid in front: by position everything after it
            # would be compared with the wrong row
            shifted = [0.1] + times
            got = self.run_of(shifted, [1] + exact, [float(k) for k in range(1, 10)])
            self.assertEqual(judge.score_stair(case, Path(d), got)["error"], 0)
            # the pre-event value at each whole second: off by one there
            late = [c - (1 if abs(t - round(t)) < 1e-9 and t > 0 else 0)
                    for t, c in zip(times, exact)]
            got = self.run_of(times, late, [k + 1e-10 for k in range(1, 10)])
            self.assertEqual(judge.score_stair(case, Path(d), got)["error"], 1)
            # a run without the last of the CSV's times is not scored
            got = self.run_of(times[:-1], exact[:-1], [float(k) for k in range(1, 10)])
            with self.assertRaises(ValueError):
                judge.score_stair(case, Path(d), got)


class Pendulum(unittest.TestCase):
    def test_compared_by_time_with_a_point_missing(self):
        n = 11
        ref = {"x": [0.1 * k for k in range(n)], "y": [-1.0] * n,
               "vx": [0.0] * n, "vy": [0.0] * n}
        keep = [k for k in range(n) if k != 4]
        r = {"series": {"times": [k * 0.1 for k in keep], "channels": {
            "x": [ref["x"][k] for k in keep], "y": [-1.0] * len(keep),
            "vx": [0.0] * len(keep), "vy": [0.0] * len(keep)}}}
        case = {"dt": 0.1, "judge": {"m": 1.0, "g": 9.81, "L": 1.0}}
        s = judge.score_pendulum(case, ref, r)
        self.assertEqual(s["error"], 0.0)
        self.assertEqual(s["missing"], 1)


class PlanarFourbar(unittest.TestCase):
    def test_compared_by_time_with_msl_s_result_beside_it(self):
        n = 11
        ref = {"phi1": [-0.1 * k for k in range(n)], "w1": [-1.0] * n}
        # the run: a point off the grid in front, phi1 2e-9 off at k = 7
        times = [0.05] + [k * 0.1 for k in range(n)]
        phi = [-0.005] + [ref["phi1"][k] + (2e-9 if k == 7 else 0.0) for k in range(n)]
        r = {"series": {"times": times, "channels": {"phi1": phi, "w1": [-1.0] * (n + 1)}}}
        case = {"dt": 0.1, "judge": {"kind": "planar", "csv": "msl/PlanarFourbar/PlanarFourbar.csv"}}
        # MSL's result at every 0.05 s (as its CSV has it at every 1 ms),
        # 3e-4 off at 0.6 s and further off at 0.65 s, off the grid
        rows = ['"time","revolute1.phi","revolute1.w"']
        for k in range(2 * n - 1):
            t = 0.05 * k
            off = 3e-4 if k == 12 else 1e-2 if k == 13 else 0.0
            rows.append(f"{t!r},{-t + off!r},-1")
        with tempfile.TemporaryDirectory() as d, \
                mock.patch.object(judge, "cached", lambda cache, rel: Path(d) / rel):
            path = Path(d) / case["judge"]["csv"]
            path.parent.mkdir(parents=True)
            path.write_text("\n".join(rows) + "\n")
            s = judge.score_planar(case, Path(d), ref, r)
        self.assertAlmostEqual(s["error"], 2e-9, delta=1e-15)
        self.assertAlmostEqual(s["msl_error"], 3e-4, delta=1e-12)
        self.assertEqual(s["missing"], 0)
        self.assertEqual(judge.verdict(case, dict(s, ok=True, rtol=1e-10)), "right")


class Verdict(unittest.TestCase):
    def test_right_within_right_rtol(self):
        case = {"judge": {"kind": "pendulum"}}
        self.assertEqual(judge.RIGHT, 100.0)
        self.assertEqual(judge.verdict(case, {"ok": True, "rtol": 1e-10, "error": 2.5e-9}),
                         "right")
        # 0.72 rad at rtol 1e-10: 1.1e-8 m, beyond the brief's 1e-8
        self.assertEqual(judge.verdict(case, {"ok": True, "rtol": 1e-10, "error": 1.1e-8}),
                         "wrong")
        # 1.40 rad at rtol 1e-4: 1.1e-2 m, beyond 1 cm
        self.assertEqual(judge.verdict(case, {"ok": True, "rtol": 1e-4, "error": 1.1e-2}),
                         "wrong")
        self.assertEqual(judge.verdict(case, {"ok": False, "rtol": 1e-4}), "stopped")
        self.assertIsNone(judge.verdict({"judge": {"kind": "testset"}},
                                        {"ok": True, "rtol": 1e-4, "error": 1.0}))

    def test_a_count_is_right_when_it_fired_once_per_crossing(self):
        case = {"t_end": 2.0, "judge": {"kind": "count", "exact": [0.625]}}
        once = {"final": {"n": 1.0}, "event_times": [0.62499996],
                "event_labels": ["the model: it counts"]}
        twice = {"final": {"n": 2.0}, "event_times": [0.62499996, 0.625],
                 "event_labels": ["the model: it counts"] * 2}
        a = judge.score_count(case, once)
        self.assertAlmostEqual(a["error"], 4e-8, delta=1e-15)
        self.assertEqual(judge.verdict(case, dict(a, ok=True, rtol=1e-7)), "right")
        b = judge.score_count(case, twice)
        self.assertEqual(judge.verdict(case, dict(b, ok=True, rtol=1e-7)), "wrong")


if __name__ == "__main__":
    unittest.main()

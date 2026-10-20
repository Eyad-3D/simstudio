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
* MSL's reference result is compared at its own times, and a limit's
  instants are found between the output points;
* a driven trace is judged by 40 CFR 1066.425(b): its band from the
  schedule within 1 s, 2 s outside allowed, a shortfall at full throttle
  excused, too fast at full brake the car's.
"""
from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import driver  # noqa: E402
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


class MslReference(unittest.TestCase):
    def test_compared_at_its_own_times(self):
        # the reference: x = t at every 0.1 s, and an event row at 0.25 s
        lines = ['"time","x"'] + [f"{0.1 * k!r},{0.1 * k!r}" for k in range(3)]
        lines += ["0.25,0.25", "0.25,0.25"] + [f"{0.1 * k!r},{0.1 * k!r}" for k in range(3, 5)]
        case = {"dt": 0.1, "judge": {"csv": "ref.csv", "signals": {"x": ["m.x", 2.0]},
                                     "limit": {"m.u": 0.5, "u": 0.5}, "time_events": []}}
        r = {"series": {"times": [0.1 * k for k in range(5)],
                        "channels": {"m.x": [0.05 * k for k in range(4)] + [0.21],
                                     "m.u": [0.0, 0.2, 0.4, 0.6, 0.8]}}}
        with tempfile.TemporaryDirectory() as d, \
                mock.patch.object(judge, "cached", lambda cache, rel: Path(d) / rel):
            (Path(d) / "ref.csv").write_text("\n".join(lines) + "\n")
            s = judge.score_msl(case, Path(d), r)
            # 2 · 0.21 = 0.42 against 0.4: 0.02 of the peak 0.4
            self.assertAlmostEqual(s["error"], 0.05, delta=1e-12)
            # u crosses 0.5 halfway between 0.2 and 0.3 s; the reference's
            # event row at 0.25 s is its one state event
            self.assertAlmostEqual(s["limit_times"][0], 0.25, delta=1e-12)
            self.assertEqual(s["limit_times_reference"], [0.25])
            # another tool's run of the original, by the original's names:
            # x off by 0.01 at 0.1 s, u crossing 0.02 s later
            other = {"series": {"times": [0.1 * k for k in range(5)],
                                "channels": {"x": [0.0, 0.11, 0.2, 0.3, 0.42],
                                             "u": [0.0, 0.2, 0.3, 0.6, 0.8]}}}
            p = judge.beside_msl(case, Path(d), r, other)
            self.assertEqual(p["points"], 5)
            self.assertAlmostEqual(p["signals"]["x"], 0.01 / 0.4, delta=1e-12)
            self.assertAlmostEqual(p["limit_time_difference"], 0.0166666666667, delta=1e-9)
            r["series"]["times"][2] = 0.21  # not one of the reference's times
            with self.assertRaises(ValueError):
                judge.score_msl(case, Path(d), r)


def trace(n, dt=0.1, v=50.0):
    return {"speed": [v] * n, "target": [v] * n, "accel": [0.3] * n, "brake": [0.0] * n}


def judged(t, dt=0.1):
    return driver.score_trace(dt, t["speed"], t["target"], t["accel"], t["brake"])


class SpeedTrace(unittest.TestCase):
    def test_on_the_schedule_and_within_its_band(self):
        t = trace(100)
        self.assertEqual(judged(t)["verdict"], "pass")
        # 2 mi/h = 3.218688 km/h: just inside, and lagging a ramp by 1 s
        t["speed"] = [50.0 - 3.2186] * 100
        self.assertEqual(judged(t)["verdict"], "pass")
        t = trace(100)
        t["target"] = [10.0 + 0.1 * k for k in range(100)]  # 1 km/h each second
        t["speed"] = [10.0 + 0.1 * max(k - 10, 0) - 3.2 for k in range(100)]
        self.assertEqual(judged(t)["occasions"], 0)

    def test_two_seconds_outside_allowed_and_no_more(self):
        t = trace(100)
        for k in range(20, 40):  # 20 points: 2.0 s
            t["speed"][k] = 45.0
        self.assertEqual(judged(t)["verdict"], "pass")
        t["speed"][40] = 45.0  # 2.1 s
        j = judged(t)
        self.assertEqual(j["verdict"], "driver")
        self.assertEqual(j["counted"], [{"side": "under", "t_start": 2.0, "length_s": 2.1,
                                         "kind": "driver"}])

    def test_behind_at_full_throttle_is_excused(self):
        t = trace(200)
        for k in range(20, 150):
            t["speed"][k], t["accel"][k] = 30.0, 1.0
        j = judged(t)
        self.assertEqual(j["verdict"], "pass")
        self.assertAlmostEqual(j["excused_s"], 13.0, delta=1e-9)
        # the pedal lifted before the car reached the schedule
        for k in range(100, 150):
            t["accel"][k] = 0.99
        self.assertEqual(judged(t)["verdict"], "driver")

    def test_too_fast_at_full_brake_is_the_cars(self):
        t = trace(100)
        for k in range(10, 60):
            t["speed"][k], t["brake"][k] = 55.0, 1.0
        self.assertEqual(judged(t)["verdict"], "brakes")
        t["brake"][30] = 0.5
        self.assertEqual(judged(t)["verdict"], "driver")


if __name__ == "__main__":
    unittest.main()

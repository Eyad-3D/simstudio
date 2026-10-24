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
* MSL's planar four-bar is compared with its reference by time, MSL's
  own result beside it at the grid's times.
* the tables read at a state (`kink-…`): the interpolants as the engine
  builds them (Steffen's slopes, the pieces a linear table continues
  outside), each channel's error relative to its largest value, the
  summaries beside the located controls and the other ways, and (where
  scipy is there) the cell-by-cell reference of the swinging gear against
  its closed form.
"""
from __future__ import annotations

import importlib.util
import math
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


class Positions(unittest.TestCase):
    def test_each_rod_compared_by_time_the_worst_judged(self):
        # two rods' x, y on a 0.2 s grid; the run's rows by time, one
        # extra row at an event between grid times
        case = {"dt": DT, "record": ["x0", "y0", "x1", "y1"],
                "judge": {"kind": "pendulums", "angles": [1.5, 1.8]}}
        ref = {n: [0.0, 1.0, 2.0] for n in case["record"]}
        r = {"series": {"times": [0.0, 0.2, 0.3, 0.4],
                        "channels": {"x0": [0.0, 1.0 + 1e-9, 9.0, 2.0],
                                     "y0": [0.0, 1.0, 9.0, 2.0 - 2e-9],
                                     "x1": [0.0, 1.0, 9.0, 2.0 + 5e-7],
                                     "y1": [0.0, 1.0, 9.0, 2.0]}}}
        s = judge.score_positions(case, ref, r, case["record"])
        self.assertEqual(s["missing"], 0)
        self.assertEqual(len(s["per_rod"]), 2)
        self.assertAlmostEqual(s["per_rod"][0], 2e-9, delta=1e-15)
        self.assertAlmostEqual(s["error"], 5e-7, delta=1e-15)
        self.assertAlmostEqual(s["rod_min"], 2e-9, delta=1e-15)
        self.assertEqual(judge.verdict(case, dict(s, ok=True, rtol=1e-8)), "right")
        self.assertEqual(judge.verdict(case, dict(s, ok=True, rtol=1e-9)), "wrong")
        self.assertEqual(judge.verdict({"judge": {"kind": "double"}},
                                       dict(s, ok=True, rtol=1e-9)), "wrong")


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

    def test_standing_at_full_throttle_is_told_apart(self):
        t = trace(300)
        for k in range(50, 300):  # behind at full throttle, standing from 10 s
            t["accel"][k] = 1.0
            t["speed"][k] = max(0.0, 50.0 - 5.0 * (k - 50))
        j = judged(t)
        self.assertEqual(j["verdict"], "stood")
        self.assertAlmostEqual(j["stood_s"], 24.0, delta=1e-9)
        t["speed"][150:] = [0.6] * 150  # crawling, not standing
        self.assertEqual(judged(t)["verdict"], "pass")

    def test_too_fast_at_full_brake_is_the_cars(self):
        t = trace(100)
        for k in range(10, 60):
            t["speed"][k], t["brake"][k] = 55.0, 1.0
        self.assertEqual(judged(t)["verdict"], "brakes")
        t["brake"][30] = 0.5
        self.assertEqual(judged(t)["verdict"], "driver")



def piece_value(pieces, x):
    """A table's function from its pieces (judge.pieces_1d) at x."""
    lo, hi, x0, c = pieces[judge.kink_piece(pieces, x)]
    return sum(ck * (x - x0) ** n for n, ck in enumerate(c))


class KinkCases(unittest.TestCase):
    """The tables read at a state (`kink-…`): the interpolants as the
    engine builds them, and their scoring beside the located controls."""

    def test_steffen_slopes_are_exact_on_a_parabola(self):
        # interior and end slopes of x**2 at 0, 1, 2, as Steffen's
        # (lsim_ir::interp::steffen_slopes) give them
        self.assertEqual(judge.steffen([0.0, 1.0, 2.0], [0.0, 1.0, 4.0]), [0.0, 2.0, 4.0])
        # two points: the secant
        self.assertEqual(judge.steffen([0.0, 2.0], [1.0, 5.0]), [2.0, 2.0])

    def test_pieces_are_the_tables_function(self):
        xs, ys = [-1.0, 0.5, 2.0], [1.0, -0.5, -5.0]
        lin = judge.pieces_1d(xs, ys, "linear", "linear")
        # a linear table continued linearly: its edge cells reach out, no
        # piece (and no kink) at its edges
        self.assertEqual(len(lin), 2)
        self.assertIsNone(lin[0][0])
        self.assertIsNone(lin[-1][1])
        for x, y in zip(xs, ys):
            self.assertAlmostEqual(piece_value(lin, x), y, places=14)
        self.assertAlmostEqual(piece_value(lin, 3.0), -5.0 - 3.0, places=14)
        held = judge.pieces_1d(xs, ys, "linear", "clamp")
        self.assertEqual(len(held), 4)
        self.assertEqual(piece_value(held, -7.0), 1.0)
        self.assertEqual(piece_value(held, 9.0), -5.0)
        cubic = judge.pieces_1d([0.0, 1.0, 2.0], [0.0, 1.0, 4.0], "cubic", "linear")
        for x in (0.0, 0.25, 0.5, 1.0, 1.5, 2.0):
            self.assertAlmostEqual(piece_value(cubic, x), x * x, places=14)
        # outside, the end slope (4 at 2)
        self.assertAlmostEqual(piece_value(cubic, 3.0), 8.0, places=14)

    def test_the_located_control_branches_on_the_breakpoints(self):
        lin = judge.pieces_1d([-1.0, 0.5, 2.0], [1.0, -0.5, -5.0], "linear", "linear")
        text = judge.if_chain(lin, "x")
        self.assertEqual(text.count("if "), 1)
        self.assertIn("x < 0.5", text)

    def test_a_kink_run_is_scored_relative_to_each_channels_largest_value(self):
        case = {"dt": 1.0, "record": ["x", "v"], "judge": {"kind": "kink"}}
        ref = {"x": [1.0, 2.0, 4.0], "v": [0.0, -10.0, 5.0], "crossings": [[0.5, 0, 1.5, 1.0]]}
        run = {"series": {"times": [0.0, 1.0, 1.5, 2.0],
                          "channels": {"x": [1.0, 2.0, 9.0, 4.04], "v": [0.0, -10.05, 0.0, 5.0]}}}
        got = judge.score_kink(case, ref, run)
        # x: 0.04 of 4 (the row at 1.5 s is no grid point); v: 0.05 of 10
        self.assertAlmostEqual(got["per_channel"]["x"], 0.01)
        self.assertAlmostEqual(got["per_channel"]["v"], 0.005)
        self.assertAlmostEqual(got["error"], 0.01)
        self.assertEqual(got["crossings"], 1)
        self.assertEqual(judge.verdict(case, {"ok": True, "error": 0.01, "rtol": 1e-4}), "right")
        self.assertEqual(judge.verdict(case, {"ok": True, "error": 0.02, "rtol": 1e-4}), "wrong")

    def test_the_summary_puts_each_case_beside_its_control(self):
        row = lambda e, steps: {"rtol": 1e-6, "ok": True, "error": e, "steps": steps,
                                "err_test_fails": 1, "crossings": 3}
        after = {"kink-a": {"rows": [row(4e-6, 10)]}, "kink-a-located": {"rows": [row(2e-6, 12)]}}
        md = judge.kink_summary(after)
        self.assertIn("| kink-a | 1e-06 | 4.00 | 2.00 | 2.00 | 10 | 12 | 1 | 1 | 3 |", md)

    def test_the_other_summary_puts_each_case_beside_its_other_way(self):
        row = lambda e, steps, events: {"rtol": 1e-6, "ok": True, "error": e, "steps": steps,
                                        "events": events, "crossings": 3}
        after = {"kink-b": {"rows": [row(2e-6, 10, 0)]}, "kink-b-held": {"rows": [row(8e-6, 30, 3)]},
                 "kink-c": {"rows": [row(1e-6, 20, 6)]}, "kink-c-over": {"rows": [row(5e-5, 15, 0)]}}
        md = judge.kink_beside_summary(after)
        # the other way's error over the engine's
        self.assertIn("| kink-b | held | 1e-06 | 2.00 | 8.00 | 4.00 | 10 | 30 | 0 | 3 | 3 |", md)
        self.assertIn("| kink-c | over | 1e-06 | 1.00 | 50.00 | 50.00 | 20 | 15 | 6 | 0 | 3 |", md)
        self.assertEqual(judge.kink_beside_summary({"kink-b": {"rows": []}}), "")
        # held without the releases beside the engine's way too
        after["kink-b-always"] = {"rows": [row(4e-5, 60, 12)]}
        md = judge.kink_beside_summary(after)
        self.assertIn("| kink-b | always | 1e-06 | 2.00 | 40.00 | 20.00 | 10 | 60 | 0 | 12 | 3 |", md)

    @unittest.skipUnless(importlib.util.find_spec("scipy"), "the references need scipy")
    def test_the_cell_by_cell_reference_of_a_swinging_gear(self):
        # a constant ratio (two equal points, continued linearly): r' = 0,
        # the load a harmonic oscillator, phi = (w0 / om) sin(om t),
        # om^2 = k / (J2 + J1 r^2)
        pieces = judge.pieces_1d([-40.0, 40.0], [2.5, 2.5], "linear", "linear")
        j = {"pieces": pieces, "J1": 0.05, "J2": 2.0, "k": 50.0, "w0": 28.0}
        times = [0.01 * k for k in range(301)]
        r = judge.kink_osc_reference(j, times)
        om = math.sqrt(50.0 / (2.0 + 0.05 * 2.5 ** 2))
        worst = max(abs(p - 28.0 / om * math.sin(om * t)) for p, t in zip(r["phi"], times))
        self.assertLess(worst, 1e-10)
        self.assertEqual(r["crossings"], [])
        self.assertLess(r["check"], 1e-12)
        # a table of two cells: each crossing of its middle point found,
        # both ways, the energy conserved through them
        pieces = judge.pieces_1d([-30.0, 0.0, 30.0], [2.0, 2.5, 2.2], "linear", "linear")
        r = judge.kink_osc_reference(dict(j, pieces=pieces), times)
        self.assertGreater(len(r["crossings"]), 2)
        self.assertEqual({c[3] for c in r["crossings"]}, {1.0, -1.0})
        self.assertLess(r["check"], 1e-11)

    @unittest.skipUnless(importlib.util.find_spec("scipy"), "the references need scipy")
    def test_the_wavy_beads_second_derivative_jumps_and_its_reference_holds(self):
        # Steffen's cubic of x^2/2 + sin(3x)/4: its r'' differs on the two
        # sides of each inner point (the parabola's does not), and the
        # reference by DOP853 agrees with Radau's
        def jumps(pr):
            """r'' on the two sides of each inner point"""
            def r2(p, x):
                _, _, x0, c = pr[p]
                return 2 * c[2] + 6 * c[3] * (x - x0)
            return [abs(r2(p, pr[p][1]) - r2(p + 1, pr[p][1])) for p in range(1, len(pr) - 2)]
        for n in (11, 41):
            text, case = judge.kinks()[f"kink-bead-sin-{n}"]
            self.assertIn("x^2/2 + sin(3x)/4", text)
            self.assertGreater(min(jumps(case["judge"]["pieces_r"])), 1e-3 if n == 11 else 1e-5)
        _, case = judge.kinks()["kink-bead-41"]
        self.assertLess(max(jumps(case["judge"]["pieces_r"])), 1e-12)
        _, case = judge.kinks()["kink-bead-sin-11"]
        times = [0.01 * k for k in range(201)]
        r = judge.kink_bead_reference(case["judge"], times)
        self.assertGreater(len(r["crossings"]), 2)
        self.assertLess(r["check"], 1e-9)

    @unittest.skipUnless(importlib.util.find_spec("scipy"), "the references need scipy")
    def test_the_two_dimensional_gears_reference_per_pair_of_cells(self):
        # a constant ratio (each axis's values equal: both slopes 0): the
        # load accelerates uniformly, w2 = r tau0 t / (J2 + J1 r^2), through
        # every edge of both axes, which are crossings all the same
        flat = judge.kink_gear2d("flat", [0.0, 20.0, 40.0], [2.0, 2.0, 2.0],
                                 [-1.0, 0.7, 4.0], [0.5, 0.5, 0.5], 20.0, 3.0, 0.01, False)[1]
        times = [0.01 * k for k in range(301)]
        r = judge.kink_gear2d_reference(flat["judge"], times)
        w = [2.5 * 20.0 * t / (2.0 + 0.05 * 2.5 ** 2) for t in times]
        self.assertLess(max(abs(a - b) for a, b in zip(r["w2"], w)), 1e-10)
        self.assertLess(max(abs(a - 2.5 * b) for a, b in zip(r["w1"], w)), 1e-10)
        self.assertEqual(sorted({c[1] for c in r["crossings"]}), [0, 1])
        self.assertLess(r["check"], 1e-12)
        # the judge's case: its slopes jump across both axes' lines, the
        # energy passed on through them; the located run's model reads the
        # same function as if branches
        text, case = judge.kinks()["kink-gear-2d"]
        self.assertIn("  w1 = r(w2, c) * w2", text)
        self.assertIn("stateSelect = StateSelect.always", text)
        located, lcase = judge.kinks()["kink-gear-2d-located"]
        self.assertNotIn("r(w2, c) *", located)
        self.assertIn("  w1 = ((if (w2 / wu) < 20.0 then", located)
        self.assertEqual(case["judge"], lcase["judge"])
        r = judge.kink_gear2d_reference(case["judge"], times)
        self.assertEqual(sorted({c[1] for c in r["crossings"]}), [0, 1])
        self.assertLess(r["check"], 1e-12)

    def test_the_two_dimensional_swings_are_the_one_dimensional_problems(self):
        # the ratio read as r(w2, c), its values the same along c: the same
        # solution as the 1-D swing's, so the same reference; c a state
        # marked always (the explicit form), the table's values per row
        k = judge.kinks()
        for n in (41, 101):
            text, case = k[f"kink-osc2d-{n}"]
            one_text, one = k[f"kink-osc-{n}"]
            self.assertEqual(case["judge"], one["judge"])
            self.assertEqual(case["model"], "Judge.KinkOsc2D")
            self.assertIn("  w1 = r(w2, c) * w2", text)
            self.assertIn("  t2 = r(w2, c) * t1", text)
            self.assertIn("  der(c) = cu;", text)
            self.assertIn("stateSelect = StateSelect.always", text)
            self.assertNotIn("= r(w2) ", text)
            self.assertIn("x2 = {-1, 0.5, 2}", text)
            rows = text.split("values = [")[1].split("]")[0].split("; ")
            self.assertEqual(len(rows), n, "a row per point of w2")
            self.assertTrue(all(len(set(row.split(", "))) == 1 for row in rows), "the same along c")
            self.assertNotIn("options", case)
            self.assertEqual(k[f"kink-osc2d-{n}-over"][1]["options"], {"table_cells": "off"})
            self.assertEqual(k[f"kink-osc2d-{n}-always"][1]["options"],
                             {"table_cells": "kinks", "table_release": False})

    def test_the_gears_on_the_implicit_form_are_the_same_problems(self):
        # the torques written implicitly: t2 an iteration variable, the
        # same solution, the same reference
        k = judge.kinks()
        for name in ("kink-gear-linear", "kink-dense-gear-41", "kink-osc-41"):
            for v in ("", "-located"):
                text, case = k[f"{name}-imp{v}"]
                self.assertIn("t2 + 1e-6 * t2^3 / tu^2 = ", text)
                self.assertIn('parameter Real tu(unit = "N.m") = 1;', text)
                self.assertNotIn("\n  t2 = ", text)
                self.assertEqual(case["judge"], k[name][1]["judge"])
                self.assertEqual(case["located"], v == "-located")
            self.assertEqual(k[f"{name}-imp-always"][1]["options"],
                             {"table_cells": "jumps", "table_release": False})
        # the plain relation, as kink_gear and kink_osc write it
        with self.assertRaises(ValueError):
            judge.implicit_torques("model M\nend M;\n")


if __name__ == "__main__":
    unittest.main()

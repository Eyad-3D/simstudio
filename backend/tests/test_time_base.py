"""Time base of a run: recorded points carry the time of the state they hold,
the run stops exactly at the case duration, and the live stream starts at
t = 0."""
import pytest
from helpers import el, project, series

from app.solver import simulate


def _cruising_vehicle(duration: float, step: float):
    """A lone vehicle body at 36 km/h with no resistances: it keeps 10 m/s,
    so the distance driven is exactly 10 m/s × t (the run warns that the
    vehicle has no wheels)."""
    elements = [el("veh", "vehicle.body", "Vehicle", initial_speed_kmh=36, cd=0.0)]
    return project(elements, [], [], duration=duration, time_step=step)


@pytest.mark.parametrize("duration,step", [(5, 1.0), (5.5, 1.0), (2, 0.25), (1, 0.3)])
def test_distance_is_v_times_t_at_every_recorded_time(duration, step):
    result = simulate(_cruising_vehicle(duration, step), "case")
    assert result.status != "failed", [m.text for m in result.messages]
    distance = series(result, "veh", "sig_distance")
    assert distance[0] == {"t": 0.0, "value": 0.0}
    for p in distance:
        assert p["value"] == pytest.approx(10.0 * p["t"], rel=1e-9, abs=1e-9), p
    speed = series(result, "veh", "sig_speed")
    assert all(p["value"] == pytest.approx(36.0) for p in speed)


@pytest.mark.parametrize("duration,step,n_points", [(5, 1.0, 6), (5.5, 1.0, 7), (1, 0.3, 5)])
def test_run_stops_exactly_at_the_case_duration(duration, step, n_points):
    result = simulate(_cruising_vehicle(duration, step), "case")
    distance = series(result, "veh", "sig_distance")
    assert len(distance) == n_points
    assert distance[-1]["t"] == duration
    assert distance[-1]["value"] == pytest.approx(10.0 * duration, rel=1e-9)
    summary = {s.label: s.value for s in result.summary}
    assert summary["Simulated duration"] == duration
    assert summary["Distance driven"] == pytest.approx(10.0 * duration / 1000.0, abs=1e-3)


def test_recorded_target_matches_the_profile_at_its_time():
    proj = _cruising_vehicle(10, 1.0)
    proj.systems[0].elements.append(
        el("task", "signal.driving_task", "Task", profile="0:10; 10:110"))
    result = simulate(proj, "case")
    for p in series(result, "task", "sig_demand"):
        assert p["value"] == pytest.approx(10.0 + 10.0 * p["t"]), p


def test_live_stream_starts_with_the_initial_state():
    events = []
    simulate(_cruising_vehicle(3, 1.0), "case", emit=events.append)
    steps = [e for e in events if e["type"] == "step"]
    assert [e["t"] for e in steps] == [0.0, 1.0, 2.0, 3.0]
    assert steps[0]["values"]["veh:sig_distance"] == 0.0
    assert steps[0]["values"]["veh:sig_speed"] == pytest.approx(36.0)
    assert steps[-1]["pct"] == 100.0

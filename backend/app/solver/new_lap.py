"""A lap's speed profile on the new engine (lsim-lap): the case's car as the
engine models it, solved one instant at a time, by today's quasi-steady
method (lapsim's cornering speeds, forward and backward passes).

Stage 2 of the lap port: the profile alone, which Calibrate lap fits. A lap
case's energy pass, its channels and its rows come with stage 3; until then
the new engine refuses lap cases (NEW-LAP).
"""
from __future__ import annotations

from .. import cycles
from ..schemas import Project
from . import lapsim
from .network import build_model


def lap_model(project: Project, case_id: str):
    """The case's lap model (lightsim_engine.LapModel)."""
    import lightsim_engine

    from .new_engine import read_texts  # (new_engine imports this package)
    return lightsim_engine.lap_model(project.model_dump_json(), case_id,
                                     cycles_dir=str(cycles.DIR), texts=read_texts(project))


def track_of(project: Project, overrides: dict, spacing: float = 1.0) -> lapsim.Track:
    """The Race Track's points with ``overrides`` (a case's) applied."""
    model = build_model(project, {}, overrides)
    if model.track is None:
        raise ValueError("A lap needs a Race Track.")
    return lapsim.load_track(model.params_of[model.track], spacing)


def solve(lm, track: lapsim.Track, v_start: float, more: bool = False, coast: float = 0.0,
          stop: bool = False) -> dict:
    """One lap's profile (LapModel.solve) on ``track``; a car that cannot
    drive it is a lapsim.LapError."""
    try:
        return lm.solve(track.kappa, track.sin_t, track.ds, track.closed, v_start, coast=coast,
                        more=more, stop=stop)
    except ValueError as e:
        raise lapsim.LapError(f"The car cannot drive the lap: {e}.") from e

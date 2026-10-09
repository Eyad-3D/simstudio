"""Reference runs of today's engine for the golden comparison (DESIGN.md
12.3): every time-domain case of every example project, at today's solver
step (10 ms) and at a tenth of it (1 ms). The difference between the two
measures today's own numerical error and sets each figure's band.

Run from the repository's ``backend`` folder with its Python:

    python ../engine/crates/lsim-project/golden/reference.py OUT_DIR [project ...]

Writes ``OUT_DIR/<project>__<case>.json``: for each run (``normal``,
``fine``) the summary rows, every recorded channel on the case's grid, and
the parts' energy books. Single runs (no charge balancing), as fast as
possible (``realtimeFactor`` 0). Script blocks with no Sample Time keep
today's 10 ms tick in the fine run too, so both runs drive the same
controller and differ only in the integration.
"""

import copy
import json
import pathlib
import sys
import time

sys.path.insert(0, str(pathlib.Path.cwd()))  # the backend folder: today's engine

from app.schemas import Project  # noqa: E402
from app.solver import core  # noqa: E402
from app.solver.keys import fill_keys  # noqa: E402


def run(project: Project, case_id: str, substep: float) -> dict:
    saved = core.MAX_SUBSTEP
    core.MAX_SUBSTEP = substep
    try:
        t0 = time.perf_counter()
        res = core.run_case(project, case_id)
        wall = time.perf_counter() - t0
    finally:
        core.MAX_SUBSTEP = saved
    fill_keys(res.summary, project)
    channels = {}
    times = None
    for ch in res.channels:
        if times is None:
            times = [p["t"] for p in ch.timeSeries]
        channels[f"{ch.elementId}:{ch.portId}"] = {
            "unit": ch.unit,
            "values": [p["value"] for p in ch.timeSeries],
        }
    return {
        "status": res.status,
        "messages": [m.text for m in res.messages],
        "wall_seconds": wall,
        "summary": [
            {"key": s.key, "label": s.label, "value": s.value, "unit": s.unit, "notValid": s.notValid}
            for s in res.summary
        ],
        "times": times or [],
        "channels": channels,
        "part_energy": [p.model_dump() for p in res.partEnergy],
    }


def main() -> None:
    out = pathlib.Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    wanted = set(sys.argv[2:])
    for path in sorted(pathlib.Path("projects").glob("*.json")):
        if wanted and path.stem not in wanted:
            continue
        raw = json.loads(path.read_text())
        for case in raw.get("cases", []):
            if case.get("kind") == "lap":
                continue
            data = copy.deepcopy(raw)
            for c in data["cases"]:
                c["realtimeFactor"] = 0
            fine = copy.deepcopy(data)
            for sys_ in fine["systems"]:
                for el in sys_["elements"]:
                    if el["componentDefId"] == "signal.script":
                        po = el.setdefault("parameterOverrides", {})
                        if not po.get("sample_time_s"):
                            po["sample_time_s"] = 0.01
            result = {"project": path.stem, "case": case["id"], "name": case.get("name", "")}
            for label, proj, h in (("normal", data, 0.01), ("fine", fine, 0.001)):
                result[label] = run(Project.model_validate(proj), case["id"], h)
                print(f"{path.stem} {case['id']} {label}: {result[label]['status']} "
                      f"in {result[label]['wall_seconds']:.1f} s", flush=True)
            (out / f"{path.stem}__{case['id']}.json").write_text(json.dumps(result))


if __name__ == "__main__":
    main()

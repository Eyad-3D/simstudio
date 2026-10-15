"""Today's gear channels rebuilt from a run's own sources (golden/README.md,
"Every difference outside its band").

Today's engine reports a gear's torque and power as the torque and power
of the motors and clutches above it pushed through the gears on the way
(``through_gears`` and ``book_gears`` in ``domains.py``): each stage passes
on eta x the torque (or power) when the power flows towards the road, 1/eta
x when it flows back, and loses the difference; an open differential gives
each side half of its input torque × ratio × eta (in either direction:
``joint_torque_a``). Locked differentials, transfer cases, several axles
and a motor behind a ratio of its own are not handled. The inertias between the sources and the gear (a motor's
rotor, the gears' own) are not taken out. This script applies that rule to
the channels of a run (today's 1 ms run, as a check that the rule is
today's, and the new engine's run) and compares the result with today's
channels as the golden comparison does (RMS of the difference over every
output point but the first, against the channel's band).

    python definitions.py [--all] REF_DIR OUT_DIR [project[/case] ...]

REF_DIR holds the reference runs (``reference.py``), OUT_DIR the golden
runner's output (its ``<project>__<case>.new.json``: the new run's channels
under today's names, the value before an event at an output time that falls
on one). Prints the channels outside their bands as compared, or every
rebuilt channel with ``--all``. Reads the projects and the library's defaults from the repository's
``backend`` folder. Prints a Markdown table. Standard library only.
"""

import json
import math
import pathlib
import sys

REPO = pathlib.Path(__file__).resolve().parents[4]
BACKEND = REPO / "backend"
GEARS = ("mech.gearbox", "mech.final_drive", "mech.differential")
AXLE_GEARS = ("mech.final_drive", "mech.differential", "mech.transfer_case")
RPM = 60.0 / (2.0 * math.pi)


def rms(xs):
    xs = list(xs)
    return math.sqrt(sum(x * x for x in xs) / len(xs)) if xs else 0.0


def defaults():
    lib = json.loads((BACKEND / "app/library/components.json").read_text())
    return {c["id"]: {p["key"]: p.get("default") for p in c.get("parameters", [])}
            for c in lib["components"]}


def driveline(project):
    """The gears from the differential up to the shaft the sources drive,
    and those sources: ([(element id, type, params)], motors, clutches)."""
    dfl = defaults()
    els, conns = {}, []
    for s in project["systems"]:
        for e in s["elements"]:
            p = dict(dfl.get(e["componentDefId"], {}))
            p.update(e.get("parameterOverrides", {}))
            els[e["id"]] = (e["componentDefId"], p)
        conns += s["connections"]

    def peers(el, port):
        for c in conns:
            if (c["sourceElementId"], c["sourcePortId"]) == (el, port):
                yield c["targetElementId"], c["targetPortId"]
            if (c["targetElementId"], c["targetPortId"]) == (el, port):
                yield c["sourceElementId"], c["sourcePortId"]

    veh = next((p for t, p in els.values() if t == "vehicle.body"), {})
    lossless = (veh.get("road_load_mode") == "Coefficients A/B/C"
                and veh.get("abc_include_driveline_losses", True))
    chain = []
    el = next(i for i, (t, _) in els.items() if t == "mech.differential")
    while True:
        t, p = els[el]
        eta = 1.0 if (lossless and t in AXLE_GEARS) else float(p.get("efficiency_pct", 100)) / 100
        chain.append((el, t, p, eta))
        up = [e for e, _ in peers(el, "flange_in")]
        if len(up) != 1 or els[up[0]][0] not in GEARS:
            break
        el = up[0]
    chain.reverse()  # from the sources down to the differential
    motors, clutches, seen, todo = [], [], set(), [(e, pt) for e, pt in peers(el, "flange_in")]
    while todo:  # the shaft above the first gear: nodes, motors, clutches
        e, pt = todo.pop()
        if e in seen:
            continue
        seen.add(e)
        t = els[e][0]
        if t == "motor.emotor":
            motors.append(e)
        elif t == "mech.clutch":
            clutches.append(e)
        elif t == "mech.node":
            todo += [(x, y) for f in ("f1", "f2", "f3", "f4") for x, y in peers(e, f)]
    return chain, motors, clutches, next((i for i, (t, _) in els.items() if t == "mech.node"
                                          and i in seen), None)


def nearest_ratio(table, gear):
    keys = sorted(float(k) for k in table)
    k = min(keys, key=lambda x: abs(x - gear))
    return float(table[str(int(k)) if str(int(k)) in table else str(k)])


def rebuild(ch, chain, motors, clutches, node):
    """Today's gear channels from the channels ``ch`` (name → values)."""
    n = len(ch[f"{(motors or clutches)[0]}:sig_{'torque'}"])
    out = {}
    for k in range(n):
        if motors:
            w = ch[f"{motors[0]}:sig_speed"][k] / RPM
        else:
            w = ch[f"{node}:sig_speed"][k] / RPM
        tq = sum(ch[f"{m}:sig_torque"][k] for m in motors)
        tq += sum(ch[f"{c}:sig_torque"][k] for c in clutches)
        p = tq * w / 1000.0  # kW
        for el, t, prm, eta in chain:
            out.setdefault(f"{el}:sig_power", []).append(p)
            fwd = p >= 0.0
            p_out = p * eta if fwd else p / eta
            out.setdefault(f"{el}:sig_losses", []).append(abs(p - p_out))
            if t == "mech.gearbox":
                ratio = nearest_ratio(prm["ratios"], ch[f"{el}:sig_gear"][k])
            else:
                ratio = float(prm.get("ratio", 1.0) or 1.0)
            if t == "mech.differential":  # its torques: × eta whichever way
                out.setdefault(f"{el}:sig_torque_a", []).append(0.5 * tq * ratio * eta)
                out.setdefault(f"{el}:sig_torque_b", []).append(0.5 * tq * ratio * eta)
            tq = tq * ratio * (eta if fwd else 1.0 / eta)
            p = p_out
    return out


def main():
    args = sys.argv[1:]
    every = "--all" in args
    args = [a for a in args if a != "--all"]
    refs, outs = pathlib.Path(args[0]), pathlib.Path(args[1])
    wanted = args[2:]
    print("| case | channel | × band as compared | today's rule on the new run, × band "
          "| today's rule on today's run, × band |")
    print("|---|---|---:|---:|---:|")
    for path in sorted(refs.glob("*__*.json")):
        project, case = path.stem.split("__")
        tag = f"{project}/{case}"
        new_path = outs / f"{project}__{case}.new.json"
        if (wanted and project not in wanted and tag not in wanted) or not new_path.exists():
            continue
        ref = json.loads(path.read_text())
        new = json.loads(new_path.read_text())["channels"]
        chain, motors, clutches, node = driveline(
            json.loads((BACKEND / "projects" / f"{project}.json").read_text()))
        fine = {k: v["values"] for k, v in ref["fine"]["channels"].items()}
        normal = {k: v["values"] for k, v in ref["normal"]["channels"].items()}
        mine = rebuild(fine, chain, motors, clutches, node)
        theirs = rebuild(new, chain, motors, clutches, node)
        for name in theirs:
            if name not in fine:
                continue
            f, nn = fine[name], normal[name]
            idx = range(1, min(len(f), len(nn), len(new[name])))
            band = max(2 * rms(nn[k] - f[k] for k in idx), 0.005 * rms(f[k] for k in idx), 1e-9)
            x = [rms(s[k] - f[k] for k in idx) / band for s in (new[name], theirs[name], mine[name])]
            if every or x[0] > 1.0:
                print(f"| {tag} | {name} | {x[0]:.3f} | {x[1]:.3f} | {x[2]:.3f} |")


if __name__ == "__main__":
    main()

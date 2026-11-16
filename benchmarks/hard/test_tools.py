"""hard.py stops at once, with a plain message, when a stage needs a tool the
machine does not have (before, `all` ran 15 minutes and then failed in the
SUNDIALS stage with a traceback when cmake was missing).

    python3 benchmarks/hard/test_tools.py
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import hard  # noqa: E402


def outcome(stage, have, cpus=None):
    """None if the stage may start, else the message it stops with."""
    saved = hard.shutil.which, hard.find_cmake, hard.CPUS
    hard.shutil.which = lambda n: f"/x/{n}" if n in have else None
    hard.find_cmake = lambda: "/x/cmake" if "cmake" in have else None
    hard.CPUS = cpus
    try:
        hard.require_tools(stage)
        return None
    except SystemExit as e:
        return str(e.code)
    finally:
        hard.shutil.which, hard.find_cmake, hard.CPUS = saved


def main():
    everything = {"cargo", "gfortran", "cc", "tar", "cmake", "taskset"}
    assert outcome("all", everything, cpus="16-23") is None
    m = outcome("all", everything - {"cmake"})
    assert m and "cmake" in m and "pip install cmake" in m and "gfortran" not in m, m
    assert "Nothing was started" in m, m
    # every missing tool is named, with its way to get it
    m = outcome("all", {"cargo", "tar", "cc"})
    assert m and "gfortran" in m and "cmake" in m and "conda-forge" in m, m
    # a stage needs only its own tools
    assert outcome("probe", {"gfortran"}) is None
    m = outcome("sundials", everything - {"cc"})
    assert m and "cc" in m and "gfortran" not in m, m
    assert outcome("fetch", set(), cpus="0") is None
    assert outcome("run", set()) is None
    m = outcome("run", set(), cpus="0")
    assert m and "taskset" in m and "HARD_CPUS" in m, m
    print("ok")


if __name__ == "__main__":
    main()

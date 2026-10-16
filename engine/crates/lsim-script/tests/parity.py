"""The Python side of lsim-script's parity test: the app's own Script runner,
called as the app calls it. That is the locked-down worker's call of step()
(backend/app/solver/sandbox_worker.py's `_compile` and `_run_step`, which run
the script through scripting.compile_script and call step() without the
in-process tracer), with the parameters as the app hands them over: typed by
its project model (app/schemas.py's ParamValue, through pydantic), sent to
the worker as JSON, and a fresh shallow copy of them each call; and the
inputs as the engine passes them (sandbox.py's `float(x or 0.0)`: -0.0
becomes +0.0). The solver's modules are loaded from their files, with
app/schemas.py and nothing else of the backend (so the standard library and
pydantic are all it needs); floats travel as their 64-bit patterns.

stdin: one JSON document, a list of cases:
  {"code": str, "label": str, "params_text": str (a JSON object),
   "calls": [{"t": bits, "dt": bits, "inputs": [[name, bits], ...]}]}
stdout: one JSON document, per case either {"compile_error": str} (the
  script, or the app's reading of its parameters, refused) or
  {"results": [{"ok": [[name, bits], ...]} | {"err": str}]}
"""
import faulthandler
import importlib
import json
import struct
import sys
import threading
import types
from pathlib import Path


def f64(bits):
    return struct.unpack("<d", struct.pack("<Q", bits))[0]


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def main():
    solver_dir = Path(sys.argv[1])
    pkg = types.ModuleType("lsim_parity_solver")
    pkg.__path__ = [str(solver_dir)]
    sys.modules["lsim_parity_solver"] = pkg
    scripting = importlib.import_module("lsim_parity_solver.scripting")
    worker = importlib.import_module("lsim_parity_solver.sandbox_worker")
    # the project model's parameter type (app/schemas.py imports only
    # pydantic and app/migrations.py)
    sys.path.insert(0, str(solver_dir.parent.parent))
    from pydantic import TypeAdapter, ValidationError

    from app.schemas import ParamValue

    typed = TypeAdapter(dict[str, ParamValue])
    # the bytes as sent (UTF-8), not decoded as the locale says (Windows)
    cases = json.loads(sys.stdin.buffer.read())
    # what scripts print goes to stderr: stdout carries the results
    results_out = sys.stdout
    sys.stdout = sys.stderr
    out = []
    for case in cases:
        names = [k for k, _ in case["calls"][0]["inputs"]] if case["calls"] else []
        try:
            # read as the app reads a project, and sent to the worker as
            # JSON (sandbox.py's init message)
            params = json.loads(json.dumps(typed.validate_python(json.loads(case["params_text"]))))
        except ValidationError as e:
            out.append({"compile_error": f"the app refuses the parameters: {e}"})
            continue
        spec = {"label": case["label"], "code": case["code"], "input_keys": names,
                "params": params}
        try:
            script, err = worker._compile(scripting, spec, False)
        except (RecursionError, MemoryError) as e:
            script, err = None, str(e)
        if script is None:
            out.append({"compile_error": err})
            continue
        results = []
        for call in case["calls"]:
            values = [float(f64(b) or 0.0) for _, b in call["inputs"]]
            inputs = dict(zip(script.input_keys, values))
            try:
                res = worker._run_step(scripting, script, f64(call["t"]), f64(call["dt"]), inputs)
                results.append({"ok": [[k, bits(v)] for k, v in res.items()]})
            except scripting.ScriptError as e:
                results.append({"err": str(e)})
        out.append({"results": results})
    json.dump(out, results_out)


# A fatal error (a C stack overflow, say) tells where on stderr.
faulthandler.enable()
# The cases run on a thread with a stack of the same size on every platform:
# the hostile ones (100 000 nested enumerates, say) recurse in C as they are
# freed, deeper than python.exe's 2 MB main thread allows on Windows (Linux
# gives its main thread 8 MB).
threading.stack_size(64 * 1024 * 1024)
failed = []


def run():
    try:
        main()
    except BaseException:
        failed.append(True)
        raise


worker_thread = threading.Thread(target=run)
worker_thread.start()
worker_thread.join()
sys.exit(1 if failed else 0)

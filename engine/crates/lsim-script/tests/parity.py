"""The Python side of lsim-script's parity test: today's own script runner
(backend/app/solver/scripting.py, loaded from its file with app/solver/maps.py
and nothing else of the backend, so only the standard library is needed)
runs each case's calls; floats travel as their 64-bit patterns.

stdin: one JSON document, a list of cases:
  {"code": str, "label": str, "params_text": str (a JSON object),
   "calls": [{"t": bits, "dt": bits, "inputs": {name: bits}}]}
stdout: one JSON document, per case either {"compile_error": str} or
  {"results": [{"ok": [[name, bits], ...]} | {"err": str}]}
"""
import faulthandler
import importlib
import json
import struct
import sys
import threading
import types


def f64(bits):
    return struct.unpack("<d", struct.pack("<Q", bits))[0]


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def main():
    solver_dir = sys.argv[1]
    pkg = types.ModuleType("lsim_parity_solver")
    pkg.__path__ = [solver_dir]
    sys.modules["lsim_parity_solver"] = pkg
    scripting = importlib.import_module("lsim_parity_solver.scripting")
    # the bytes as sent (UTF-8), not decoded as the locale says (Windows)
    cases = json.loads(sys.stdin.buffer.read())
    # what scripts print goes to stderr: stdout carries the results
    results_out = sys.stdout
    sys.stdout = sys.stderr
    out = []
    for case in cases:
        try:
            fn = scripting.compile_script(case["code"], case["label"])
        except (scripting.ScriptError, RecursionError, MemoryError) as e:
            out.append({"compile_error": str(e)})
            continue
        state = {}
        # as the app reads them: in the project's own order
        params = json.loads(case["params_text"])
        results = []
        for call in case["calls"]:
            inputs = {k: f64(v) for k, v in call["inputs"].items()}
            try:
                res = scripting.run_script(fn, case["label"], f64(call["t"]), f64(call["dt"]),
                                           inputs, state, params)
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


worker = threading.Thread(target=run)
worker.start()
worker.join()
sys.exit(1 if failed else 0)

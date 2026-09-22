"""SimStudio solver package — causal multi-pass solver with explicit states.

Public surface:
- simulate(project, case_id, emit=None, control=None) → SimResult
- build_model / ModelError (used by validation)
- map & profile helpers (shared interpolation)
"""
from .core import simulate
from .maps import TableError, interp1, interp2, parse_table1d, parse_table2d
from .network import Model, ModelError, build_model
from .profiles import interp_profile, parse_profile
from .scripting import ScriptError, check_script, compile_script

__all__ = [
    "simulate",
    "build_model",
    "Model",
    "ModelError",
    "TableError",
    "interp1",
    "interp2",
    "parse_table1d",
    "parse_table2d",
    "interp_profile",
    "parse_profile",
    "ScriptError",
    "check_script",
    "compile_script",
]

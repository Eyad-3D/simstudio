#!/usr/bin/env python3
"""Build the ``lightsim`` Python wheel (AI-02).

    python scripts/build-wheel.py [--out dist]

The wheel holds the ``lightsim`` package and, as ``lightsim._app``, a copy
of the engine (``backend/app`` without its web server) with the library,
the drive cycles, the examples and VERSION, so installing it never puts a
package called ``app`` on anyone's path. It needs only pydantic. It is
proprietary like the app (EULA.txt goes into the wheel's metadata); the
file-format specification in docs/spec is separate.

Publishing it to PyPI needs the owner's account: the "Python package"
workflow uploads it only when the PYPI_API_TOKEN secret exists.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import io
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BACKEND = ROOT / "backend"
#: engine modules the wheel leaves out: the web server and what only it uses
SERVER_ONLY = {"main.py", "server.py", "security.py", "api_docs.py"}
REQUIRES = ["pydantic>=2.6"]


def _files() -> list[tuple[str, bytes]]:
    out = []
    for path in sorted((BACKEND / "lightsim").rglob("*")):
        if path.is_file() and "__pycache__" not in path.parts:
            out.append((f"lightsim/{path.relative_to(BACKEND / 'lightsim').as_posix()}",
                        path.read_bytes()))
    for path in sorted((BACKEND / "app").rglob("*")):
        rel = path.relative_to(BACKEND / "app")
        if (not path.is_file() or "__pycache__" in path.parts
                or (len(rel.parts) == 1 and rel.name in SERVER_ONLY)):
            continue
        out.append((f"lightsim/_app/{rel.as_posix()}", path.read_bytes()))
    # app/paths.py finds these beside the engine package (BUNDLE_DIR)
    for path in sorted((BACKEND / "projects").glob("*.json")):
        out.append((f"lightsim/projects/{path.name}", path.read_bytes()))
    out.append(("lightsim/VERSION", (ROOT / "VERSION").read_bytes()))
    return out


def _record_line(name: str, data: bytes) -> str:
    digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
    return f"{name},sha256={digest},{len(data)}"


def build(out_dir: Path) -> Path:
    version = (ROOT / "VERSION").read_text(encoding="utf-8").strip()
    info = f"lightsim-{version}.dist-info"
    metadata = "\n".join([
        "Metadata-Version: 2.1", "Name: lightsim", f"Version: {version}",
        "Summary: Run, check and read LightSim vehicle-simulation models from Python "
        "or a terminal",
        "Home-page: https://github.com/Eyad-3D/simstudio",
        "Author: Eyad Abualkhair",
        "License: Proprietary; free for non-commercial use, see EULA.txt",
        "Classifier: License :: Other/Proprietary License",
        "Requires-Python: >=3.11", *(f"Requires-Dist: {r}" for r in REQUIRES), "", ""])
    files = _files() + [
        (f"{info}/METADATA", metadata.encode()),
        (f"{info}/WHEEL", b"Wheel-Version: 1.0\nGenerator: lightsim build-wheel.py\n"
                          b"Root-Is-Purelib: true\nTag: py3-none-any\n"),
        (f"{info}/entry_points.txt", b"[console_scripts]\nlightsim = lightsim.cli:main\n"),
        (f"{info}/LICENSE", (ROOT / "LICENSE").read_bytes()),
        (f"{info}/EULA.txt", (ROOT / "EULA.txt").read_bytes()),
    ]
    record = "\n".join([_record_line(n, d) for n, d in files] + [f"{info}/RECORD,,", ""])
    out_dir.mkdir(parents=True, exist_ok=True)
    wheel = out_dir / f"lightsim-{version}-py3-none-any.whl"
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        for name, data in files + [(f"{info}/RECORD", record.encode())]:
            zi = zipfile.ZipInfo(name, date_time=(2026, 1, 1, 0, 0, 0))  # reproducible
            zi.external_attr = 0o644 << 16
            z.writestr(zi, data, zipfile.ZIP_DEFLATED)
    wheel.write_bytes(buf.getvalue())
    return wheel


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--out", default=str(ROOT / "backend" / "dist-wheel"))
    print(build(Path(ap.parse_args().out)))

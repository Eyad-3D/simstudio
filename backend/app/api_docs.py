"""The engine's API reference at /docs, built from its own OpenAPI
description and served from this process, so it works with no network.

FastAPI's default /docs loads Swagger UI from a CDN, which an offline
computer cannot reach. This page needs nothing from outside: one HTML page
with its styles inline and no scripts, listing every route with its
parameters, request and response bodies, and the data models. The full
machine-readable description stays at /openapi.json; the written
specification of the files and the live-run WebSocket is docs/spec/.
"""
from __future__ import annotations

from html import escape
from typing import Any

_STYLE = """
:root { color-scheme: light dark; --fg: #1d2330; --dim: #5b6475; --bg: #fff; --line: #d8dde6;
        --code: #f3f5f8; --get: #1f6feb; --post: #1a7f37; --put: #9a6700; --delete: #cf222e; }
@media (prefers-color-scheme: dark) {
  :root { --fg: #e6e9ef; --dim: #9aa4b5; --bg: #161a21; --line: #2e3542; --code: #1f2530; } }
body { font: 14px/1.5 system-ui, sans-serif; color: var(--fg); background: var(--bg);
       max-width: 960px; margin: 0 auto; padding: 16px; }
h1 { font-size: 22px; } h2 { font-size: 18px; margin-top: 32px; border-bottom: 1px solid var(--line); }
h3 { font-size: 15px; margin: 20px 0 4px; } p, li { color: var(--fg); } .dim { color: var(--dim); }
code { background: var(--code); padding: 1px 4px; border-radius: 3px; font-size: 13px; }
.m { display: inline-block; min-width: 56px; font-weight: 600; font-family: monospace; }
.GET { color: var(--get); } .POST { color: var(--post); } .PUT { color: var(--put); }
.DELETE { color: var(--delete); }
table { border-collapse: collapse; width: 100%; margin: 4px 0 12px; }
td, th { text-align: left; border-bottom: 1px solid var(--line); padding: 3px 6px; vertical-align: top; }
th { color: var(--dim); font-weight: 600; } a { color: var(--get); }
"""


def _type(schema: dict[str, Any]) -> str:
    if "$ref" in schema:
        name = schema["$ref"].rsplit("/", 1)[-1]
        return f'<a href="#model-{escape(name)}">{escape(name)}</a>'
    if "anyOf" in schema:
        return " or ".join(_type(s) for s in schema["anyOf"])
    if "enum" in schema:
        return " | ".join(f"<code>{escape(repr(v))}</code>" for v in schema["enum"])
    if "const" in schema:
        return f"<code>{escape(repr(schema['const']))}</code>"
    t = schema.get("type", "any")
    if t == "array":
        return f"list of {_type(schema.get('items', {}))}"
    if t == "object" and isinstance(schema.get("additionalProperties"), dict):
        return f"map of {_type(schema['additionalProperties'])}"
    return escape(str(t))


def _body(content: dict[str, Any]) -> str:
    for media, spec in content.items():
        return f"{_type(spec.get('schema', {}))} <span class=dim>({escape(media)})</span>"
    return ""


def render(openapi: dict[str, Any]) -> str:
    """The HTML page for an OpenAPI document."""
    info = openapi.get("info", {})
    out = [f"<!doctype html><html lang=en><head><meta charset=utf-8>"
           f"<meta name=viewport content='width=device-width, initial-scale=1'>"
           f"<title>LightSim engine API</title><style>{_STYLE}</style></head><body>",
           f"<h1>{escape(info.get('title', 'API'))} {escape(info.get('version', ''))}</h1>",
           "<p>The local engine's HTTP API, as this engine serves it. Every <code>/api</code> "
           "call from outside the LightSim window needs the per-launch token, so this page is "
           "for reading, not for trying calls. Machine-readable: <a href='/openapi.json'>"
           "/openapi.json</a>. The written specification of the project, run and study files "
           "and of the live-run WebSocket (<code>/api/simulate/run</code>) is "
           "<code>docs/spec/</code> in the LightSim repository and in the help under "
           "<i>Reference</i>.</p>", "<h2>Routes</h2>"]
    for path, ops in openapi.get("paths", {}).items():
        for method, op in ops.items():
            m = method.upper()
            out.append(f"<h3><span class='m {m}'>{m}</span> <code>{escape(path)}</code></h3>")
            text = op.get("description") or op.get("summary") or ""
            if text:
                out.append(f"<p>{escape(text)}</p>")
            params = op.get("parameters", [])
            if params:
                out.append("<table><tr><th>Parameter</th><th>In</th><th>Type</th></tr>")
                out += [f"<tr><td><code>{escape(p['name'])}</code>"
                        f"{'' if p.get('required') else ' <span class=dim>(optional)</span>'}"
                        f"</td><td>{escape(p.get('in', ''))}</td>"
                        f"<td>{_type(p.get('schema', {}))}</td></tr>" for p in params]
                out.append("</table>")
            if "requestBody" in op:
                out.append(f"<p>Body: {_body(op['requestBody'].get('content', {}))}</p>")
            for code, resp in op.get("responses", {}).items():
                body = _body(resp.get("content", {}))
                out.append(f"<p class=dim>{escape(code)} {escape(resp.get('description', ''))}"
                           f"{': ' + body if body else ''}</p>")
    schemas = openapi.get("components", {}).get("schemas", {})
    if schemas:
        out.append("<h2>Data models</h2>")
    for name, schema in sorted(schemas.items()):
        out.append(f"<h3 id='model-{escape(name)}'>{escape(name)}</h3>")
        if schema.get("description"):
            out.append(f"<p>{escape(schema['description'])}</p>")
        props = schema.get("properties", {})
        if props:
            required = set(schema.get("required", []))
            out.append("<table><tr><th>Field</th><th>Type</th><th>Default</th></tr>")
            for field, fs in props.items():
                default = fs.get("default", "" if field in required else "—")
                out.append(f"<tr><td><code>{escape(field)}</code>"
                           f"{'' if field in required else ' <span class=dim>(optional)</span>'}"
                           f"</td><td>{_type(fs)}</td><td>{escape(str(default))}</td></tr>")
            out.append("</table>")
        elif "enum" in schema:
            out.append(f"<p>{_type(schema)}</p>")
    out.append("</body></html>")
    return "\n".join(out)

"""Keep the local engine private to the LightSim window.

The engine listens on a loopback port, and any web page the user opens can
aim requests at loopback addresses. So every request is checked:

* Host must be 127.0.0.1 or localhost (plus LIGHTSIM_ALLOWED_HOSTS). A page
  that rebinds its own domain name to 127.0.0.1 still sends that name as the
  Host, so DNS rebinding is refused.
* A request with an Origin header must come from the engine's own origin (in
  development also from the Vite dev server). This covers the live-run
  WebSocket too, which browsers open to any site without a CORS check.
* The desktop shell starts the engine with a random per-launch secret in
  LIGHTSIM_TOKEN and sends it as a bearer token on the window's first page
  load. The engine answers that load with the secret in an HttpOnly,
  SameSite=Strict cookie, and every /api request and WebSocket must carry
  the cookie or the bearer header. Other sites can neither read the cookie
  nor make the browser send it, and loading the page without the secret
  does not hand it out.

Without LIGHTSIM_TOKEN (development, tests) there is no token check; the
Host and Origin checks always apply.
"""
from __future__ import annotations

import hmac
import os
from urllib.parse import urlsplit

from starlette.datastructures import Headers, MutableHeaders
from starlette.requests import cookie_parser
from starlette.responses import JSONResponse
from starlette.types import ASGIApp, Message, Receive, Scope, Send
from starlette.websockets import WebSocketClose

TOKEN_ENV = "LIGHTSIM_TOKEN"
HOSTS_ENV = "LIGHTSIM_ALLOWED_HOSTS"
COOKIE_NAME = "lightsim_token"

LOCAL_HOSTS = ("127.0.0.1", "localhost")
#: The Vite dev server (`npm run dev`); only trusted when there is no token.
DEV_ORIGINS = ("http://localhost:5173", "http://127.0.0.1:5173")

#: Answered without the token: the desktop shell polls it before any window
#: exists, and it only reports the version.
OPEN_PATHS = frozenset({"/api/health"})
#: The UI's entry page, where the cookie is handed out.
ENTRY_PATHS = frozenset({"/", "/index.html"})

#: For the UI page. Everything it loads comes from this origin; inline styles
#: are allowed because the canvas and charts set them.
CONTENT_SECURITY_POLICY = (
    "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; "
    "img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'; "
    "object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'"
)


def launch_token() -> str | None:
    return os.environ.get(TOKEN_ENV) or None


def allowed_hosts() -> tuple[str, ...]:
    extra = os.environ.get(HOSTS_ENV, "")
    return LOCAL_HOSTS + tuple(h.strip().lower() for h in extra.split(",") if h.strip())


def _hostname(host: str) -> str:
    host = host.strip().lower()
    if host.startswith("["):  # IPv6 literal, e.g. [::1]:8000
        return host[1:host.find("]")] if "]" in host else host
    return host.rsplit(":", 1)[0]


def _same_origin(origin: str, host: str) -> bool:
    parts = urlsplit(origin)
    return parts.scheme in ("http", "https") and parts.netloc.lower() == host.strip().lower()


class LocalOnlyMiddleware:
    """Refuses foreign hosts, foreign origins and, when a launch token is
    set, /api requests that do not carry it. Pure ASGI, so it sees the
    WebSocket handshake as well as plain HTTP."""

    def __init__(
        self,
        app: ASGIApp,
        token: str | None = None,
        hosts: tuple[str, ...] = LOCAL_HOSTS,
        origins: tuple[str, ...] = (),
    ) -> None:
        self.app = app
        self.token = token or None
        self.hosts = frozenset(hosts)
        self.origins = frozenset(origins)

    async def __call__(self, scope: Scope, receive: Receive, send: Send) -> None:
        if scope["type"] not in ("http", "websocket"):
            await self.app(scope, receive, send)
            return
        headers = Headers(scope=scope)
        host = headers.get("host", "")
        if _hostname(host) not in self.hosts:
            await self._refuse(scope, receive, send, 400, "Invalid host header.")
            return
        origin = headers.get("origin")
        if origin is not None and origin not in self.origins and not _same_origin(origin, host):
            await self._refuse(scope, receive, send, 403, "Cross-origin request refused.")
            return

        path = scope["path"]
        has_token = self._has_token(headers)
        if self.token and path.startswith("/api/") and path not in OPEN_PATHS and not has_token:
            await self._refuse(scope, receive, send, 401, "Missing or invalid session token.")
            return
        if scope["type"] == "http" and path in ENTRY_PATHS:
            send = self._entry_page_headers(send, set_cookie=has_token)
        await self.app(scope, receive, send)

    def _has_token(self, headers: Headers) -> bool:
        if not self.token:
            return False
        cookie = cookie_parser(headers.get("cookie", "")).get(COOKIE_NAME, "")
        auth = headers.get("authorization", "")
        bearer = auth[7:].strip() if auth[:7].lower() == "bearer " else ""
        expected = self.token.encode()
        return any(v and hmac.compare_digest(v.encode(), expected) for v in (cookie, bearer))

    def _entry_page_headers(self, send: Send, set_cookie: bool) -> Send:
        async def wrapped(message: Message) -> None:
            if message["type"] == "http.response.start":
                headers = MutableHeaders(scope=message)
                headers["content-security-policy"] = CONTENT_SECURITY_POLICY
                # a cached copy would skip the load that sets a new launch's cookie
                headers["cache-control"] = "no-store"
                if set_cookie:
                    headers.append(
                        "set-cookie",
                        f"{COOKIE_NAME}={self.token}; Path=/; HttpOnly; SameSite=Strict",
                    )
            await send(message)

        return wrapped

    @staticmethod
    async def _refuse(scope: Scope, receive: Receive, send: Send, status: int, detail: str) -> None:
        if scope["type"] == "websocket":
            # closing before accept makes the server answer the handshake with 403
            await WebSocketClose(code=1008, reason=detail)(scope, receive, send)
        else:
            await JSONResponse({"detail": detail}, status_code=status)(scope, receive, send)

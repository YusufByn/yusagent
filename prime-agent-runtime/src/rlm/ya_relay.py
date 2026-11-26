"""yusAgent (YA-11, ticket 41, PR B): MCP HTTP requests through the worker.

The kernel never reads ``auth.json`` nor holds a token. A server that needs a credential or a
configured header sends each HTTP request to the worker (``host_request("mcp.http")``), which
adds them and returns the complete response. The server-initiated GET stream is not relayed
(the worker answers 405; the SDK gives it up after its retries).

Listed in ``docs/fork-changes.md``.
"""

from __future__ import annotations

from typing import Any

from . import host_request

# Response headers describing the worker's connection, not the body handed back.
_DROPPED_RESPONSE_HEADERS = {"content-length", "transfer-encoding", "content-encoding", "connection"}


def relayed(config: dict[str, Any]) -> bool:
    """Whether this server's requests carry a credential or a configured header: the worker
    adds them, so they go through ``mcp.http``."""
    return (
        config.get("relay") is True
        or config.get("oauth") is True
        or config.get("credentialSource") in ("static-token", "acp")
        or bool(config.get("bearerTokenEnvVar"))
    )


def relay_client(server: str, read_timeout: float, headers: dict[str, str] | None = None, send=host_request):
    """An ``httpx2.AsyncClient`` whose every request goes through the worker for ``server``.

    ``send`` is the caller's ``host_request`` (each module keeps its own binding).
    """
    import httpx2

    class _RelayTransport(httpx2.AsyncBaseTransport):
        async def handle_async_request(self, request: httpx2.Request) -> httpx2.Response:
            body = await request.aread()
            sent = {name: value for name, value in request.headers.items() if name.lower() != "authorization"}
            reply = await send(
                "mcp.http",
                {
                    "server": server,
                    "method": request.method,
                    "url": str(request.url),
                    "headers": sent,
                    "body": body.decode("utf-8", errors="replace"),
                },
            )
            reply_headers = {
                name: value
                for name, value in (reply.get("headers") or {}).items()
                if name.lower() not in _DROPPED_RESPONSE_HEADERS
            }
            return httpx2.Response(
                int(reply.get("status", 502)),
                headers=reply_headers,
                content=str(reply.get("body") or "").encode("utf-8"),
                request=request,
            )

    return httpx2.AsyncClient(
        transport=_RelayTransport(),
        headers=headers or {},
        timeout=httpx2.Timeout(30.0, read=read_timeout),
        follow_redirects=False,
    )

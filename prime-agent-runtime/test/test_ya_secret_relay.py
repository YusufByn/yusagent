"""yusAgent (YA-11, ticket 41, PR B): the kernel never reads a credential.

The kernel's HTTP requests to an MCP server go through the worker (`host_request("mcp.http")`),
which adds the token bound to the configured endpoint; a web search goes through
`host_request("websearch.search")`, which adds the Serper key. The kernel never reads
`auth.json`, never sends an `Authorization` header and never sees a token.

Here the worker is a fake relay that forwards to a local MCP fixture server and adds the
sentinel token itself. The secrets are sentinels; nothing leaves 127.0.0.1.
"""

from __future__ import annotations

import asyncio
import importlib.machinery
import importlib.util
import json
import os
import socket
import subprocess
import sys
import tempfile
import time
import types
import unittest
from pathlib import Path
from unittest import mock

from rlm import mcp
from rlm import mcp_base

SENTINEL = "ya41-sentinel-token-7f3a"
SERPER_SENTINEL = "ya41-sentinel-serper-key-8a4c"

WEBSEARCH = Path(__file__).parents[2] / "skills/websearch/src/websearch/websearch.py"

_HTTP_FIXTURE = """from mcp.server.mcpserver import MCPServer
server = MCPServer("fixture")
@server.tool(name="http/raw.tool")
def echo(value: str) -> dict[str, str]:
    return {"value": value}
server.run(transport="streamable-http", host="127.0.0.1", port=int(__import__("sys").argv[1]))
"""


def run(coro):
    return asyncio.run(coro)


class AuthReads:
    """Records every `Path.read_text` of an `auth.json` while active."""

    def __init__(self):
        self.paths: list[str] = []
        self._original = Path.read_text

    def __enter__(self):
        original = self._original
        reads = self.paths

        def read_text(path, *args, **kwargs):
            if Path(path).name == "auth.json":
                reads.append(str(path))
            return original(path, *args, **kwargs)

        self._patch = mock.patch.object(Path, "read_text", read_text)
        self._patch.start()
        return self

    def __exit__(self, *exc):
        self._patch.stop()
        return False


class FakeRelay:
    """The worker side of `mcp.http`: forwards to the server and adds the bound token."""

    def __init__(self):
        self.requests: list[dict] = []
        self.calls: list[tuple[str, dict]] = []

    async def __call__(self, request_type, payload):
        self.calls.append((request_type, json.loads(json.dumps(payload))))
        if request_type == "mcp.refresh":
            return {}
        if request_type != "mcp.http":
            raise AssertionError(f"unexpected host request {request_type!r}")
        self.requests.append(payload)
        if payload["method"] == "GET":
            return {"status": 405, "headers": {}, "body": ""}
        import httpx2

        headers = dict(payload.get("headers", {}))
        headers["Authorization"] = f"Bearer {SENTINEL}"
        async with httpx2.AsyncClient(timeout=30) as client:
            response = await client.request(
                payload["method"], payload["url"], headers=headers, content=payload.get("body", "").encode()
            )
        return {"status": response.status_code, "headers": dict(response.headers), "body": response.text}


class HttpFixture:
    """The local MCP fixture server (streamable HTTP, no auth of its own)."""

    def __enter__(self):
        self.tmp = tempfile.TemporaryDirectory()
        fixture = Path(self.tmp.name) / "http_server.py"
        fixture.write_text(_HTTP_FIXTURE)
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        self.url = f"http://127.0.0.1:{port}/mcp"
        self.process = subprocess.Popen(
            [sys.executable, str(fixture), str(port)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
        )
        deadline = time.time() + 10
        while time.time() < deadline:
            with socket.socket() as probe:
                if probe.connect_ex(("127.0.0.1", port)) == 0:
                    return self
            time.sleep(0.05)
        self.__exit__(None, None, None)
        raise RuntimeError("HTTP MCP fixture did not start")

    def __exit__(self, *exc):
        self.process.terminate()
        try:
            self.process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=3)
        self.tmp.cleanup()
        return False


def agent_dir_with(credentials: dict) -> tempfile.TemporaryDirectory:
    tmp = tempfile.TemporaryDirectory()
    (Path(tmp.name) / "auth.json").write_text(json.dumps(credentials))
    return tmp


def bound_oauth(url: str) -> dict:
    return {"type": "oauth", "access": SENTINEL, "refresh": "ya41-refresh", "expires": 4102444800000, "endpoint": url}


class McpRelayTest(unittest.TestCase):
    def setUp(self):
        mcp._registry = mcp._Registry()

    def test_generic_http_server_goes_through_the_relay_without_reading_auth(self):
        with HttpFixture() as fixture:
            agent = agent_dir_with({"mcp:svc": bound_oauth(fixture.url)})
            self.addCleanup(agent.cleanup)
            relay = FakeRelay()

            async def scenario():
                generation = mcp._Generation("svc", {"type": "http", "url": fixture.url, "oauth": True})
                await generation.open()
                try:
                    return await generation.call("http/raw.tool", {"value": "ok"})
                finally:
                    await generation.close()

            with mock.patch.dict(os.environ, {"PRIME_AGENT_CODING_AGENT_DIR": agent.name}), mock.patch.object(
                mcp, "host_request", relay
            ), AuthReads() as reads:
                result = run(scenario())

        self.assertEqual(reads.paths, [], "the kernel read auth.json")
        self.assertTrue(relay.requests, "the kernel called the MCP server itself, not through mcp.http")
        for request in relay.requests:
            names = {name.lower() for name in request.get("headers", {})}
            self.assertNotIn("authorization", names, "the kernel sent an Authorization header")
            self.assertEqual(request["server"], "svc")
        self.assertNotIn(SENTINEL, json.dumps(relay.calls), "a token went through the kernel")
        self.assertEqual(result, {"value": "ok"})

    def test_mcp_integration_goes_through_the_relay_without_reading_auth(self):
        with HttpFixture() as fixture:
            agent = agent_dir_with({"mcp:svc": bound_oauth(fixture.url)})
            self.addCleanup(agent.cleanup)
            relay = FakeRelay()

            class Svc(mcp_base.McpIntegration):
                server = "svc"
                url = fixture.url

            with mock.patch.dict(os.environ, {"PRIME_AGENT_CODING_AGENT_DIR": agent.name}), mock.patch.object(
                mcp_base, "host_request", relay
            ), mock.patch.object(mcp, "host_request", relay), AuthReads() as reads:
                tools = run(Svc().list_tools())

        self.assertEqual(reads.paths, [], "the integration read auth.json")
        self.assertTrue(relay.requests, "the integration called the MCP server itself, not through mcp.http")
        for request in relay.requests:
            names = {name.lower() for name in request.get("headers", {})}
            self.assertNotIn("authorization", names, "the integration sent an Authorization header")
        self.assertNotIn(SENTINEL, json.dumps(relay.calls), "a token went through the kernel")
        self.assertEqual([tool["name"] for tool in tools], ["http/raw.tool"])


class DirectClient:
    """Stands for `httpx.AsyncClient`: the skill must never build one (it would hold the key)."""

    def __init__(self, *args, **kwargs):
        raise AssertionError("the skill called Serper itself, with the key")


def fake_httpx() -> types.ModuleType:
    """A stand-in `httpx` (the runtime suite has only `httpx2`; the kernel venv has both)."""
    module = types.ModuleType("httpx")
    module.__spec__ = importlib.machinery.ModuleSpec("httpx", None)
    module.AsyncClient = DirectClient
    module.HTTPStatusError = type("HTTPStatusError", (Exception,), {})
    return module


class WebsearchRelayTest(unittest.TestCase):
    def load(self):
        spec = importlib.util.spec_from_file_location("ya41_websearch", WEBSEARCH)
        assert spec and spec.loader
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def test_search_goes_through_the_relay_without_reading_the_key(self):
        stand_in = fake_httpx()
        with mock.patch.dict(sys.modules, {"httpx": stand_in}):
            module = self.load()
        agent = agent_dir_with({"serper": {"type": "api_key", "key": SERPER_SENTINEL}})
        self.addCleanup(agent.cleanup)
        calls: list[tuple[str, dict]] = []

        async def host_request(request_type, payload):
            calls.append((request_type, payload))
            return {"organic": [{"title": "ya41 result", "link": "https://example.invalid/", "snippet": "ok"}]}

        env = {"PRIME_AGENT_CODING_AGENT_DIR": agent.name, "SERPER_API_KEY": SERPER_SENTINEL}
        with mock.patch.dict(os.environ, env), mock.patch.dict(sys.modules, {"httpx": stand_in}), mock.patch.object(
            module, "host_request", host_request, create=True
        ), mock.patch("rlm.host_request", host_request), AuthReads() as reads:
            output = run(module.run("ya41", num_results=3))

        self.assertEqual(reads.paths, [], "the skill read auth.json")
        self.assertEqual([call[0] for call in calls], ["websearch.search"])
        self.assertEqual(calls[0][1].get("query"), "ya41")
        self.assertNotIn(SERPER_SENTINEL, json.dumps(calls), "the key went through the kernel")
        self.assertIn("ya41 result", output)


if __name__ == "__main__":
    unittest.main()

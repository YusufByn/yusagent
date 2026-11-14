"""Proxys du smoke bench (T06, YA-14), lancés hors sandbox par le runner.

Reverse proxy (accès au modèle) : le `baseUrl` du provider pointe sur
`http://127.0.0.1:<port>/backend-api`. Chaque connexion est relayée vers une cible
amont fixe (`chatgpt.com:443` en TLS), jamais choisie par le client : seuls les
chemins sous `/backend-api/codex/` passent, tout le reste reçoit 403. HTTP (SSE) et
WebSocket passent par le même port, ce qui mesure le transport par défaut du harness
(WebSocket) au lieu de le forcer en SSE.

Proxy CONNECT (rafraîchissement du jeton) : `HTTPS_PROXY` du harness. N'accepte que
les hôtes de `hosts/<provider>.txt`, port 443. Mode enregistrement (`record=True`,
run de chauffe de `record-hosts` seulement) : accepte tout hôte sur le port 443 et le
note, pour établir la liste.

Journaux (JSON Lines) : jamais d'en-tête ni de corps. Par requête modèle : horodatage,
transport, méthode, chemin, statut, octets, l'identifiant de session lu dans le
corps (`prompt_cache_key`, l'id de session du harness), qui sert à apparier la
requête avec le journal d'usage, et son champ `generate` (YA-14 : `false` pour le prewarm
de Codex CLI). Par réponse (YA-14, sens serveur → client) : sa fin (`event: response` :
type d'événement, id de réponse, statut, compteurs d'usage) ou une erreur
(`event: response_error` : statut, type et code), jamais de texte. Par CONNECT : hôte,
port, accepté ou refusé.

Inactivité (YA-14) : le reverse proxy note l'heure du dernier octet relayé, dans un sens ou
dans l'autre (`idle_seconds`) ; le runner arrête une tâche dont le flux reste muet trop
longtemps (`--idle-timeout`, statut `stalled`).

Bibliothèque standard uniquement.
"""

from __future__ import annotations

import collections
import json
import socket
import socketserver
import ssl
import struct
import threading
import time
from pathlib import Path

UPSTREAM_HOST = "chatgpt.com"
UPSTREAM_PORT = 443
ALLOWED_PREFIX = "/backend-api/codex/"
HEAD_LIMIT = 64 * 1024
# En-têtes refaits par le proxy : la cible amont est fixe, la connexion amont est
# fermée après chaque requête HTTP, et aucune extension WebSocket n'est négociée
# (les trames du client doivent rester lisibles pour compter les requêtes).
DROPPED_HEADERS = {"host", "connection", "proxy-connection", "keep-alive",
                   "proxy-authorization", "sec-websocket-extensions"}


class JsonLog:
    def __init__(self, path: Path):
        self.path = path
        self.lock = threading.Lock()
        path.parent.mkdir(parents=True, exist_ok=True)
        path.touch()

    def write(self, **entry) -> None:
        entry = {"ts": round(time.time() * 1000), **entry}
        with self.lock, self.path.open("a") as handle:
            handle.write(json.dumps(entry, ensure_ascii=False) + "\n")


def read_head(sock: socket.socket) -> tuple[bytes, bytes]:
    """En-tête HTTP (jusqu'à la ligne vide) et octets déjà lus au-delà."""
    data = b""
    while b"\r\n\r\n" not in data:
        chunk = sock.recv(65536)
        if not chunk:
            break
        data += chunk
        if len(data) > HEAD_LIMIT:
            raise ValueError("en-tête trop long")
    head, _, rest = data.partition(b"\r\n\r\n")
    return head, rest


def parse_head(head: bytes) -> tuple[str, list[tuple[str, str]]]:
    lines = head.decode("latin-1").split("\r\n")
    headers = []
    for line in lines[1:]:
        name, _, value = line.partition(":")
        headers.append((name.strip(), value.strip()))
    return lines[0], headers


def header(headers: list[tuple[str, str]], name: str) -> str | None:
    return next((v for k, v in headers if k.lower() == name), None)


def session_of(body: bytes) -> str | None:
    """`prompt_cache_key` d'un corps de requête Codex (id de session), seul champ lu."""
    try:
        value = json.loads(body).get("prompt_cache_key")
    except (ValueError, AttributeError):
        return None
    return value if isinstance(value, str) else None


def generate_of(body: bytes) -> bool | None:
    """Champ `generate` d'un corps de requête (YA-14 : `false` pour le prewarm de Codex CLI)."""
    try:
        value = json.loads(body).get("generate")
    except (ValueError, AttributeError):
        return None
    return value if isinstance(value, bool) else None


def pipe(src: socket.socket, dst: socket.socket, counter: list[int], on_bytes=None,
         on_data=None) -> None:
    try:
        while True:
            chunk = src.recv(65536)
            if not chunk:
                break
            counter[0] += len(chunk)
            if on_bytes is not None:
                on_bytes()
            if on_data is not None:
                on_data(chunk)
            dst.sendall(chunk)
    except OSError:
        pass
    finally:
        for sock in (dst, src):
            try:
                sock.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass


class FrameReader:
    """Messages d'un sens d'un WebSocket relayé (trames texte ou binaires, fragmentées ou non,
    masquées côté client) ; `message` reçoit chaque message complet."""

    def __init__(self):
        self.buffer = b""
        self.parts: list[bytes] = []

    def feed(self, data: bytes) -> None:
        self.buffer += data
        while True:
            frame = self.next_frame()
            if frame is None:
                return
            fin, opcode, payload = frame
            if opcode in (0x0, 0x1, 0x2):
                self.parts.append(payload)
                if fin:
                    message, self.parts = b"".join(self.parts), []
                    self.message(message)

    def next_frame(self) -> tuple[bool, int, bytes] | None:
        buf = self.buffer
        if len(buf) < 2:
            return None
        fin, opcode = bool(buf[0] & 0x80), buf[0] & 0x0F
        masked, length, offset = bool(buf[1] & 0x80), buf[1] & 0x7F, 2
        if length == 126:
            if len(buf) < 4:
                return None
            length, offset = struct.unpack(">H", buf[2:4])[0], 4
        elif length == 127:
            if len(buf) < 10:
                return None
            length, offset = struct.unpack(">Q", buf[2:10])[0], 10
        mask = buf[offset:offset + 4] if masked else b""
        offset += len(mask)
        if len(buf) < offset + length:
            return None
        payload = buf[offset:offset + length]
        if masked:
            payload = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        self.buffer = buf[offset + length:]
        return fin, opcode, payload

    def message(self, message: bytes) -> None:
        raise NotImplementedError


class FrameCounter(FrameReader):
    """Lit les trames client d'un WebSocket relayé et note chaque `response.create`, avec son
    champ `generate` (YA-14 : `false` pour le prewarm de Codex CLI, sans génération)."""

    def __init__(self, on_request):
        super().__init__()
        self.on_request = on_request

    def message(self, message: bytes) -> None:
        try:
            event = json.loads(message)
        except ValueError:
            return
        if isinstance(event, dict) and event.get("type") == "response.create":
            key = event.get("prompt_cache_key")
            generate = event.get("generate")
            self.on_request(key if isinstance(key, str) else None,
                            event.get("previous_response_id") is not None, len(message), message,
                            generate if isinstance(generate, bool) else None)


def usage_counts(usage) -> dict:
    """Compteurs d'usage d'une réponse Responses : des nombres, rien d'autre."""
    usage = usage if isinstance(usage, dict) else {}
    inp = usage.get("input_tokens_details") or {}
    out = usage.get("output_tokens_details") or {}
    return {"input_tokens": usage.get("input_tokens"), "cached_tokens": inp.get("cached_tokens"),
            "cache_write_tokens": inp.get("cache_write_tokens"),
            "output_tokens": usage.get("output_tokens"),
            "reasoning_tokens": out.get("reasoning_tokens")}


def response_outcome(event) -> dict | None:
    """Fin d'une réponse dans un événement du serveur (YA-14) : usage d'une réponse terminée,
    statut d'une réponse en échec ou incomplète, type et code d'une erreur. Jamais de texte."""
    if not isinstance(event, dict):
        return None
    kind = event.get("type")
    response = event.get("response") if isinstance(event.get("response"), dict) else {}
    if kind in ("response.completed", "response.done", "response.failed", "response.incomplete"):
        return {"event": "response", "kind": kind, "response_id": response.get("id"),
                "status": response.get("status"), "usage": usage_counts(response.get("usage"))}
    if kind == "error":
        error = event.get("error") if isinstance(event.get("error"), dict) else {}
        return {"event": "response_error", "kind": kind, "status": event.get("status"),
                "error_type": error.get("type"), "error_code": error.get("code")}
    return None


class ResponseWatcher(FrameReader):
    """Sens serveur → client (YA-14) : fin de chaque réponse (`response_outcome`), trames
    WebSocket ou flux SSE. Nourri par les octets relayés ; n'écrit ni corps ni texte."""

    def __init__(self, on_outcome, websocket: bool):
        super().__init__()
        self.on_outcome = on_outcome
        self.websocket = websocket
        self.text = b""

    def feed(self, data: bytes) -> None:
        if self.websocket:
            super().feed(data)
            return
        self.text += data
        while b"\n\n" in self.text:
            block, self.text = self.text.split(b"\n\n", 1)
            lines = [line[5:].strip() for line in block.replace(b"\r", b"").split(b"\n")
                     if line.startswith(b"data:")]
            if lines:
                self.message(b"\n".join(lines))

    def message(self, message: bytes) -> None:
        try:
            outcome = response_outcome(json.loads(message))
        except ValueError:
            return
        if outcome is not None:
            self.on_outcome(outcome)


class ReverseProxy(socketserver.ThreadingTCPServer):
    """Relais vers une cible amont fixe. `upstream` n'est jamais lu dans une requête."""

    daemon_threads = True
    allow_reuse_address = True

    def __init__(self, log_path: Path, upstream: tuple[str, int, bool] | None = None,
                 bodies_path: Path | None = None, allowed_paths: set[str] | None = None):
        # `upstream` n'est passé que par le mode capture du runner (serveur local, sans
        # TLS) ; sinon la cible est chatgpt.com:443 en TLS.
        self.upstream = upstream or (UPSTREAM_HOST, UPSTREAM_PORT, True)
        # Liste stricte de chemins (YA-14, bras Codex CLI : `/backend-api/codex/responses`
        # seul) ; sans elle, tout chemin sous ALLOWED_PREFIX passe.
        self.allowed_paths = allowed_paths
        if upstream is not None and upstream[0] != "127.0.0.1":
            raise ValueError("cible amont de test : 127.0.0.1 uniquement")
        self.log = JsonLog(log_path)
        # Corps des requêtes (`--log-bodies`, désactivé par défaut, refusé pendant un A/B) :
        # le JSON envoyé au modèle seul, jamais les en-têtes (authentification).
        self.bodies = JsonLog(bodies_path) if bodies_path is not None else None
        self.tls = ssl.create_default_context()
        # Garde-fou d'inactivité du runner (YA-14) : dernier octet relayé, dans un sens ou
        # dans l'autre (horloge monotone).
        self.last_activity = time.monotonic()
        super().__init__(("127.0.0.1", 0), ReverseHandler)

    @property
    def port(self) -> int:
        return self.server_address[1]

    def touch(self) -> None:
        self.last_activity = time.monotonic()

    def write_body(self, transport: str, path: str, session: str | None,
                   continuation: bool | None, raw: bytes) -> None:
        if self.bodies is None:
            return
        try:
            body = json.loads(raw)
        except ValueError:
            body = None
        self.bodies.write(transport=transport, path=path, session=session,
                          continuation=continuation, bytes_up=len(raw), body=body)

    def idle_seconds(self) -> float:
        """Secondes depuis le dernier octet relayé (ou le démarrage du proxy)."""
        return time.monotonic() - self.last_activity

    def connect_upstream(self) -> socket.socket:
        host, port, tls = self.upstream
        sock = socket.create_connection((host, port), timeout=30)
        sock.settimeout(None)
        return self.tls.wrap_socket(sock, server_hostname=host) if tls else sock


class ReverseHandler(socketserver.BaseRequestHandler):
    server: ReverseProxy

    def handle(self) -> None:
        self.server.touch()
        client = self.request
        started = time.monotonic()
        try:
            head, rest = read_head(client)
            request_line, headers = parse_head(head)
            method, target, _ = (request_line.split(" ") + ["", "", ""])[:3]
        except (OSError, ValueError):
            return
        path = target.split("?", 1)[0]
        is_ws = (header(headers, "upgrade") or "").lower() == "websocket"
        transport = "websocket" if is_ws else "http"
        if (not path.startswith(ALLOWED_PREFIX) or not target.startswith("/")
                or (self.server.allowed_paths is not None
                    and path not in self.server.allowed_paths)):
            self.server.log.write(event="refused", transport=transport, method=method,
                                  path=path, reason="chemin hors cible")
            client.sendall(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n"
                           b"Connection: close\r\n\r\n")
            return
        body = rest
        length = header(headers, "content-length")
        if not is_ws and length is None and header(headers, "transfer-encoding"):
            self.server.log.write(event="refused", transport=transport, method=method,
                                  path=path, reason="corps chunked non géré")
            client.sendall(b"HTTP/1.1 411 Length Required\r\nContent-Length: 0\r\n"
                           b"Connection: close\r\n\r\n")
            return
        while length is not None and len(body) < int(length):
            chunk = client.recv(65536)
            if not chunk:
                break
            body += chunk
            self.server.touch()
        host = self.server.upstream[0]
        out = [f"{method} {target} HTTP/1.1", f"Host: {host}"]
        out += [f"{k}: {v}" for k, v in headers
                if k.lower() not in DROPPED_HEADERS and not (is_ws and k.lower() == "upgrade")]
        out += ["Upgrade: websocket", "Connection: Upgrade"] if is_ws else ["Connection: close"]
        try:
            upstream = self.server.connect_upstream()
            upstream.sendall(("\r\n".join(out) + "\r\n\r\n").encode("latin-1") + body)
        except OSError as error:
            self.server.log.write(event="upstream_error", transport=transport, method=method,
                                  path=path, error=type(error).__name__)
            client.sendall(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n"
                           b"Connection: close\r\n\r\n")
            return
        if is_ws:
            self.relay_websocket(client, upstream, path, started)
        else:
            self.relay_http(client, upstream, method, path, body, started)

    def relay_http(self, client, upstream, method, path, body, started) -> None:
        received = [0]
        first = upstream.recv(65536)
        self.server.touch()
        status = first.split(b" ", 2)[1].decode("latin-1") if first.count(b" ") >= 2 else None
        transport = "sse" if method == "POST" else "http"
        session, generate = session_of(body), generate_of(body)
        self.server.log.write(event="request", transport=transport, method=method, path=path,
                              status=status, session=session, bytes_up=len(body),
                              generate=generate)
        if method == "POST":
            self.server.write_body("sse", path, session, None, body)
        try:
            client.sendall(first)
        except OSError:
            return
        received[0] += len(first)
        # Usage de la réponse, lu dans le flux SSE après l'en-tête HTTP (compteurs seuls).
        watcher = ResponseWatcher(lambda outcome: self.server.log.write(
            transport=transport, path=path, session=session, generate=generate, **outcome),
            websocket=False)
        watcher.feed(first.partition(b"\r\n\r\n")[2])
        pipe(upstream, client, received, self.server.touch, watcher.feed)
        self.server.log.write(event="done", transport="sse" if method == "POST" else "http",
                              path=path, bytes_down=received[0],
                              ms=round((time.monotonic() - started) * 1000))

    def relay_websocket(self, client, upstream, path, started) -> None:
        head, rest = read_head(upstream)
        self.server.touch()
        status_line, _ = parse_head(head)
        parts = status_line.split(" ", 2)
        status = parts[1] if len(parts) > 1 else None
        self.server.log.write(event="ws_open", transport="websocket", path=path, status=status)
        client.sendall(head + b"\r\n\r\n" + rest)
        if status != "101":
            pipe(upstream, client, [0])
            return

        # Requêtes en attente de leur réponse, dans l'ordre (une connexion sert ses requêtes
        # l'une après l'autre) : la fin de chaque réponse est notée avec sa session.
        pending: collections.deque = collections.deque()

        def on_request(session, continuation, size, message, generate=None):
            pending.append((session, generate))
            self.server.log.write(event="request", transport="websocket", method="WS",
                                  path=path, status=status, session=session,
                                  continuation=continuation, bytes_up=size, generate=generate)
            self.server.write_body("websocket", path, session, continuation, message)

        def on_outcome(outcome):
            session, generate = pending.popleft() if pending else (None, None)
            self.server.log.write(transport="websocket", path=path, session=session,
                                  generate=generate, **outcome)

        counter = FrameCounter(on_request)
        watcher = ResponseWatcher(on_outcome, websocket=True)
        watcher.feed(rest)
        up, down = [0], [len(rest)]

        def client_to_upstream():
            try:
                while True:
                    chunk = client.recv(65536)
                    if not chunk:
                        break
                    up[0] += len(chunk)
                    self.server.touch()
                    counter.feed(chunk)
                    upstream.sendall(chunk)
            except OSError:
                pass
            finally:
                for sock in (upstream, client):
                    try:
                        sock.shutdown(socket.SHUT_RDWR)
                    except OSError:
                        pass

        thread = threading.Thread(target=client_to_upstream, daemon=True)
        thread.start()
        pipe(upstream, client, down, self.server.touch, watcher.feed)
        thread.join(timeout=5)
        self.server.log.write(event="ws_close", transport="websocket", path=path,
                              bytes_up=up[0], bytes_down=down[0],
                              ms=round((time.monotonic() - started) * 1000))


class ConnectProxy(socketserver.ThreadingTCPServer):
    """Proxy CONNECT à liste blanche (port 443 seulement)."""

    daemon_threads = True
    allow_reuse_address = True

    def __init__(self, log_path: Path, allowed: set[str], record: bool = False):
        self.allowed = {host.lower() for host in allowed}
        self.record = record
        self.log = JsonLog(log_path)
        super().__init__(("127.0.0.1", 0), ConnectHandler)

    @property
    def port(self) -> int:
        return self.server_address[1]


class ConnectHandler(socketserver.BaseRequestHandler):
    server: ConnectProxy

    def handle(self) -> None:
        client = self.request
        try:
            head, rest = read_head(client)
            request_line, _ = parse_head(head)
        except (OSError, ValueError):
            return
        method, target = (request_line.split(" ") + ["", ""])[:2]
        host, _, port = target.rpartition(":")
        host = host.strip("[]").lower()
        allowed = (method == "CONNECT" and port == "443"
                   and (self.server.record or host in self.server.allowed))
        self.server.log.write(event="connect", host=host if method == "CONNECT" else None,
                              port=port if method == "CONNECT" else None, method=method,
                              accepted=allowed)
        if not allowed:
            client.sendall(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
            return
        try:
            upstream = socket.create_connection((host, 443), timeout=30)
            upstream.settimeout(None)
        except OSError as error:
            self.server.log.write(event="connect_error", host=host, error=type(error).__name__)
            client.sendall(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
            return
        client.sendall(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        if rest:
            upstream.sendall(rest)
        thread = threading.Thread(target=pipe, args=(client, upstream, [0]), daemon=True)
        thread.start()
        pipe(upstream, client, [0])
        thread.join(timeout=5)


def serve(server: socketserver.BaseServer) -> threading.Thread:
    thread = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.2},
                              daemon=True)
    thread.start()
    return thread


def read_log(path: Path) -> list[dict]:
    if not path.exists():
        return []
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def load_hosts(path: Path) -> set[str]:
    """Hôtes autorisés : une ligne par hôte, `#` pour les commentaires."""
    if not path.exists():
        return set()
    return {line.split("#", 1)[0].strip().lower() for line in path.read_text().splitlines()
            if line.split("#", 1)[0].strip()}

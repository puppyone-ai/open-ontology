"""Operator-owned, single-destination CONNECT tunnel. Standard library only."""

import argparse
import ipaddress
import json
import os
import re
import selectors
import socket
import socketserver
import stat
import sys
import threading
import time

LISTEN = ("127.0.0.1", 19483)
HEADER_LIMIT = 8192
HEADER_TIMEOUT = 5
CONNECT_TIMEOUT = 10
IDLE_TIMEOUT = 120
MAX_CONNECTIONS = 16


def validate_config(config):
    expected = {"host", "port", "ipv4", "interface", "listen_host", "listen_port"}
    if not isinstance(config, dict) or set(config) != expected:
        raise ValueError("invalid_config")
    host = config["host"]
    if (not isinstance(host, str) or len(host) > 253 or "." not in host
            or not all(re.fullmatch(r"[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?", label)
                       for label in host.split("."))):
        raise ValueError("invalid_host")
    try:
        ipaddress.ip_address(host)
    except ValueError:
        pass
    else:
        raise ValueError("hostname_required")
    if not isinstance(config["ipv4"], str):
        raise ValueError("public_ipv4_required")
    address = ipaddress.IPv4Address(config["ipv4"])
    if not address.is_global or address.is_multicast or address.is_reserved:
        raise ValueError("public_ipv4_required")
    if (type(config["port"]) is not int or config["port"] != 443
            or config["interface"] != "en0"
            or config["listen_host"] != LISTEN[0]
            or type(config["listen_port"]) is not int
            or config["listen_port"] != LISTEN[1]):
        raise ValueError("fixed_route_required")
    return dict(config)


def load_config(path):
    # Refuse symlinks and group/world permissions; never print config contents.
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as source:
        info = os.fstat(source.fileno())
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
                or stat.S_IMODE(info.st_mode) & 0o077 or info.st_size > 4096):
            raise ValueError("private_config_required")
        return validate_config(json.loads(source.read(4097)))


def connect(config):
    if sys.platform != "darwin":
        raise ValueError("darwin_required")
    stream = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        # Same per-socket binding as deploy/ssh-allowlist/direct_tcp.py.
        stream.setsockopt(socket.IPPROTO_IP, 25, socket.if_nametoindex("en0"))
        stream.settimeout(CONNECT_TIMEOUT)
        stream.connect((config["ipv4"], 443))
        return stream
    except Exception:
        stream.close()
        raise


def read_headers(stream):
    data = bytearray()
    deadline = time.monotonic() + HEADER_TIMEOUT
    while b"\r\n\r\n" not in data:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError()
        stream.settimeout(remaining)
        chunk = stream.recv(HEADER_LIMIT + 1 - len(data))
        if not chunk:
            raise ValueError("incomplete_headers")
        data.extend(chunk)
        if len(data) > HEADER_LIMIT:
            raise ValueError("headers_too_large")
    headers, pending = bytes(data).split(b"\r\n\r\n", 1)
    return headers, pending


def request_status(headers, host):
    lines = headers.split(b"\r\n")
    if lines[0].split(b" ", 1)[0] != b"CONNECT":
        return 405
    allowed = [f"CONNECT {host}:443 HTTP/{version}".encode("ascii")
               for version in ("1.0", "1.1")]
    if lines[0] not in allowed:
        return 403
    for line in lines[1:]:
        name, sep, value = line.partition(b":")
        if (not sep or not re.fullmatch(rb"[!#$%&'*+.^_`|~0-9A-Za-z-]+", name)
                or any(byte < 32 and byte != 9 or byte == 127 for byte in value)):
            return 400
        if name.lower() in (b"proxy-authorization", b"proxy-authenticate", b"authorization"):
            return 403
    return 200


def reply(stream, status):
    reasons = {200: "Connection Established", 400: "Bad Request", 403: "Forbidden",
               405: "Method Not Allowed", 502: "Bad Gateway"}
    stream.settimeout(HEADER_TIMEOUT)
    extra = "" if status == 200 else "Connection: close\r\nContent-Length: 0\r\n"
    stream.sendall(f"HTTP/1.1 {status} {reasons[status]}\r\n{extra}\r\n".encode("ascii"))


def relay(client, upstream, pending):
    client.settimeout(IDLE_TIMEOUT)
    upstream.settimeout(IDLE_TIMEOUT)
    if pending:
        upstream.sendall(pending)
    deadline = time.monotonic() + IDLE_TIMEOUT
    with selectors.DefaultSelector() as selector:
        selector.register(client, selectors.EVENT_READ, upstream)
        selector.register(upstream, selectors.EVENT_READ, client)
        while selector.get_map():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                return
            events = selector.select(remaining)
            if not events:
                return
            for key, _ in events:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    return
                key.fileobj.settimeout(remaining)
                chunk = key.fileobj.recv(65536)
                if not chunk:
                    selector.unregister(key.fileobj)
                    key.data.shutdown(socket.SHUT_WR)
                else:
                    key.data.settimeout(max(0.001, deadline - time.monotonic()))
                    key.data.sendall(chunk)
                    deadline = time.monotonic() + IDLE_TIMEOUT


def serve_connection(client, config):
    established = False
    try:
        headers, pending = read_headers(client)
        status = request_status(headers, config["host"])
        if status != 200:
            reply(client, status)
            return
        with connect(config) as upstream:
            reply(client, 200)
            established = True
            relay(client, upstream, pending)
    except (OSError, ValueError):
        if not established:
            try:
                reply(client, 502)
            except OSError:
                pass


class Handler(socketserver.BaseRequestHandler):
    def handle(self):
        serve_connection(self.request, self.server.config)


class Proxy(socketserver.ThreadingTCPServer):
    daemon_threads = True
    allow_reuse_address = False

    def __init__(self, config):
        self.config = validate_config(config)
        self.slots = threading.BoundedSemaphore(MAX_CONNECTIONS)
        super().__init__(LISTEN, Handler)

    def process_request(self, request, client_address):
        if not self.slots.acquire(blocking=False):
            self.shutdown_request(request)
            return
        try:
            super().process_request(request, client_address)
        except Exception:
            self.slots.release()
            raise

    def process_request_thread(self, request, client_address):
        try:
            super().process_request_thread(request, client_address)
        finally:
            self.slots.release()

    def handle_error(self, request, client_address):
        # No tracebacks: exception messages may contain request or secret data.
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True)
    args = parser.parse_args()
    try:
        config = load_config(args.config)
        if sys.platform != "darwin":
            raise ValueError("darwin_required")
        socket.if_nametoindex("en0")
        with Proxy(config) as server:
            print("MODEL_CONNECT_PROXY_READY 127.0.0.1:19483", flush=True)
            server.serve_forever()
    except KeyboardInterrupt:
        return 0
    except Exception:
        print("MODEL_CONNECT_PROXY_FAILED", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

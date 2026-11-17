#!/usr/bin/env python3
"""Fault injecting TCP proxy for the L3 fault suite (10 section 12.5).

Sits between the engine and an HTTP dependency (the Docling sidecar today, an
S3 endpoint for the s3 backend) and misbehaves on command. Stdlib only.

  faultproxy.py --listen 127.0.0.1:7600 --upstream 127.0.0.1:7481 --control 127.0.0.1:7601

Modes (set with `POST /mode?name=<mode>[&n=<count>][&ms=<delay>]` on the control port,
read with `GET /mode`, counters with `GET /stats`):

  pass        forward bytes both ways (default)
  status      answer every request with HTTP `status` (default 503) without asking
              upstream; `n` limits it to that many requests, then back to pass
  reset       close every new connection at once (connection reset)
  blackhole   accept, read, never answer (the client's own timeout fires)
  slow        forward, but wait `ms` before each upstream connect

The proxy reads only the request line and headers in `status` mode; the body
is not consumed, like a server that answers early.
"""

import argparse
import asyncio
import json
import sys
from urllib.parse import parse_qs, urlparse

STATE = {"mode": "pass", "status": 503, "n": None, "ms": 0, "served": 0, "faulted": 0, "connections": 0}


def hostport(text):
    host, port = text.rsplit(":", 1)
    return host, int(port)


async def pipe(reader, writer):
    try:
        while True:
            chunk = await reader.read(65536)
            if not chunk:
                break
            writer.write(chunk)
            await writer.drain()
    except (ConnectionError, asyncio.CancelledError):
        pass
    finally:
        try:
            writer.close()
        except Exception:
            pass


def take_fault():
    """True when this request should be faulted; counts a limited mode down."""
    if STATE["mode"] == "pass":
        return False
    if STATE["mode"] == "status" and STATE["n"] is not None:
        if STATE["n"] <= 0:
            STATE["mode"] = "pass"
            return False
        STATE["n"] -= 1
    STATE["faulted"] += 1
    return True


async def handle(upstream, reader, writer):
    STATE["connections"] += 1
    mode = STATE["mode"]
    if mode == "reset":
        take_fault()
        writer.transport.abort()
        return
    if mode == "blackhole":
        take_fault()
        try:
            await reader.read(1 << 20)
            await asyncio.sleep(3600)
        except Exception:
            pass
        writer.close()
        return
    if mode == "status" and take_fault():
        try:
            await reader.readuntil(b"\r\n\r\n")
        except Exception:
            pass
        body = b'{"error":{"code":"FAULT_INJECTED"}}'
        head = (f"HTTP/1.1 {STATE['status']} Injected\r\nContent-Type: application/json\r\n"
                f"Content-Length: {len(body)}\r\nConnection: close\r\n\r\n").encode()
        writer.write(head + body)
        try:
            await writer.drain()
        except ConnectionError:
            pass
        writer.close()
        return
    if mode == "slow":
        await asyncio.sleep(STATE["ms"] / 1000)
    try:
        ur, uw = await asyncio.open_connection(*upstream)
    except OSError:
        writer.transport.abort()
        return
    STATE["served"] += 1
    await asyncio.gather(pipe(reader, uw), pipe(ur, writer))


async def control(reader, writer):
    try:
        head = await reader.readuntil(b"\r\n\r\n")
        line = head.split(b"\r\n", 1)[0].decode()
        _method, target, _ = line.split(" ", 2)
        url = urlparse(target)
        q = {k: v[0] for k, v in parse_qs(url.query).items()}
        if url.path == "/mode" and q.get("name"):
            STATE["mode"] = q["name"]
            STATE["status"] = int(q.get("status", 503))
            STATE["n"] = int(q["n"]) if "n" in q else None
            STATE["ms"] = int(q.get("ms", 0))
        body = json.dumps(STATE).encode()
        writer.write(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n"
                     + f"Content-Length: {len(body)}\r\nConnection: close\r\n\r\n".encode() + body)
        await writer.drain()
    except Exception:
        pass
    writer.close()


async def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--listen", required=True)
    ap.add_argument("--upstream", required=True)
    ap.add_argument("--control", required=True)
    a = ap.parse_args()
    up = hostport(a.upstream)
    srv = await asyncio.start_server(lambda r, w: handle(up, r, w), *hostport(a.listen))
    ctl = await asyncio.start_server(control, *hostport(a.control))
    print("faultproxy ready", flush=True)
    async with srv, ctl:
        await asyncio.gather(srv.serve_forever(), ctl.serve_forever())


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        sys.exit(0)

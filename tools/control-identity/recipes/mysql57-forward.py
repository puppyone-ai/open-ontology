#!/usr/bin/env python3
"""TLS-transparent loopback transport for the isolated mysql57 identity fixture."""
import socket
import threading

PORT = 18447


def copy(source, target):
    try:
        while chunk := source.recv(65536):
            target.sendall(chunk)
    except OSError:
        pass
    finally:
        try:
            target.shutdown(socket.SHUT_WR)
        except OSError:
            pass


def handle(client):
    try:
        with client, socket.create_connection(("host.docker.internal", PORT), timeout=120) as upstream:
            worker = threading.Thread(target=copy, args=(client, upstream))
            worker.start()
            copy(upstream, client)
            worker.join()
    except OSError:
        pass


if __name__ == "__main__":
    with socket.socket() as server:
        server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        server.bind(("127.0.0.1", PORT))
        server.listen(32)
        while True:
            client, _ = server.accept()
            threading.Thread(target=handle, args=(client,), daemon=True).start()

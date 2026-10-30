import contextlib
import errno
import io
import json
import os
from pathlib import Path
import socket
import tempfile
import threading
import unittest
from unittest.mock import Mock, patch

import model_connect_proxy as proxy


CONFIG = {"host": "model.example.com", "port": 443, "ipv4": "8.8.8.8",
          "interface": "en0", "listen_host": "127.0.0.1", "listen_port": 19483}
SECRET = b"secret-api-key-do-not-log"


class ProxyTests(unittest.TestCase):
    def exchange(self, request, connector=None):
        client, worker = socket.socketpair()
        output = io.StringIO()

        def run():
            with worker:
                proxy.serve_connection(worker, CONFIG)

        with patch.object(proxy, "connect", connector or Mock()) as dial:
            with contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
                thread = threading.Thread(target=run, daemon=True)
                thread.start()
                with client:
                    client.settimeout(2)
                    client.sendall(request)
                    try:
                        client.shutdown(socket.SHUT_WR)
                    except OSError as error:
                        # A rejected peer can close before this half-close on macOS.
                        if error.errno != errno.ENOTCONN:
                            raise
                    response = b""
                    while True:
                        chunk = client.recv(65536)
                        if not chunk:
                            break
                        response += chunk
                thread.join(2)
                self.assertFalse(thread.is_alive())
        self.assertEqual(output.getvalue(), "")
        self.assertNotIn(SECRET, response)
        return response, dial

    def test_reject_destinations_methods_and_credentials(self):
        for line in (b"CONNECT other.example.com:443 HTTP/1.1",
                     b"CONNECT model.example.com:80 HTTP/1.1",
                     b"CONNECT 127.0.0.1:443 HTTP/1.1",
                     b"CONNECT user:pass@model.example.com:443 HTTP/1.1",
                     b"GET https://model.example.com/ HTTP/1.1"):
            with self.subTest(line=line):
                response, dial = self.exchange(line + b"\r\nX-Secret: " + SECRET + b"\r\n\r\n")
                self.assertRegex(response, rb"HTTP/1.1 (403|405)")
                dial.assert_not_called()
        response, dial = self.exchange(
            b"CONNECT model.example.com:443 HTTP/1.1\r\nProxy-Authorization: "
            + SECRET + b"\r\n\r\n")
        self.assertIn(b"403", response)
        dial.assert_not_called()

    def test_reject_nonpublic_pins_and_config_overrides(self):
        for address in ("127.0.0.1", "10.0.0.1", "192.168.1.1", "169.254.1.1",
                        "100.64.0.1", "198.18.0.1", "0.0.0.0", "224.0.0.1",
                        "240.0.0.1", "::1", "example.com"):
            with self.subTest(address=address), self.assertRaises(ValueError):
                proxy.validate_config({**CONFIG, "ipv4": address})
        for override in ({"port": 80}, {"interface": "utun0"},
                         {"listen_host": "0.0.0.0"}, {"listen_port": 8080},
                         {"host": "127.0.0.1"}, {"api_key": "secret"}):
            with self.assertRaises(ValueError):
                proxy.validate_config({**CONFIG, **override})

    def test_private_config_permissions(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "config.json"
            path.write_text(json.dumps(CONFIG))
            os.chmod(path, 0o600)
            self.assertEqual(proxy.load_config(path), CONFIG)
            os.chmod(path, 0o644)
            with self.assertRaises(ValueError):
                proxy.load_config(path)
            link = Path(directory) / "link"
            link.symlink_to(path)
            with self.assertRaises(OSError):
                proxy.load_config(link)

    def test_headers_are_bounded_and_errors_do_not_leak(self):
        response, dial = self.exchange(b"CONNECT " + SECRET + b"x" * proxy.HEADER_LIMIT)
        self.assertIn(b"502", response)
        dial.assert_not_called()
        response, _ = self.exchange(
            b"CONNECT model.example.com:443 HTTP/1.1\r\n\r\n",
            Mock(side_effect=OSError(SECRET.decode())))
        self.assertIn(b"502", response)

    def test_tunnel_preserves_bytes_and_half_close(self):
        upstream, peer = socket.socketpair()
        payload = b"\x16\x03\x01opaque-encrypted-test-record-" + SECRET
        received = []

        def remote():
            with peer:
                peer.settimeout(2)
                data = b""
                while True:
                    chunk = peer.recv(65536)
                    if not chunk:
                        break
                    data += chunk
                received.append(data)
                peer.sendall(b"\x17\x03\x03reply")

        thread = threading.Thread(target=remote, daemon=True)
        thread.start()
        response, dial = self.exchange(
            b"CONNECT model.example.com:443 HTTP/1.1\r\nX-Secret: " + SECRET
            + b"\r\n\r\n" + payload, Mock(return_value=upstream))
        thread.join(2)
        self.assertFalse(thread.is_alive())
        self.assertEqual(received, [payload])
        self.assertEqual(response, b"HTTP/1.1 200 Connection Established\r\n\r\n\x17\x03\x03reply")
        dial.assert_called_once_with(CONFIG)

    def test_header_and_idle_timeouts(self):
        client, worker = socket.socketpair()
        with client, worker, patch.object(proxy, "HEADER_TIMEOUT", 0.02):
            client.sendall(b"CONNECT ")
            with self.assertRaises(TimeoutError):
                proxy.read_headers(worker)
        client, worker = socket.socketpair()
        upstream, peer = socket.socketpair()
        with client, worker, upstream, peer, patch.object(proxy, "IDLE_TIMEOUT", 0.02):
            proxy.relay(worker, upstream, b"")

    def test_slow_header_deadline_is_not_reset_by_each_byte(self):
        stream = Mock()
        stream.recv.return_value = b"C"
        with patch.object(proxy.time, "monotonic", side_effect=[0, 1, 4, 6]):
            with self.assertRaises(TimeoutError):
                proxy.read_headers(stream)
        self.assertEqual(stream.recv.call_count, 2)

    def test_fixed_loopback_listener_and_sanitized_startup_failure(self):
        with patch.object(proxy.socketserver.ThreadingTCPServer, "__init__",
                          return_value=None) as initialize:
            proxy.Proxy(CONFIG)
        initialize.assert_called_once_with(("127.0.0.1", 19483), proxy.Handler)
        output = io.StringIO()
        with patch.object(proxy.sys, "argv", ["proxy", "--config", "private.json"]):
            with patch.object(proxy, "load_config", side_effect=ValueError(SECRET.decode())):
                with contextlib.redirect_stderr(output):
                    self.assertEqual(proxy.main(), 1)
        self.assertEqual(output.getvalue(), "MODEL_CONNECT_PROXY_FAILED\n")

    @patch.object(proxy.sys, "platform", "darwin")
    @patch.object(proxy.socket, "if_nametoindex", return_value=14)
    @patch.object(proxy.socket, "socket")
    def test_fixed_direct_socket_and_failure_cleanup(self, factory, index):
        stream = factory.return_value
        self.assertIs(proxy.connect(CONFIG), stream)
        index.assert_called_once_with("en0")
        stream.setsockopt.assert_called_once_with(socket.IPPROTO_IP, 25, 14)
        stream.settimeout.assert_called_once_with(proxy.CONNECT_TIMEOUT)
        stream.connect.assert_called_once_with(("8.8.8.8", 443))
        stream.connect.side_effect = OSError("connection failure")
        with self.assertRaises(OSError):
            proxy.connect(CONFIG)
        stream.close.assert_called_once()


if __name__ == "__main__":
    unittest.main()

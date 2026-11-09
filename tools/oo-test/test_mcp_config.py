"""Generated configs -> native CLI -> MCP bridge -> local HTTPS fixture.

This tests the installed transport, not Gateway publication or QS-04.
Set OO_TEST_BIN_DIR to a directory containing oo and mcp-gateway.
"""
import http.server
import json
import os
from pathlib import Path
import secrets
import ssl
import subprocess
import tempfile
import threading
import tomllib
import unittest


class GeneratedConfig(unittest.TestCase):
    @unittest.skipUnless(os.environ.get("OO_TEST_BIN_DIR"), "OO_TEST_BIN_DIR required")
    def test_all_agents_start_authenticated_bridge(self):
        binary = Path(os.environ["OO_TEST_BIN_DIR"]).resolve()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            token = "ooct_" + secrets.token_urlsafe(32)
            (root / "credential").write_text(token)
            (root / "credential").chmod(0o600)
            (root / "openssl.cnf").write_text(
                "[req]\ndistinguished_name=dn\nx509_extensions=ext\nprompt=no\n"
                "[dn]\nCN=localhost\n[ext]\nsubjectAltName=DNS:localhost\n"
                "basicConstraints=critical,CA:TRUE\n"
                "keyUsage=critical,digitalSignature,keyEncipherment,keyCertSign\n"
            )
            subprocess.run(
                ["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
                 "-keyout", str(root / "ca-key.pem"), "-out", str(root / "ca.pem"),
                 "-days", "1", "-config", str(root / "openssl.cnf")],
                check=True, capture_output=True,
            )
            (root / "leaf.cnf").write_text(
                "subjectAltName=DNS:localhost\nbasicConstraints=critical,CA:FALSE\n"
                "keyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n"
            )
            subprocess.run(
                ["openssl", "req", "-new", "-newkey", "rsa:2048", "-nodes",
                 "-keyout", str(root / "key.pem"), "-out", str(root / "leaf.csr"),
                 "-subj", "/CN=localhost"], check=True, capture_output=True,
            )
            subprocess.run(
                ["openssl", "x509", "-req", "-in", str(root / "leaf.csr"),
                 "-CA", str(root / "ca.pem"), "-CAkey", str(root / "ca-key.pem"),
                 "-CAcreateserial", "-days", "1", "-out", str(root / "leaf.pem"),
                 "-extfile", str(root / "leaf.cnf")], check=True, capture_output=True,
            )
            requests = []

            class Handler(http.server.BaseHTTPRequestHandler):
                def log_message(self, *_args):
                    pass

                def do_GET(self):
                    self.reply()

                def do_POST(self):
                    self.reply()

                def reply(self):
                    if self.headers.get("Authorization") != "Bearer " + token:
                        self.send_error(401)
                        return
                    body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
                    requests.append((self.command, self.path, body))
                    if self.path == "/v1/check/readiness":
                        result = dict(ready=True, pack_published=True, gateway_reachable=True,
                                      visible_tables=1, freshness=dict(publication="live"))
                    elif self.path == "/v1/query":
                        result = dict(query_id="fixture-query", rows=[["1"]])
                    else:
                        self.send_error(404)
                        return
                    payload = json.dumps(result).encode()
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(payload)))
                    self.end_headers()
                    self.wfile.write(payload)

            server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
            context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            context.load_cert_chain(root / "leaf.pem", root / "key.pem")
            server.socket = context.wrap_socket(server.socket, server_side=True)
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                profiles = root / "profiles"
                profiles.mkdir(mode=0o700)
                profile = profiles / "demo.json"
                profile.write_text(json.dumps(dict(
                    schema_version=1, gateway=f"https://localhost:{server.server_port}",
                    token_file=str(root / "credential"), ca_file=str(root / "ca.pem"),
                    connect_ip="127.0.0.1",
                )))
                profile.chmod(0o600)
                env = dict(os.environ, OO_CONFIG_DIR=str(root),
                           PATH=str(binary) + os.pathsep + os.environ.get("PATH", ""))
                for agent in ("claude", "cursor", "codex"):
                    with self.subTest(agent=agent):
                        rendered = subprocess.check_output(
                            [binary / "oo", "mcp-config", "--agent", agent, "--profile", "demo"],
                            env=env, text=True,
                        )
                        self.assertTrue(token not in rendered, "credential leaked into config")
                        config = (tomllib.loads(rendered)["mcp_servers"] if agent == "codex"
                                  else json.loads(rendered)["mcpServers"])["open-ontology"]
                        messages = [
                            dict(jsonrpc="2.0", id=1, method="initialize", params=dict(
                                protocolVersion="2025-11-25", capabilities={},
                                clientInfo=dict(name="config-test", version="1"))),
                            dict(jsonrpc="2.0", method="notifications/initialized"),
                            dict(jsonrpc="2.0", id=2, method="tools/call",
                                 params=dict(name="check_readiness", arguments={})),
                            dict(jsonrpc="2.0", id=3, method="tools/call",
                                 params=dict(name="query", arguments=dict(sql="SELECT 1"))),
                        ]
                        # Keep stdin open until responses arrive; EOF cancels in-flight tools.
                        process = subprocess.Popen(
                            [config["command"], *config["args"]], env=env,
                            stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, text=True,
                        )
                        try:
                            import selectors
                            selector = selectors.DefaultSelector()
                            selector.register(process.stdout, selectors.EVENT_READ)
                            for message in messages:
                                process.stdin.write(json.dumps(message) + "\n")
                                process.stdin.flush()
                                if "id" not in message:
                                    continue
                                self.assertTrue(selector.select(15), "MCP response timeout")
                                response = json.loads(process.stdout.readline())
                                self.assertEqual(response["id"], message["id"])
                                self.assertNotIn("error", response)
                                self.assertFalse(response.get("result", {}).get("isError", False))
                                if message["id"] == 2:
                                    self.assertIn('"ready":true', json.dumps(response).replace(" ", "").replace("\\", ""))
                                if message["id"] == 3:
                                    self.assertIn("fixture-query", json.dumps(response))
                            selector.close()
                        finally:
                            process.terminate()
                            process.communicate(timeout=10)
                self.assertEqual(sum(path == "/v1/check/readiness" for _, path, _ in requests), 3)
                self.assertEqual(sum(path == "/v1/query" for _, path, _ in requests), 3)
                for method, path, body in requests:
                    if path == "/v1/query":
                        self.assertEqual(method, "POST")
                        self.assertEqual(json.loads(body)["sql"], "SELECT 1")
            finally:
                server.shutdown()
                server.server_close()
                thread.join()


if __name__ == "__main__":
    unittest.main()

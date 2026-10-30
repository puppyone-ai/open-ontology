# Model-only CONNECT proxy

Optional operator-owned macOS workaround, not a confirmed model reliability fix.
No dependencies, API keys, model calls, global proxy changes, Gateway changes,
public listener, TLS interception, or automatic DNS/pin refresh.

## Private configuration

Create a JSON file outside the repository in an operator-only directory (directory
mode 0700, file mode 0600). The file must be owned by the running user and cannot
be a symlink. Exactly these fields are accepted:

```json
{
  "host": "REPLACE_WITH_MODEL_ENDPOINT_HOSTNAME",
  "port": 443,
  "ipv4": "REPLACE_WITH_OPERATOR_VERIFIED_PUBLIC_IPV4",
  "interface": "en0",
  "listen_host": "127.0.0.1",
  "listen_port": 19483
}
```

Use the model endpoint URL's `.hostname` (lowercase ASCII/punycode), without
scheme, path, credentials or port. Both placeholders intentionally fail
validation. The operator must verify the pin independently of TUN fake DNS;
private, loopback, link-local, shared, benchmark/fake-DNS, multicast and reserved
addresses are rejected. Never put credentials or API keys in this file.

## Run and stop

From the repository root, in a dedicated terminal:

```sh
python3 product/tools/agent-contract/model_connect_proxy.py \
  --config "$HOME/.config/model-connect/private.json"
```

Only after the ready message, prefix **only the intended model process**:

```sh
env -u ALL_PROXY -u all_proxy -u HTTP_PROXY -u http_proxy \
  -u https_proxy -u no_proxy HTTPS_PROXY=http://127.0.0.1:19483 \
  NO_PROXY=localhost,127.0.0.1,::1 your-model-command
```

Replace `your-model-command` with the operator's existing model invocation.
Do not export these variables globally or wrap unrelated processes. The proxy
URL is `http://`: it carries HTTPS via CONNECT, not TLS to the proxy itself.
The original SDK retains the original HTTPS hostname, TLS SNI and certificate
verification. Do not replace the endpoint hostname with the pin or disable TLS
verification. No authorization headers or other CONNECT headers are forwarded;
proxy credentials (and Authorization on CONNECT) are rejected.

Per the integration contract, the Pi model SDK consumes process-local
`HTTPS_PROXY`; the Gateway Rust SDK's `.no_proxy()` remains unchanged. This
implementation does not validate either SDK integration. Child processes inherit
the model process environment; keep that process tree deliberately scoped.

Ctrl-C stops the proxy and closes active tunnels. End the prefixed model command
to remove its proxy environment. There is no installed service or persistent
route setting. Any pin rotation requires operator verification and restart.

## Boundaries and checks

- Only exact `CONNECT configured-host:443 HTTP/1.0` or `HTTP/1.1` is accepted;
  other hosts, ports, absolute URLs and GET forwarding are refused.
- Outbound TCP always uses the one numeric IPv4 pin, port 443 and per-socket
  Darwin `IP_BOUND_IF=25` on `en0`. No resolver or proxy fallback is used.
- Headers: 8192 bytes and 5 seconds total. Connect: 10 seconds. Tunnel inactivity
  and blocked writes: 120 seconds. At most 16 active connections.
- Tunnel bytes are opaque and bounded-buffer relayed, with TCP half-close support.
  The proxy does not decrypt, validate or inspect application/TLS traffic.
- Output contains only fixed startup/failure messages, never request headers,
  exception details, credentials or stream contents.
- Loopback is **not** a per-user access control: other local processes can reach
  this fixed-destination unauthenticated tunnel. Use only on a trusted workstation;
  it cannot enforce which process connects or which TLS SNI that process sends.

Local-only tests (socket pairs and mocked outbound sockets; no model or public
network connection):

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover \
  -s product/tools/agent-contract -p test_model_connect_proxy.py -v
```

These tests establish guardrails and byte forwarding, not real TLS, live
`en0` routing, SDK compatibility or model timeout recovery. The parent/operator
must perform the actual model probe and decide whether to adopt this workaround.

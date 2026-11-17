"""The doc-parser container has no way out (10 §2.7, B-S3-01).

Brings up deploy/docs/compose.yaml (doc-parser only) with the lane label,
then, from inside the container, tries outbound TCP and DNS and expects them to
fail, while a sibling on the same internal network can still reach the service.
Needs docker compose and the image (`OO_DOCS_PARSER_IMAGE`, default
`oo-docs-parser:dev`). Removes everything it created.

    python3 -B -m pytest -q deploy/docs/test_no_egress.py
"""
import ipaddress
import json
import os
import subprocess
import tempfile
import time
import uuid
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parent
LANE = os.environ.get("OO_DOCS_LANE", "super-ontology-be2")
IMAGE = os.environ.get("OO_DOCS_PARSER_IMAGE", "oo-docs-parser:dev")


def sh(*args, check=True, timeout=120, env=None):
    r = subprocess.run(args, capture_output=True, text=True, timeout=timeout, env=env)
    if check and r.returncode:
        raise AssertionError(f"{' '.join(args)}\n{r.stdout}\n{r.stderr}")
    return r


def free_subnet():
    """A /28 that overlaps no existing docker network. Only needed on hosts whose
    default address pools are exhausted; read-only inspection of networks."""
    ids = sh("docker", "network", "ls", "-q").stdout.split()
    used = []
    if ids:
        for net in json.loads(sh("docker", "network", "inspect", *ids).stdout):
            for cfg in (net.get("IPAM") or {}).get("Config") or []:
                if cfg.get("Subnet"):
                    used.append(ipaddress.ip_network(cfg["Subnet"], strict=False))
    for n in range(0, 250):
        cand = ipaddress.ip_network(f"10.249.{n}.0/28")
        if not any(cand.overlaps(u) for u in used if u.version == 4):
            return str(cand)
    pytest.skip("no free subnet for the test network")


@pytest.fixture(scope="module")
def stack():
    if subprocess.run(["docker", "image", "inspect", IMAGE], capture_output=True).returncode:
        pytest.skip(f"image {IMAGE} is not built")
    project = f"oo-docs-egress-{uuid.uuid4().hex[:8]}"
    state = tempfile.mkdtemp(prefix="oo-docs-state-")
    token_file = Path(state) / "parser-token"
    token_file.write_text("egress-test-token\n")
    token_file.chmod(0o644)
    env = dict(os.environ, OO_DOCS_PROJECT=project, OO_DOCS_LANE=LANE, OO_DOCS_STATE=state, OO_DOCS_PARSER_IMAGE=IMAGE)
    # The compose file leaves addressing to docker; the test pins a free subnet so
    # it also runs on hosts with exhausted default pools.
    override = Path(state) / "override.yaml"
    override.write_text(f"networks:\n  docs-internal:\n    ipam:\n      config:\n        - subnet: {free_subnet()}\n")
    compose = ["docker", "compose", "-f", str(HERE / "compose.yaml"), "-f", str(override), "-p", project]
    try:
        sh(*compose, "up", "-d", "--wait", "--wait-timeout", "120", "doc-parser", env=env, timeout=180)
        yield {"project": project, "compose": compose, "env": env, "service": "doc-parser"}
    finally:
        subprocess.run([*compose, "down", "-v", "--remove-orphans"], env=env, capture_output=True, timeout=120)
        subprocess.run(["rm", "-rf", state])


def exec_py(stack, code, check=False):
    return sh(*stack["compose"], "exec", "-T", stack["service"], "python", "-c", code, env=stack["env"], check=check)


def test_outbound_tcp_and_dns_fail_from_inside_the_container(stack):
    code = (
        "import socket,sys\n"
        "for host,port in [('1.1.1.1',443),('8.8.8.8',53),('93.184.216.34',80)]:\n"
        "    s=socket.socket(); s.settimeout(4)\n"
        "    try:\n"
        "        s.connect((host,port)); print('CONNECTED',host); sys.exit(3)\n"
        "    except OSError as e:\n"
        "        print('blocked',host,type(e).__name__)\n"
        "    finally:\n"
        "        s.close()\n"
        "try:\n"
        "    socket.getaddrinfo('example.com',443); print('RESOLVED'); sys.exit(4)\n"
        "except OSError as e:\n"
        "    print('dns blocked',type(e).__name__)\n"
    )
    r = exec_py(stack, code)
    assert r.returncode == 0, r.stdout + r.stderr
    assert "CONNECTED" not in r.stdout and "RESOLVED" not in r.stdout
    assert r.stdout.count("blocked") >= 4


def test_internal_peers_reach_it_with_a_token_only(stack):
    net = f"{stack['project']}_docs-internal"
    probe = (
        "import urllib.request as u, urllib.error as e\n"
        "print(u.urlopen('http://doc-parser:7481/healthz',timeout=5).status)\n"
        "try:\n"
        "    u.urlopen('http://doc-parser:7481/v1/info',timeout=5)\n"
        "except e.HTTPError as x:\n"
        "    print(x.code)\n"
        "rq=u.Request('http://doc-parser:7481/v1/info',headers={'Authorization':'Bearer egress-test-token'})\n"
        "print(u.urlopen(rq,timeout=5).status)\n"
    )
    r = sh("docker", "run", "--rm", "--label", f"oo.lane={LANE}", "--network", net, "--entrypoint", "python", IMAGE, "-c", probe)
    assert r.stdout.split() == ["200", "401", "200"], r.stdout + r.stderr


def test_compose_hardening_and_labels(stack):
    cid = sh(*stack["compose"], "ps", "-q", stack["service"], env=stack["env"]).stdout.strip()
    info = json.loads(sh("docker", "inspect", cid).stdout)[0]
    hc, cfg = info["HostConfig"], info["Config"]
    assert cfg["Labels"]["oo.lane"] == LANE
    assert cfg["User"] == "10001:10001"
    assert hc["ReadonlyRootfs"] is True
    assert "ALL" in hc["CapDrop"] and "no-new-privileges:true" in hc["SecurityOpt"]
    assert hc["Memory"] == 3 * 1024**3 and hc["PidsLimit"] == 128 and hc["NanoCpus"] == 2_000_000_000
    assert not info["NetworkSettings"]["Ports"].get("7481/tcp"), "the port must not be published"
    nets = list(info["NetworkSettings"]["Networks"])
    assert nets == [f"{stack['project']}_docs-internal"], nets
    net = json.loads(sh("docker", "network", "inspect", nets[0]).stdout)[0]
    assert net["Internal"] is True and net["Labels"]["oo.lane"] == LANE
    mounts = {m["Destination"]: m for m in info["Mounts"]}
    assert mounts["/run/oo/parser-token"]["RW"] is False
    vol = mounts["/scratch"]["Name"]
    assert json.loads(sh("docker", "volume", "inspect", vol).stdout)[0]["Labels"]["oo.lane"] == LANE
    assert info["State"]["Health"]["Status"] == "healthy"

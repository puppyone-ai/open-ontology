#!/usr/bin/env python3
"""S4-M-02 final L1 driver: runs tools/oo-test/l1_clean.py (or l1_ssh.py) unmodified from the
release-candidate commit, with these run-time substitutions only:
  * lane label oo.lane=l1 (harness default a4) for every container/volume/network and the client image;
  * the npm tarball is the stage.mjs-staged + npm-packed release-candidate tarball (--tarball), not the
    harness's diagnostic pack;
  * the Agent answers the supported gold questions in every flow (harness: stack flow only);
  * a one-token model health probe (three consecutive successes, up to 30 min) precedes each Agent run.
"""
import hashlib, json, os, shutil, subprocess, sys, time
from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/oo-test"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
import l1_clean as l1  # noqa: E402
import t1  # noqa: E402
import l1_probe as probe  # noqa: E402

LANE = "l1"
l1.LANE = t1.LANE = LANE
l1.AGENT_IMAGE = "oo-l1-client:linux-amd64"


def take(flag):
    i = sys.argv.index(flag)
    value = sys.argv[i + 1]
    del sys.argv[i:i + 2]
    return value

TARBALL = Path(take("--tarball")).resolve()
SSH = "--ssh" in sys.argv
if SSH:
    sys.argv.remove("--ssh")


def staged_pack(binary, output, tools, extra=None):
    image = binary.with_name("oo-lite-image").read_text().strip()
    dest = output / TARBALL.name
    shutil.copy2(TARBALL, dest)
    data = dest.read_bytes()
    l1.write_json(output / "staged-tarball.json", {
        "source": "packages/cli/scripts/stage.mjs + npm pack (release-candidate staging)",
        "filename": TARBALL.name, "sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data),
        "image_pin": image})
    return dest, image

l1.diagnostic_pack = staged_pack


def ensure_client_image(output):
    if subprocess.run(["docker", "image", "inspect", l1.AGENT_IMAGE], capture_output=True).returncode:
        with (output / "client-image.log").open("wb") as log:
            subprocess.run(["docker", "build", "--platform", "linux/amd64", "--build-arg", "OO_LANE=" + LANE,
                            "--label", "oo.lane=" + LANE, "-t", l1.AGENT_IMAGE, "-f",
                            l1.ROOT / "tools/oo-test/linux-amd64/client.Dockerfile",
                            l1.ROOT / "tools/oo-test/linux-amd64"], stdout=log, stderr=subprocess.STDOUT, check=True)
    return l1.AGENT_IMAGE

l1.ensure_client_image = ensure_client_image

_agent = l1.agent


def agent(prompt, output, source, machine, questions_too):
    output.mkdir(exist_ok=True)
    health = probe.healthy(output / "model-health-probe.json")
    if not health["healthy"]:
        report = {"error": "MODEL_HEALTH_PROBE_FAILED", "questions": [], "correct": 0, "asked": 0,
                  "binding": "not_run", "probe": {"healthy": False, "attempts": len(health["attempts"])}}
        l1.write_json(output / "result.json", report)
        return report
    report = _agent(prompt, output, source, machine, l1.AGENT)
    report["probe"] = {"healthy": True, "attempts": len(health["attempts"])}
    l1.write_json(output / "result.json", report)
    return report

l1.agent = agent

if __name__ == "__main__":
    if SSH:
        import l1_ssh
        l1_ssh.a2.LABEL = "oo.lane=" + LANE
        raise SystemExit(l1_ssh.main())
    raise SystemExit(l1.main())

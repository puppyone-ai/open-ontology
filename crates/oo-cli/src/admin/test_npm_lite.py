#!/usr/bin/env python3
"""Installed native CLI smoke test. Fake Docker; no database or release claim."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[4]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", required=True)
    args = parser.parse_args()
    assert re.fullmatch("[a-z0-9-]+", args.run_id)
    os.umask(0o077)
    out = ROOT / "evidence" / args.run_id
    out.mkdir(mode=0o700)
    home, tools, package = (out / name for name in ("home", "tools", "package"))
    for path in (home, tools, package):
        path.mkdir()
    for tool in ("node", "npm"):
        (tools / tool).symlink_to(shutil.which(tool))
    calls = out / "docker.calls"
    (tools / "docker").write_text(
        f"#!{sys.executable}\nimport sys\n"
        f"with open({str(calls)!r}, 'a') as f: f.write(' '.join(sys.argv[1:])+'\\n')\n"
        "raise SystemExit(0 if sys.argv[1]=='info' else 1)\n"
    )
    (tools / "docker").chmod(0o700)
    env = {"HOME": str(home), "PATH": f"{tools}:/usr/bin:/bin",
           "npm_config_cache": str(home / "npm-cache"), "npm_config_update_notifier": "false"}

    def run(name, argv, cwd=out):
        result = subprocess.run(list(map(str, argv)), cwd=cwd, env=env,
                                capture_output=True, timeout=60)
        (out / f"{name}.stdout").write_bytes(result.stdout)
        (out / f"{name}.stderr").write_bytes(result.stderr)
        return result

    for name in ("package.json", "README.md", "bin/oo.mjs", "bin/auth.mjs"):
        dest = package / name
        dest.parent.mkdir(exist_ok=True)
        shutil.copy2(ROOT / "packages/cli" / name, dest)
    for name in ("LICENSE", "NOTICE"):
        shutil.copy2(ROOT / name, package / name)
    # Deliberately synthetic. This fixture tests npm inclusion, not licensing.
    (package / "THIRD_PARTY_NOTICES.txt").write_text("Synthetic test notices; NOT a release artifact.\n")
    (package / "dependencies.json").write_text('{"schema_version":1,"dependencies":[]}\n')
    skill = (ROOT / "tools/agent-contract/business-answer-policy.md").read_bytes()
    (package / "SKILL.md").write_bytes(skill)
    target = {"Darwin": "darwin", "Linux": "linux"}[platform.system()] + "-" + {
        "arm64": "arm64", "aarch64": "arm64", "x86_64": "x64"}[platform.machine()]
    native = package / "native" / target / "oo"
    native.parent.mkdir(parents=True)
    shutil.copy2(ROOT / "target/debug/oo", native)
    reference = "registry.example/runtime@sha256:" + "a" * 64
    (native.parent / "oo-lite-image").write_text(reference + "\n")
    (package / "native/manifest.json").write_text(json.dumps({
        "schema_version": 1, "skill_sha256": hashlib.sha256(skill).hexdigest(),
        "binaries": {target: {"path": f"native/{target}/oo",
                             "sha256": hashlib.sha256(native.read_bytes()).hexdigest()}}}))
    packed = run("pack", [tools / "npm", "pack", "--json", "--ignore-scripts",
                          "--pack-destination", out], package)
    assert packed.returncode == 0
    inventory = json.loads(packed.stdout)[0]
    paths = {entry["path"] for entry in inventory["files"]}
    assert {f"native/{target}/oo-lite-image", "THIRD_PARTY_NOTICES.txt",
            "NOTICE", "dependencies.json"} <= paths
    prefix = home / "npm"
    installed = run("install", [tools / "npm", "install", "--global", "--prefix", prefix,
                                "--ignore-scripts", "--no-audit", "--no-fund", out / inventory["filename"]])
    assert installed.returncode == 0
    stack = out / "stack.yaml"
    shutil.copy2(ROOT / "crates/oo-stack/example.stack.json", stack)
    result = run("up", [prefix / "bin/oo", "up", "--config", stack, "--json"])
    assert result.returncode == 1
    message = json.loads(result.stdout)["message"]
    assert "LITE_IMAGES_MISSING" in message and f"docker pull {reference}" in message
    assert "docker pull docker.io/library/postgres@sha256:" in message
    assert "LITE_IMAGE_DIGEST_REQUIRED" not in message
    assert all(line.startswith(("info ", "image inspect ")) for line in calls.read_text().splitlines())
    assert not (out / ".oo").exists()
    (out / "result.json").write_text(json.dumps({
        "status": "passed", "installed_sidecar": True, "notice_inventory": True,
        "pinned_pull_guidance": True, "deployment_state_created": False,
        "scope": "diagnostic npm pack; fake Docker; synthetic notices; no databases or model calls",
        "containers_created": [],
    }, indent=2) + "\n")


if __name__ == "__main__":
    main()

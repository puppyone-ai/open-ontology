#!/usr/bin/env python3
"""L1 `--target ssh` runtime build (D16) against lane A2's sshd+Docker target.

The target is A2's masked systemd image (deploy/lite/test_remote_ssh.py): its
binfmt, sysctl and module units are masked and the runner checks the Docker
VM's kernel state before and after. Everything carries oo.lane=a4.

Flow: secrets enter a fresh HOME through the real `oo init` TTY wizard (SAVE);
a stack.yaml with deploy.target ssh is derived from it; `oo up` installs the
lite stack on the target, `oo build` runs every stage in the target's runtime
containers with owner review here (explicit decision file), the printed
prompt's own steps run through an SSH tunnel to the remote Gateway's loopback
port, and one scheduled data-change cycle must serve new data.

  python3 tools/oo-test/l1_ssh.py --run-id a4-ssh-RUN --binary target/debug/oo \\
      --model-endpoint ~/.config/open-ontology/secrets/model-endpoint-9r \\
      --model-key ~/.config/open-ontology/secrets/model-key-9r-sol
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
import l1_clean as l1  # noqa: E402

spec = importlib.util.spec_from_file_location("a2", ROOT / "deploy/lite/test_remote_ssh.py")
a2 = importlib.util.module_from_spec(spec)
spec.loader.exec_module(a2)
a2.LABEL = "oo.lane=" + l1.LANE


def remote_sql(target, project, service, sql):
    cid = target.remote(f"docker ps -q --filter label=com.docker.compose.project={project} "
                        f"--filter label=com.docker.compose.service={service}").strip()
    return target.exec("docker", "exec", "-i", cid, "psql", "-X", "-qAt", "-v", "ON_ERROR_STOP=1",
                       "-U", "postgres", stdin=sql.encode()).stdout.decode().strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--model-endpoint", type=Path, required=True)
    parser.add_argument("--model-key", type=Path, required=True)
    parser.add_argument("--engine", default="postgres", choices=sorted(l1.ENGINES))
    args = parser.parse_args()
    if not re.fullmatch(r"[a-z0-9-]+", args.run_id):
        parser.error("invalid run id")
    if shutil.disk_usage("/").free < 20 * 1024 ** 3:
        parser.error("less than 20 GB free; refusing to start containers")
    os.umask(0o077)
    binary = args.binary.resolve()
    image = binary.with_name("oo-lite-image").read_text().strip()
    out = ROOT / "evidence" / args.run_id
    out.mkdir(mode=0o700)
    private = out / ".private"
    private.mkdir(mode=0o700)
    home = out / "home"
    home.mkdir(mode=0o700)
    work = out / "work"
    work.mkdir()
    tools = out / "tools"
    tools.mkdir()
    for tool in ("docker",):
        (tools / tool).symlink_to(shutil.which(tool))
    (tools / "docker-config").mkdir()
    l1.write_json(tools / "docker-config/config.json", {"cliPluginsExtraDirs": [
        "/Applications/Docker.app/Contents/Resources/cli-plugins"]})
    env = l1.clean_env(home, tools, home / "npm")
    env["PATH"] = f"{binary.parent}:{env['PATH']}"
    for name in ("ssh_host_ed25519_key", "client"):
        a2.sh(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-C", "oo-a4", "-f", str(private / name)])
    (private / "sshd.conf").write_text("PermitRootLogin yes\nPasswordAuthentication no\n"
                                       "KbdInteractiveAuthentication no\nUsePAM yes\n")
    pin = a2.fingerprint(private / "ssh_host_ed25519_key.pub")
    report = {"run_id": args.run_id, "engine": args.engine, "runtime_image": image,
              "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "target": "lane A2 masked systemd+sshd+Docker image, linux/arm64, privileged",
              "pinned_host_key": pin}
    (out / "source").mkdir()
    source = l1.Source(out / "source", args.engine)
    target = a2.Target("arm64", args.run_id, out)
    tunnel = None
    monitor = l1.ArgvMonitor()
    try:
        a2.VM_AMD64_PROBE[0] = "docker.io/library/alpine:3.22"
        vm_before = a2.vm_state(image)
        source.start()
        report["source_version"] = source.actual_version
        # Secrets enter this HOME only through the wizard's own import (SAVE).
        tty = l1.Tty([str(binary), "init"], work, env, out / "init.terminal.raw")
        report["init"] = l1.wizard(tty, source, args.model_endpoint, args.model_key)
        local = json.loads((work / "stack.yaml").read_text())
        store = home / ".config/open-ontology/secrets"
        l1.private(store / "deploy-key", (private / "client").read_bytes())
        port = target.start(private)
        assert a2.vm_state(image) == vm_before, "target changed Docker VM kernel state"
        stack = dict(local)
        stack["sources"] = [dict(local["sources"][0], host=source.ip, port=source.port_inside)]
        stack["deploy"] = {"target": "ssh", "profile": "lite", "ssh": {
            "host": "127.0.0.1", "port": port, "user": "root",
            "key": {"secret_ref": "deploy-key"}, "host_key_sha256": pin}}
        config = work / "stack-ssh.yaml"
        l1.private(config, json.dumps(stack, indent=2))
        oo = lambda name, *argv, timeout=3600, data=None: l1.command(
            [binary, *argv], work, env, out, name, timeout, data)
        up = oo("up", "up", "--config", config, "--json")
        report["up"] = {"exit": up.returncode, "status": json.loads(up.stdout or b"{}").get("status")}
        if up.returncode:
            raise RuntimeError("oo up --target ssh failed")
        first = oo("build-1", "build", "--config", config, "--json")
        review_input = work / ".oo/review-input.json"
        if first.returncode != 1 or not review_input.exists():
            raise RuntimeError("remote build did not stop at owner review")
        decisions = l1.accept_all(json.loads(review_input.read_text()),
                                  l1.compiler_wrapper(out, image, f"{os.getuid()}:{os.getgid()}"))
        l1.private(work / "decisions.json", json.dumps(decisions))
        second = oo("build-2", "build", "--config", config, "--resume", "--review-file", work / "decisions.json")
        text = second.stdout.decode(errors="replace")
        (out / "completion.txt").write_text(text)
        report["build_exit"] = second.returncode
        if second.returncode or "2. Bind: " not in text:
            raise RuntimeError("remote build did not complete with a share prompt")
        chain = json.loads(target.exec("cat", "/root/.open-ontology/lite/work/build/.oo/chain.json").stdout)
        report["chain"] = {"status": chain["status"], "model_calls": chain.get("model_calls"),
                           "stages": [(s["name"], s["status"]) for s in chain["stages"]]}
        # The remote Gateway listens on the target's loopback; reach it over the
        # same pinned connection, keeping its URL (the token audience) unchanged.
        gateway = int(re.search(r"--gateway 'https://localhost:(\d+)'", text).group(1))
        tunnel = subprocess.Popen(["ssh", "-F", "/dev/null", "-N", "-o", "ExitOnForwardFailure=yes",
                                   "-o", f"UserKnownHostsFile={work / '.oo/ssh/known_hosts'}",
                                   "-o", "StrictHostKeyChecking=yes", "-o", "IdentitiesOnly=yes",
                                   "-o", "IdentityAgent=none", "-i", str(private / "client"),
                                   "-p", str(port), "-L", f"127.0.0.1:{gateway}:127.0.0.1:{gateway}",
                                   "root@127.0.0.1"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(2)
        agent_home = out / "agent-home"
        agent_home.mkdir(mode=0o700)
        agent_env = dict(env, HOME=str(agent_home))
        report["consumer"] = l1.consumer(binary, agent_env, text, out, source, "consumer")
        project = json.loads(target.exec("cat", "/root/.open-ontology/lite/owner.json").stdout)["project"]
        deadline = time.monotonic() + 420
        while time.monotonic() < deadline:
            status = json.loads(oo("status", "status", "--config", config, "--json").stdout or b"{}")
            if status.get("healthy"):
                break
            time.sleep(5)
        report["status_before"] = {"status": status.get("status"), "healthy": status.get("healthy"),
                                   "services": {r["Service"]: r["Status"] for r in status.get("services", [])},
                                   "gateway_health": status.get("gateway_health", {}).get("status"),
                                   "timer": status.get("remote", {}).get("timer")}
        if not status.get("healthy"):
            raise RuntimeError("remote deployment did not reach seven healthy services")
        count = "SELECT count(*) FROM jobs.tasks WHERE state='succeeded';"
        before = int(remote_sql(target, project, "control-db", count))
        source.change()
        remote_sql(target, project, "control-db", "UPDATE jobs.schedules SET next_run_at=clock_timestamp();")
        deadline = time.monotonic() + 900
        while time.monotonic() < deadline and int(remote_sql(target, project, "control-db", count)) <= before:
            if int(remote_sql(target, project, "control-db",
                              "SELECT count(*) FROM jobs.tasks WHERE state='failed';")):
                raise RuntimeError("remote scheduled cycle failed")
            time.sleep(3)
        query = l1.command([binary, "query", "--json", source.query], out, agent_env, out, "query-after-cycle")
        rows = json.loads(query.stdout or b"{}").get("rows")
        report["scheduled"] = {"query_rows": rows, "expected": source.changed,
                               "result": "passed" if l1.same_number(rows, source.changed) else "failed"}
        report["result"] = ("passed" if report["consumer"]["status"] == "passed"
                            and report["scheduled"]["result"] == "passed" else "failed")
        down = oo("down", "down", "--config", config, "--json")
        report["down"] = json.loads(down.stdout or b"{}").get("status")
        leftover = target.remote("docker ps -aq | wc -l; docker volume ls -q | wc -l", check=False)
        report["remote_after_down"] = leftover.split()
        assert a2.vm_state(image) == vm_before, "Docker VM kernel state changed"
        report["docker_vm_kernel_state"] = "unchanged"
    except Exception as error:
        report["result"] = "failed"
        report["error"] = str(error)[:400]
        logs = target.exec("sh", "-c", "docker ps -a; for c in $(docker ps -aq); do docker logs --tail 80 $c; done",
                           check=False)
        (out / "target-diagnostics.txt").write_bytes(logs.stdout + logs.stderr)
    finally:
        if tunnel:
            tunnel.terminate()
        argv = monitor.close(out / "argv.samples", [str(out).encode(), b"oo.lane=" + l1.LANE.encode()])
        values = [source.run.reader_secret.encode()] if hasattr(source, "run") else []
        files = [*out.glob("*.stdout"), *out.glob("*.stderr"), *out.glob("*.raw"), argv,
                 out / "completion.txt", out / "target-diagnostics.txt", work / "stack-ssh.yaml"]
        hits = l1.secret_scan(files, values + [(private / "client").read_bytes().strip()])
        report["secret_scan"] = {"hits": hits, "status": "passed" if not hits else "failed"}
        target.remove()
        report["source_cleanup_failures"] = source.close()
        for path in (private, home, out / "agent-home", work / ".oo/ssh/pki", work / ".oo/ssh/consumer"):
            shutil.rmtree(path, ignore_errors=True)
        for name in ("decisions.json",):
            (work / name).unlink(missing_ok=True)
        left = subprocess.run(["docker", "ps", "-aq", "--filter", "label=oo.lane=" + l1.LANE],
                              capture_output=True, text=True).stdout.split()
        report["containers_left"] = len(left)
        l1.write_json(out / "report.json", report)
    print(out.relative_to(ROOT), report["result"])
    return 0 if report["result"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())

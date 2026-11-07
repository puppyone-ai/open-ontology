import hashlib, json, subprocess
from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]
EV = ROOT / "evidence"
B = EV / "l1-final-build"
ENG = {"postgres": "PostgreSQL 17.11", "mysql": "MySQL 8.4.7", "oracle": "Oracle 23.5"}
RUNS = {"macos-arm64": {"postgres": "l1f-mac-pg", "mysql": "l1f-mac-my", "oracle": "l1f-mac-ora"},
        "linux-amd64": {"postgres": "l1f-lnx-pg", "mysql": "l1f-lnx-my", "oracle": "l1f-lnx-ora"}}
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
tarball = B / "packed/reduxxx888-ontology-cli-0.2.0.tgz"
manifest = json.loads((B / "package/native/manifest.json").read_text())
image = (B / "runtime.iid").read_text().strip()
out = {
    "status": None, "plan_item": "S4-M-02", "closes": "S6-M-03 B1 (independent review at a07ebfa01)",
    "decision": "D16",
    "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
    "worktree_tracked_changes_during_runs": bool(subprocess.check_output(
        ["git", "status", "--porcelain", "--untracked-files=no"], cwd=ROOT, text=True).strip()),
    "date": "2026-11-06",
    "artifacts": {
        "npm_tarball": {"file": tarball.name, "sha256": sha(tarball), "bytes": tarball.stat().st_size,
                        "staging": "packages/cli/scripts/stage.mjs --partial true (darwin-x64 is CI-only) + "
                                   "/opt/homebrew/bin/npm pack --ignore-scripts; notices from packages/cli/scripts/licenses.py (offline, 371 dependencies)",
                        "platforms": manifest["binaries"], "skill_sha256": manifest["skill_sha256"],
                        "published": False},
        "runtime_image": {"id": image, "platform": "linux/arm64", "label": "oo.lane=l1",
                          "build": "git archive HEAD | docker build --platform linux/arm64 --build-arg OO_LANE=l1 -f deploy/lite/Dockerfile -",
                          "tag": "oo-l1-lite-runtime:c5aa077"},
        "cli_builds": {
            "darwin-arm64": "MACOSX_DEPLOYMENT_TARGET=11.0 cargo build --release --locked -p oo-cli --bin oo (rustc 1.98.1)",
            "linux-x64": "git archive HEAD | docker buildx build --platform linux/amd64 -f ci/cli.Dockerfile (lane M cargo-zigbuild, target *.2.31); max GLIBC symbol GLIBC_2.30",
            "linux-arm64": "same recipe, --platform linux/arm64; max GLIBC symbol GLIBC_2.30",
            "portability": "Debian 11 (glibc 2.31) and Ubuntu 22.04 (glibc 2.35): oo --version + oo doctor --help pass on amd64 and arm64 (evidence/l1-final-build/linux-portability.txt)"},
    },
    "harness": {
        "l1_clean": "tools/oo-test/l1_clean.py at the commit, unmodified; driven by an untracked runner (evidence/l1-final-tools/l1_final.py) that only substitutes: lane label l1 (harness default a4), the stage.mjs tarball for the harness's diagnostic pack, gold questions in both flows (harness default: stack only), and a one-token model health probe (3 consecutive successes, <=30 min) before every Agent run",
        "registry": "the harness's own static npm registry (python http.server container, oo.lane=l1) serving the staged tarball on host loopback; the printed prompt's `npm install -g --registry ...` ran literally",
        "agent": "real model gpt-6.1-sol via secret_ref model-endpoint-9r / model-key-9r-sol; lane G t6.py @ f206d6424 model client and scoring; the Agent got only the printed share prompt on its own clean linux/amd64 machine (no Docker socket, no repository) and executed every step itself",
        "agent_machine": "always a clean linux/amd64 container (oo-l1-client:linux-amd64, Debian 13 node image); on the macOS rows the owner-side CLI ran natively on macOS arm64",
        "linux_amd64_daemon": "linux-amd64 rows: the owner CLI ran in a clean amd64 client container (no repository, no Rust) driving the HOST's arm64 Docker Desktop daemon through the mounted socket; the lite runtime image is linux/arm64. An amd64 runtime/daemon remains a D1 residual.",
    },
    "results": {}, "gold": {}, "secret_scan": {}, "model_health_probes": {}, "run_ids": [],
}
all_pass = True
for plat, engines in RUNS.items():
    for eng, rid in engines.items():
        s = json.loads((EV / rid / "summary.json").read_text())
        out["run_ids"].append(rid)
        e = s["engines"][eng]
        row = {"run_id": rid, "observed_version": e["version"], "same_result_tty_vs_stack": e.get("same_result"),
               "containers_left": s["containers_left"], "binary_sha256_in_package": manifest["binaries"]["darwin-arm64" if plat == "macos-arm64" else "linux-x64"]["sha256"]}
        if plat == "linux-amd64":
            row["client"] = {k: s["machine"].get(k) for k in ("client_uname", "client_image", "daemon", "client_tools")}
        all_pass &= s["status"] == "passed"
        for mode in ("tty", "stack"):
            r = json.loads((EV / rid / f"{plat}-{eng}" / mode / "report.json").read_text())
            a = r.get("agent", {})
            c = r.get("consumer", {})
            entry = {"result": r["result"], "init": "PTY oo init wizard (SAVE)" if mode == "tty" else "oo init --config stack.yaml --json (non-interactive)",
                     "init_exit": r["init"]["exit"], "preflight_exit": r["init"].get("preflight_exit"),
                     "up_exit": r["up"]["exit"], "chain": r["chain"]["status"], "build_model_calls": r["chain"]["model_calls"],
                     "review_items": r["review_items"], "review_mode": r["review_decision"]["mode"],
                     "agent_steps": [{"command": cmd.split(" --ca-file")[0], "exit": ex} for cmd, ex in zip(a["setup"]["commands"], a["setup"]["exits"])],
                     "agent_binding": a["binding"], "agent_model_calls": a.get("agent_model_calls"),
                     "consumer_check": {"whoami_role": (c.get("whoami", {}).get("value") or {}).get("role"),
                                        "tables": len((c.get("tables", {}).get("value") or {}).get("tables", [])),
                                        "query_rows": c.get("query", {}).get("rows"), "status": c.get("status")},
                     "seconds": r["seconds"], "teardown": r.get("teardown")}
            if mode == "stack":
                sc = r["scheduled"]
                entry["scheduled_cycle"] = {"result": sc["result"], "query_rows_after": sc["query_rows"],
                                            "expected": sc["expected"], "scheduled_model_calls": sc["scheduled_model_calls"],
                                            "services_before": sc["before"]["services"], "services_after": sc["after"]["services"],
                                            "healthy_services_after": sum(v in ("healthy", "succeeded") for v in sc["after"]["services"].values()),
                                            "gateway_health": sc["after"]["gateway_health"]}
                entry["sigkill_resume"] = r["sigkill_resume"]["attempts_after_resume"]
            key = f"{plat}/{eng}/{mode}"
            out["results"][key] = entry
            out["gold"][key] = {"asked": a["asked"], "correct": a["correct"],
                                "questions": {q["id"]: {"status": q["status"], "equal": q["equal"], "error": q["error"]} for q in a["questions"]}}
            ss = r["secret_scan"]
            out["secret_scan"][key] = {"hits": len(ss["hits"]), "containers_sampled": ss["containers_sampled"],
                                       "argv_sample_lines": len((EV / rid / f"{plat}-{eng}" / mode / "argv.samples").read_bytes().splitlines()),
                                       "status": ss["status"]}
            p = json.loads((EV / rid / f"{plat}-{eng}" / mode / "agent/model-health-probe.json").read_text())
            out["model_health_probes"][key] = {"healthy": p["healthy"], "attempts": len(p["attempts"])}
ssh = json.loads((EV / "l1f-ssh-pg/report.json").read_text())
out["run_ids"].append("l1f-ssh-pg")
out["results"]["ssh-target/postgres"] = {k: ssh.get(k) for k in (
    "run_id", "source_version", "target", "pinned_host_key", "up", "build_exit", "chain", "consumer", "status_before",
    "scheduled", "down", "remote_after_down", "docker_vm_kernel_state", "result", "error", "containers_left")}
if "consumer" in out["results"]["ssh-target/postgres"] and isinstance(ssh.get("consumer"), dict):
    out["results"]["ssh-target/postgres"]["consumer"] = {"status": ssh["consumer"].get("status"),
                                                         "query_rows": ssh["consumer"].get("query", {}).get("rows")}
out["results"]["ssh-target/postgres"]["note"] = "l1_ssh.py consumer steps are run by the harness with the host CLI over an SSH tunnel (no Agent in this harness)"
out["secret_scan"]["ssh-target/postgres"] = {"hits": len(ssh["secret_scan"]["hits"]), "status": ssh["secret_scan"]["status"]}
all_pass &= ssh["result"] == "passed"
out["secret_scan_total_hits"] = sum(v["hits"] for v in out["secret_scan"].values())
out["status"] = "passed" if all_pass and out["secret_scan_total_hits"] == 0 else "failed"
dest = ROOT / "crates/oo-cli/src/admin/l1-final-evidence.json"
dest.write_text(json.dumps(out, indent=1, ensure_ascii=False) + "\n")
print(dest, out["status"], out["secret_scan_total_hits"])

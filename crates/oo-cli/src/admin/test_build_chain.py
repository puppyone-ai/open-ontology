#!/usr/bin/env python3
"""SIGKILL handoff tests: fake draft driver, real review/compiler; no publication."""
import argparse
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time

from test_review import ROOT, make_input, lib


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-id", required=True)
    args = parser.parse_args()
    assert re.fullmatch("[a-z0-9-]+", args.run_id)
    os.umask(0o077)
    out = ROOT / "evidence" / args.run_id
    out.mkdir(mode=0o700)
    template = make_input(out)
    fixture_draft = dict(protocol="test.draft/v1", outcome="validated")
    template["source_draft_sha256"] = lib.digest(fixture_draft)
    input_hash = lib.digest(template)
    items = {}
    for id_ in template["draft"]["registry"]["entities"]:
        items["entity/" + id_] = dict(action="accept")
    for id_, value in template["draft"]["accepted"].items():
        if value["kind"] != "entity":
            items[("relationship/" if value["kind"] == "relation" else "description/") + id_] = dict(action="accept")
    for metric in template["effective"]["metrics"]:
        items["metric/" + metric["metric"]] = dict(action="accept")
    for table in template["effective"]["datasets"]:
        for column in table["columns"]:
            items["classification/" + table["dataset"] + "/" + column["name"]] = dict(action="accept")
    decision = dict(protocol="oo.owner-review/v1", input_sha256=input_hash, reviewed_at="2026-11-04",
                    items=items, g14=dict(action="accept", reason="Fixture operator reviewed",
                                        summary_sha256=template["draft"]["review"]["summary_sha256"]))
    expected_pack = None
    passed = []
    for name, block_at in (("uninterrupted", 0), ("after-draft", 1), ("after-review", 3),
                           ("late-input", 0), ("forged-output", 1)):
        root = out / name
        root.mkdir(mode=0o700)
        binaries = root / "bin"
        binaries.mkdir()
        os.link(ROOT / "target/debug/oo", binaries / "oo")
        # Only this test installation substitutes the external draft driver.
        fake = f"""#!{sys.executable}
import json,os,pathlib,sys
request=json.load(sys.stdin)
root=pathlib.Path(request['state_dir'])
count=root/'generation-count'
count.write_text(str(int(count.read_text())+1) if count.exists() else '1')
draft={fixture_draft!r}
state={{'protocol':'oo.build-progress/v1','status':'draft','agent_db_load':'local_table_views','stages':[]}}
for name,value in [('draft.json',draft),('run.json',state)]:
    (root/name).write_text(json.dumps(value))
    (root/name).chmod(0o600)
print(json.dumps(state),flush=True)
"""
        (binaries / "python3").write_text(fake)
        (binaries / "python3").chmod(0o700)
        compiler = f"""#!{sys.executable}
import json,pathlib,subprocess,sys,time
request=sys.stdin.buffer.read()
root=pathlib.Path({str(root)!r})
operation=json.loads(request)['operation']
if operation=='review':
    counter=root/'summary-count'
    n=int(counter.read_text())+1 if counter.exists() else 1
    counter.write_text(str(n))
    if n=={block_at}:
        (root/'at-boundary').write_text('ready')
        while True: time.sleep(.1)
result=subprocess.run([{str(ROOT/'target/debug/selfbuild-budget')!r}],input=request,capture_output=True)
sys.stdout.buffer.write(result.stdout)
sys.exit(result.returncode)
"""
        (binaries / "selfbuild-budget").write_text(compiler)
        (binaries / "selfbuild-budget").chmod(0o700)
        config = root / "stack.json"
        config.write_bytes((ROOT / "crates/oo-stack/example.stack.json").read_bytes())
        config.chmod(0o600)
        lib.write_private(root / "input.json", template)
        lib.write_private(root / "decisions.json", decision)
        lib.write_private(root / "target.json", dict(host="localhost", port=5432,
            database="fixture", user="loader", password={"secret_ref":"password"},
            tls_ca={"secret_ref":"ca"}, run_id="fixture"))
        argv = [str(binaries / "oo"), "build", "--config", str(config),
                "--agent-target", str(root / "target.json"),
                "--review-input", str(root / "input.json"),
                "--review-file", str(root / "decisions.json"), "--json"]
        env = dict(os.environ, PATH=str(binaries) + os.pathsep + os.environ["PATH"])
        if name == "late-input":
            position = argv.index("--review-input")
            missing = argv[:position] + argv[position + 2:]
            result = subprocess.run(missing, env=env, capture_output=True, timeout=30)
            assert result.returncode == 1
            assert json.loads(result.stdout)["stages"][1]["error"] == "BUILD_REVIEW_INPUT_REQUIRED"
            argv += ["--resume"]
        if block_at:
            child = subprocess.Popen(argv, env=env, stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE, start_new_session=True)
            try:
                deadline = time.monotonic() + 30
                while not (root / "at-boundary").exists() and time.monotonic() < deadline:
                    if child.poll() is not None:
                        raise AssertionError(child.communicate())
                    time.sleep(.02)
                assert (root / "at-boundary").exists(), name
                before = json.loads((root / ".oo/chain.json").read_text())
                assert before["stages"][0]["status"] == "complete"
                if name == "after-review":
                    assert before["stages"][1]["status"] == "complete"
                os.killpg(child.pid, signal.SIGKILL)
                stdout, stderr = child.communicate(timeout=10)
                (root / "killed.log").write_bytes(stdout + stderr)
                assert child.returncode == -signal.SIGKILL
            finally:
                if child.poll() is None:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait()
            argv += ["--resume"]
        if name == "forged-output":
            recovered = json.loads((out / "uninterrupted/.oo/reviewed-registry.json").read_text())
            recovered["compiled"]["effective"]["entities"][0]["label"] = "Unreviewed replacement"
            lib.write_private(root / ".oo/reviewed-registry.json", recovered)
        result = subprocess.run(argv, env=env, capture_output=True, timeout=30)
        (root / "result.log").write_bytes(result.stdout + result.stderr)
        assert result.returncode == 1, (name, result.stdout, result.stderr)
        state = json.loads(result.stdout)
        assert state["status"] == "blocked"
        if name == "forged-output":
            assert state["stages"][1]["error"] == "REVIEW_HANDOFF_BINDING", state
            assert state["stages"][2]["status"] == "pending"
            assert (root / ".oo/generation-count").read_text() == "1"
            passed.append("recovered-output-recompiled")
            continue
        assert [s["status"] for s in state["stages"]] == [
            "complete", "complete", "blocked", "pending", "pending", "pending"], state
        assert state["stages"][2]["error"] == "BUILD_PUBLICATION_NOT_INTEGRATED", state
        assert (root / ".oo/generation-count").read_text() == "1"
        reviewed = json.loads((root / ".oo/reviewed-registry.json").read_text())
        digest = reviewed["compiled"]["producer"]["pack_digest"]
        if expected_pack is None:
            expected_pack = digest
        assert digest == expected_pack
        status = subprocess.run([str(binaries / "oo"), "status", "--config", str(config), "--json"],
                                env=env, capture_output=True, timeout=10)
        assert status.returncode == 1 and json.loads(status.stdout) == state
        resumed = subprocess.run(argv if "--resume" in argv else argv + ["--resume"],
                                 env=env, capture_output=True, timeout=30)
        assert resumed.returncode == 1
        assert json.loads(resumed.stdout)["stages"][2]["error"] == "BUILD_PUBLICATION_NOT_INTEGRATED"
        assert (root / ".oo/generation-count").read_text() == "1"
        passed.append(name)
        # A tampered completed artifact must fail before invoking any external process.
        lib.write_private(root / ".oo/draft.json", dict(outcome="tampered"))
        bad = subprocess.run(argv if "--resume" in argv else argv + ["--resume"],
                             env=env, capture_output=True, timeout=10)
        assert bad.returncode != 0 and b"BUILD_RESUME_ARTIFACT_CHANGED" in bad.stdout + bad.stderr
        assert (root / ".oo/generation-count").read_text() == "1"
        passed.append(name + "-tamper-rejected")
    lib.write_private(out / "result.json", dict(status="passed", cases=passed,
        draft_driver="synthetic_process", compiler="real_selfbuild_budget",
        kill_boundaries=["draft_to_review", "review_to_publish"],
        publication="blocked_not_integrated", live_model=False, databases=False,
        untested_boundaries=["publish_to_gateway", "gateway_to_token", "token_to_prompt"]))
    print(json.dumps(dict(status="passed", cases=len(passed), run_id=args.run_id)))


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Independent a096 probes against the pinned aarch64 helper. Not a product test."""
import hashlib, json, os, shutil, signal, stat, subprocess, time
from pathlib import Path

HELPER = Path("/src/testing/harness/containment-supervisor/bin/containment-supervisor")
REBUILD = Path("/target/helper/release/containment-supervisor")
ROOT = Path("/target/probes")
LIMIT = 16 * 1024 * 1024
OUT = Path("/work/probe-result.json")
LOG = Path("/work/probe.log")

def log(msg):
    line = f"{time.time():.3f} {msg}"
    with LOG.open("a") as fh:
        fh.write(line + "\n")
    print(line, flush=True)

def starttime(pid):
    if pid <= 1:
        return None
    try:
        text = Path(f"/proc/{pid}/stat").read_text()
    except OSError:
        return None
    close = text.rfind(")")
    if close < 0:
        return None
    fields = text[close + 1 :].split()
    if len(fields) < 20:
        return None
    try:
        return int(fields[19])
    except ValueError:
        return None

def comm_of(pid):
    try:
        return Path(f"/proc/{pid}/comm").read_text().strip()
    except OSError:
        return None

def ppid_of(pid):
    try:
        text = Path(f"/proc/{pid}/stat").read_text()
    except OSError:
        return None
    close = text.rfind(")")
    fields = text[close + 1 :].split()
    if len(fields) < 2:
        return None
    try:
        return int(fields[1])
    except ValueError:
        return None

def field(status, key):
    for line in status.splitlines():
        if line.startswith(key + "="):
            return line.split("=", 1)[1]
    return ""

def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()

def inode_size(path):
    st = os.stat(path)
    return st.st_ino, st.st_size

def kill_owned(pid, start):
    """Narrow kill: only if the pid still has the recorded starttime."""
    if pid <= 1 or start is None:
        return "skipped"
    now = starttime(pid)
    if now is None:
        return "already-gone"
    if now != start:
        return "starttime-changed-left-alone"
    os.kill(pid, signal.SIGKILL)
    time.sleep(0.05)
    later = starttime(pid)
    if later == start:
        return "still-alive"
    return "killed"

def spawn_foreign():
    proc = subprocess.Popen(
        ["/bin/sleep", "180"],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    time.sleep(0.05)
    start = starttime(proc.pid)
    return proc, proc.pid, start

def assert_foreign(pid, start):
    now = starttime(pid)
    return {
        "pid": pid,
        "start": start,
        "now": now,
        "same": now == start,
        "comm": comm_of(pid),
    }

def run_helper(args, limit, term_after=None, kill_after=None):
    work = ROOT / f"case-{time.time_ns()}"
    work.mkdir(parents=True)
    status = work / "status"
    stdout = work / "stdout"
    stderr = work / "stderr"
    proc = subprocess.Popen(
        [str(HELPER), str(status), str(stdout), str(stderr), str(limit), "--", *args],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    helper_pid = proc.pid
    started = time.monotonic()
    if term_after is not None:
        time.sleep(term_after)
        os.kill(helper_pid, signal.SIGTERM)
    if kill_after is not None:
        time.sleep(kill_after)
        try:
            os.kill(helper_pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    deadline = started + 12
    code = None
    while time.monotonic() < deadline:
        got = proc.poll()
        if got is not None:
            code = got
            break
        time.sleep(0.02)
    else:
        try:
            os.kill(helper_pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        proc.wait(timeout=2)
        code = -9
    elapsed = time.monotonic() - started
    text = status.read_text() if status.exists() else ""
    return {
        "work": str(work),
        "code": code,
        "elapsed": round(elapsed, 4),
        "status": text,
        "containment": field(text, "containment"),
        "spawned": field(text, "spawned"),
        "cleanup_incomplete": field(text, "cleanup_incomplete"),
        "output_limited": field(text, "output_limited"),
        "note": field(text, "note"),
        "cgroup": field(text, "cgroup"),
        "exit_code": field(text, "exit_code"),
        "direct_pid": field(text, "direct_pid"),
        "stdout": stdout,
        "stderr": stderr,
        "helper_pid": helper_pid,
    }

def pid_and_start(text):
    lines = text.splitlines()
    if not lines:
        return 0, None
    try:
        pid = int(lines[0].strip())
    except ValueError:
        return 0, None
    stat_line = lines[1] if len(lines) > 1 else ""
    close = stat_line.rfind(")")
    start = None
    if close >= 0:
        fields = stat_line[close + 1 :].split()
        if len(fields) >= 20:
            try:
                start = int(fields[19])
            except ValueError:
                start = None
    return pid, start

def gone(pid, start):
    now = starttime(pid)
    if now is None:
        return True, now
    if start is None:
        return False, now
    return now != start, now

SETSID_WAIT = r"""#!/bin/sh
pidfile=$1
if [ -x /usr/bin/setsid ]; then SETSID=/usr/bin/setsid
elif [ -x /bin/setsid ]; then SETSID=/bin/setsid
else echo NO_SETSID > "$pidfile"; exit 9; fi
"$SETSID" /bin/sh -c 'echo $$ > "$1"; cat /proc/$$/stat >> "$1"; exec /bin/sleep 121' _ "$pidfile" >/dev/null 2>&1 &
i=0
while [ ! -s "$pidfile" ] && [ "$i" -lt 200 ]; do i=$((i+1)); sleep 0.01; done
exit 0
"""

SETSID_IMMEDIATE = r"""#!/bin/sh
pidfile=$1
if [ -x /usr/bin/setsid ]; then SETSID=/usr/bin/setsid
elif [ -x /bin/setsid ]; then SETSID=/bin/setsid
else echo NO_SETSID > "$pidfile"; exit 9; fi
"$SETSID" /bin/sh -c 'echo $$ > "$1"; cat /proc/$$/stat >> "$1"; exec /bin/sleep 121' _ "$pidfile" >/dev/null 2>&1 &
exit 0
"""

HOLD_INHERIT = r"""#!/bin/sh
pidfile=$1
if [ -x /usr/bin/setsid ]; then SETSID=/usr/bin/setsid
elif [ -x /bin/setsid ]; then SETSID=/bin/setsid
else echo NO_SETSID > "$pidfile"; exit 9; fi
"$SETSID" /bin/sh -c 'echo $$ > "$1"; cat /proc/$$/stat >> "$1"; while true; do printf xxxxxxxxxxxxxxxxxxxx; done' _ "$pidfile" &
i=0
while [ ! -s "$pidfile" ] && [ "$i" -lt 200 ]; do i=$((i+1)); sleep 0.01; done
exit 0
"""

def write_script(path, body):
    path.write_text(body)
    os.chmod(path, 0o755)

def elf_sections(path):
    data = path.read_bytes()
    if data[:4] != b"\x7fELF" or data[4] != 2 or data[5] != 1:
        return {"error": "not-elf64-le"}
    e_shoff = int.from_bytes(data[40:48], "little")
    e_shentsize = int.from_bytes(data[58:60], "little")
    e_shnum = int.from_bytes(data[60:62], "little")
    e_shstrndx = int.from_bytes(data[62:64], "little")
    sections = []
    for i in range(e_shnum):
        off = e_shoff + i * e_shentsize
        hdr = data[off : off + e_shentsize]
        if len(hdr) < 64:
            continue
        sh_name = int.from_bytes(hdr[0:4], "little")
        sh_offset = int.from_bytes(hdr[24:32], "little")
        sh_size = int.from_bytes(hdr[32:40], "little")
        sections.append((sh_name, sh_offset, sh_size))
    str_off = sections[e_shstrndx][1]
    str_size = sections[e_shstrndx][2]
    strtab = data[str_off : str_off + str_size]
    named = {}
    for sh_name, sh_offset, sh_size in sections:
        end = strtab.find(b"\x00", sh_name)
        name = strtab[sh_name:end].decode("ascii", "replace")
        blob = data[sh_offset : sh_offset + sh_size] if sh_offset + sh_size <= len(data) else b""
        named[name] = hashlib.sha256(blob).hexdigest()
    return named

def needed(path):
    # DT_NEEDED entries from the dynamic segment, names via the string table.
    data = path.read_bytes()
    e_phoff = int.from_bytes(data[32:40], "little")
    e_phentsize = int.from_bytes(data[54:56], "little")
    e_phnum = int.from_bytes(data[56:58], "little")
    dyn = None
    for i in range(e_phnum):
        off = e_phoff + i * e_phentsize
        hdr = data[off : off + e_phentsize]
        p_type = int.from_bytes(hdr[0:4], "little")
        if p_type == 2:  # PT_DYNAMIC
            p_offset = int.from_bytes(hdr[8:16], "little")
            p_filesz = int.from_bytes(hdr[32:40], "little")
            dyn = data[p_offset : p_offset + p_filesz]
            break
    if dyn is None:
        return []
    tags = []
    for i in range(0, len(dyn), 16):
        d_tag = int.from_bytes(dyn[i : i + 8], "little", signed=True)
        d_val = int.from_bytes(dyn[i + 8 : i + 16], "little")
        tags.append((d_tag, d_val))
        if d_tag == 0:
            break
    strsz = next((v for t, v in tags if t == 10), 0)  # DT_STRSZ
    straddr = next((v for t, v in tags if t == 5), 0)  # DT_STRTAB virtual
    # Map vaddr to offset via PT_LOAD.
    loads = []
    for i in range(e_phnum):
        off = e_phoff + i * e_phentsize
        hdr = data[off : off + e_phentsize]
        if int.from_bytes(hdr[0:4], "little") == 1:
            p_offset = int.from_bytes(hdr[8:16], "little")
            p_vaddr = int.from_bytes(hdr[16:24], "little")
            p_filesz = int.from_bytes(hdr[32:40], "little")
            loads.append((p_vaddr, p_offset, p_filesz))
    def v2o(vaddr):
        for p_vaddr, p_offset, p_filesz in loads:
            if p_vaddr <= vaddr < p_vaddr + p_filesz:
                return p_offset + (vaddr - p_vaddr)
        return None
    str_off = v2o(straddr)
    names = []
    if str_off is None:
        return names
    blob = data[str_off : str_off + strsz]
    for tag, val in tags:
        if tag == 1:  # DT_NEEDED
            end = blob.find(b"\x00", val)
            names.append(blob[val:end].decode("ascii", "replace"))
    return names

def main():
    if ROOT.exists():
        shutil.rmtree(ROOT)
    ROOT.mkdir(parents=True)
    LOG.write_text("")
    report = {"cases": []}
    foreign, foreign_pid, foreign_start = spawn_foreign()
    report["foreign_initial"] = assert_foreign(foreign_pid, foreign_start)
    log(f"foreign {foreign_pid} {foreign_start}")

    # cgroup capability
    cgroup_text = Path("/proc/self/cgroup").read_text()
    cgroup_try = {"cgroup_self": cgroup_text.strip(), "mkdir": None}
    try:
        os.mkdir("/sys/fs/cgroup/oo-a096-probe")
        cgroup_try["mkdir"] = "ok"
        os.rmdir("/sys/fs/cgroup/oo-a096-probe")
    except OSError as exc:
        cgroup_try["mkdir"] = f"errno={exc.errno} {exc.strerror}"
    report["cgroup_probe"] = cgroup_try
    log(f"cgroup {cgroup_try['mkdir']}")

    report["needed"] = needed(HELPER)
    log(f"needed {report['needed']}")

    # positive echo
    echo = run_helper(["/bin/echo", "hello-a096"], 1024)
    echo_out = echo["stdout"].read_bytes()
    report["cases"].append({
        "name": "positive-echo",
        "containment": echo["containment"],
        "note": echo["note"],
        "exit_code": echo["exit_code"],
        "cleanup_incomplete": echo["cleanup_incomplete"],
        "output_limited": echo["output_limited"],
        "cgroup": echo["cgroup"],
        "code": echo["code"],
        "elapsed": echo["elapsed"],
        "stdout": echo_out.decode(),
        "foreign": assert_foreign(foreign_pid, foreign_start),
    })
    log(f"echo {echo['containment']} {echo['note']} {echo['elapsed']}")

    # comms during a live payload
    comm_case = ROOT / "comms"
    comm_case.mkdir()
    status = comm_case / "status"
    proc = subprocess.Popen(
        [str(HELPER), str(status), str(comm_case / "stdout"), str(comm_case / "stderr"), "1024", "--", "/bin/sleep", "2"],
        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    time.sleep(0.15)
    children = []
    task_dir = Path(f"/proc/{proc.pid}/task")
    if task_dir.is_dir():
        for task in task_dir.iterdir():
            child_file = task / "children"
            try:
                text = child_file.read_text()
            except OSError:
                continue
            for token in text.split():
                try:
                    cpid = int(token)
                except ValueError:
                    continue
                children.append({"pid": cpid, "comm": comm_of(cpid), "ppid": ppid_of(cpid)})
    helper_comm = comm_of(proc.pid)
    proc.wait(timeout=8)
    report["live_comms"] = {"helper_comm": helper_comm, "children": children, "status": status.read_text() if status.exists() else ""}
    log(f"comms helper={helper_comm} children={children}")

    def escape_batch(name, script_body, count):
        alive = 0
        gone_n = 0
        inconclusive = 0
        verified = 0
        rows = []
        for i in range(count):
            work = ROOT / f"{name}-{i}"
            work.mkdir()
            script = work / "escape.sh"
            pidfile = work / "grandchild.pid"
            write_script(script, script_body)
            outcome = run_helper(["/bin/sh", str(script), str(pidfile)], 1024)
            recorded = pidfile.read_text() if pidfile.exists() else ""
            pid, start = pid_and_start(recorded)
            is_gone, now = gone(pid, start) if pid > 1 else (False, None)
            foreign_ok = assert_foreign(foreign_pid, foreign_start)
            row = {
                "i": i,
                "containment": outcome["containment"],
                "note": outcome["note"],
                "cleanup": outcome["cleanup_incomplete"],
                "pid": pid,
                "start": start,
                "now": now,
                "gone": is_gone,
                "elapsed": outcome["elapsed"],
                "foreign_same": foreign_ok["same"],
                "no_setsid": "NO_SETSID" in recorded,
            }
            rows.append(row)
            if outcome["containment"] == "verified" and outcome["cleanup_incomplete"] == "0":
                verified += 1
            if pid <= 1 or start is None or row["no_setsid"]:
                inconclusive += 1
            elif is_gone:
                gone_n += 1
            else:
                alive += 1
                kill_owned(pid, start)
            if not foreign_ok["same"]:
                log(f"FOREIGN CHANGED during {name} {i}")
                break
        return {
            "name": name,
            "count": count,
            "verified": verified,
            "gone": gone_n,
            "alive": alive,
            "inconclusive": inconclusive,
            "max_elapsed": max(r["elapsed"] for r in rows) if rows else None,
            "foreign_all_same": all(r["foreign_same"] for r in rows),
            "sample": rows[:2] + rows[-1:],
        }

    report["escape_30"] = escape_batch("setsid-wait", SETSID_WAIT, 30)
    log(f"escape30 {report['escape_30']}")
    report["escape_immediate_5"] = escape_batch("setsid-now", SETSID_IMMEDIATE, 5)
    log(f"immediate {report['escape_immediate_5']}")

    # inherited pipe hold, stdout and stderr are the same script writing to inherited stdout
    hold_rows = []
    for label in ("hold-stdout",):
        work = ROOT / label
        work.mkdir()
        script = work / "hold.sh"
        pidfile = work / "writer.pid"
        write_script(script, HOLD_INHERIT)
        outcome = run_helper(["/bin/sh", str(script), str(pidfile)], 4096)
        recorded = pidfile.read_text() if pidfile.exists() else ""
        pid, start = pid_and_start(recorded)
        is_gone, now = gone(pid, start) if pid > 1 else (False, None)
        ino, size = inode_size(outcome["stdout"])
        digest = sha256(outcome["stdout"])
        time.sleep(0.4)
        ino2, size2 = inode_size(outcome["stdout"])
        digest2 = sha256(outcome["stdout"])
        hold_rows.append({
            "name": label,
            "containment": outcome["containment"],
            "note": outcome["note"],
            "cleanup": outcome["cleanup_incomplete"],
            "output_limited": outcome["output_limited"],
            "pid": pid,
            "gone": is_gone,
            "now": now,
            "size": size,
            "size_after": size2,
            "ino": ino,
            "ino_after": ino2,
            "sha_same": digest == digest2,
            "elapsed": outcome["elapsed"],
            "foreign": assert_foreign(foreign_pid, foreign_start),
        })
        if not is_gone:
            kill_owned(pid, start)
    report["held_inherit"] = hold_rows
    log(f"hold {hold_rows}")

    # malformed limit 0
    bad = run_helper(["/bin/sleep", "30"], 0)
    report["malformed"] = {
        "code": bad["code"],
        "containment": bad["containment"],
        "spawned": bad["spawned"],
        "note": bad["note"],
        "cleanup": bad["cleanup_incomplete"],
        "stdout_exists": bad["stdout"].exists(),
        "foreign": assert_foreign(foreign_pid, foreign_start),
        "elapsed": bad["elapsed"],
    }
    log(f"malformed {report['malformed']}")

    # SIGTERM bounded
    term = run_helper(["/bin/sleep", "30"], 1024, term_after=0.2)
    direct = int(term["direct_pid"] or "0")
    report["sigterm"] = {
        "containment": term["containment"],
        "note": term["note"],
        "cleanup": term["cleanup_incomplete"],
        "elapsed": term["elapsed"],
        "direct_pid": direct,
        "direct_gone": starttime(direct) is None,
        "foreign": assert_foreign(foreign_pid, foreign_start),
    }
    log(f"sigterm {report['sigterm']}")

    # exact 16MiB, plus one, fast 32MiB
    exact = run_helper(["/bin/dd", "if=/dev/zero", "bs=1048576", "count=16"], LIMIT)
    eino, esize = inode_size(exact["stdout"])
    edig = sha256(exact["stdout"])
    time.sleep(0.3)
    eino2, esize2 = inode_size(exact["stdout"])
    edig2 = sha256(exact["stdout"])
    report["exact_16mib"] = {
        "containment": exact["containment"],
        "note": exact["note"],
        "output_limited": exact["output_limited"],
        "exit_code": exact["exit_code"],
        "cleanup": exact["cleanup_incomplete"],
        "size": esize,
        "size_after": esize2,
        "ino_same": eino == eino2,
        "sha_same": edig == edig2,
        "elapsed": exact["elapsed"],
        "foreign": assert_foreign(foreign_pid, foreign_start),
    }
    log(f"exact {report['exact_16mib']}")

    plus = run_helper(["/bin/sh", "-c", "/bin/dd if=/dev/zero bs=1048576 count=16; /bin/dd if=/dev/zero bs=1 count=1"], LIMIT)
    pino, psize = inode_size(plus["stdout"])
    pdig = sha256(plus["stdout"])
    time.sleep(0.3)
    pino2, psize2 = inode_size(plus["stdout"])
    pdig2 = sha256(plus["stdout"])
    report["plus_one"] = {
        "containment": plus["containment"],
        "note": plus["note"],
        "output_limited": plus["output_limited"],
        "exit_code": plus["exit_code"],
        "cleanup": plus["cleanup_incomplete"],
        "size": psize,
        "size_after": psize2,
        "ino_same": pino == pino2,
        "sha_same": pdig == pdig2,
        "elapsed": plus["elapsed"],
        "foreign": assert_foreign(foreign_pid, foreign_start),
    }
    log(f"plus {report['plus_one']}")

    fast = run_helper(["/bin/dd", "if=/dev/zero", "bs=1048576", "count=32"], LIMIT)
    fino, fsize = inode_size(fast["stdout"])
    fdig = sha256(fast["stdout"])
    time.sleep(0.3)
    fino2, fsize2 = inode_size(fast["stdout"])
    fdig2 = sha256(fast["stdout"])
    report["fast_32mib"] = {
        "containment": fast["containment"],
        "note": fast["note"],
        "output_limited": fast["output_limited"],
        "exit_code": fast["exit_code"],
        "cleanup": fast["cleanup_incomplete"],
        "size": fsize,
        "size_after": fsize2,
        "ino_same": fino == fino2,
        "sha_same": fdig == fdig2,
        "elapsed": fast["elapsed"],
        "foreign": assert_foreign(foreign_pid, foreign_start),
    }
    log(f"fast {report['fast_32mib']}")

    # SIGKILL supervisor while a setsid grandchild and its parent are alive.
    kill_work = ROOT / "sigkill"
    kill_work.mkdir()
    script = kill_work / "stay.sh"
    pidfile = kill_work / "grand.pid"
    write_script(script, r"""#!/bin/sh
pidfile=$1
if [ -x /usr/bin/setsid ]; then SETSID=/usr/bin/setsid
elif [ -x /bin/setsid ]; then SETSID=/bin/setsid
else echo NO_SETSID > "$pidfile"; exit 9; fi
"$SETSID" /bin/sh -c 'echo $$ > "$1"; cat /proc/$$/stat >> "$1"; exec /bin/sleep 121' _ "$pidfile" >/dev/null 2>&1 &
i=0
while [ ! -s "$pidfile" ] && [ "$i" -lt 200 ]; do i=$((i+1)); sleep 0.01; done
exec /bin/sleep 30
""")
    # manual spawn so we can kill after the pidfile exists
    status = kill_work / "status"
    proc = subprocess.Popen(
        [str(HELPER), str(status), str(kill_work / "stdout"), str(kill_work / "stderr"), "1024", "--", "/bin/sh", str(script), str(pidfile)],
        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    deadline = time.monotonic() + 3
    recorded = ""
    while time.monotonic() < deadline:
        if pidfile.exists() and pidfile.stat().st_size > 0:
            recorded = pidfile.read_text()
            if "NO_SETSID" not in recorded and pid_and_start(recorded)[0] > 1:
                break
        time.sleep(0.02)
    pid, start = pid_and_start(recorded)
    time.sleep(0.05)
    try:
        os.kill(proc.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    try:
        proc.wait(timeout=2)
    except subprocess.TimeoutExpired:
        pass
    time.sleep(0.1)
    still, now = gone(pid, start) if pid > 1 else (False, None)
    # gone() True means the original starttime is gone. Invert for alive.
    alive = (not still) if pid > 1 else False
    status_text = status.read_text() if status.exists() else ""
    report["sigkill_supervisor"] = {
        "helper_pid": proc.pid,
        "helper_return": proc.returncode,
        "grandchild_pid": pid,
        "grandchild_start": start,
        "grandchild_alive_same_start": alive,
        "status_present": status.exists(),
        "status": status_text,
        "containment": field(status_text, "containment"),
        "foreign": assert_foreign(foreign_pid, foreign_start),
        "cleanup_of_grandchild": kill_owned(pid, start) if alive else "not-needed",
    }
    log(f"sigkill {report['sigkill_supervisor']}")

    # fork storm: many double-forks then parent exits
    storm = ROOT / "storm.py"
    storm.write_text(
        "import os\n"
        "for _ in range(40):\n"
        "    if os.fork()==0:\n"
        "        if os.fork()==0:\n"
        "            os.execv('/bin/sleep', ['sleep','121'])\n"
        "        os._exit(0)\n"
        "os._exit(0)\n"
    )
    before = set()
    for entry in Path("/proc").iterdir():
        if entry.name.isdigit():
            before.add(int(entry.name))
    storm_run = run_helper(["/usr/bin/python3", str(storm)], 1024)
    after_alive = []
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        pid = int(entry.name)
        if pid in before or pid <= 1:
            continue
        if comm_of(pid) == "sleep":
            st = starttime(pid)
            # only sleeps we might have created: ppid 1 or not our foreign
            if pid != foreign_pid:
                after_alive.append({"pid": pid, "start": st, "ppid": ppid_of(pid), "comm": "sleep"})
    cleaned = []
    for item in after_alive:
        cleaned.append({"pid": item["pid"], "result": kill_owned(item["pid"], item["start"])})
    report["fork_storm"] = {
        "containment": storm_run["containment"],
        "note": storm_run["note"],
        "cleanup": storm_run["cleanup_incomplete"],
        "elapsed": storm_run["elapsed"],
        "survivor_sleeps": after_alive,
        "cleaned": cleaned,
        "foreign": assert_foreign(foreign_pid, foreign_start),
    }
    log(f"storm {report['fork_storm']}")

    # section relationship of pinned vs rebuild
    if REBUILD.is_file():
        pinned_sec = elf_sections(HELPER)
        rebuild_sec = elf_sections(REBUILD)
        names = sorted(set(pinned_sec) | set(rebuild_sec))
        diffs = []
        same = []
        for name in names:
            a = pinned_sec.get(name)
            b = rebuild_sec.get(name)
            if a == b:
                same.append(name)
            else:
                diffs.append(name)
        report["rebuild_sections"] = {"same": same, "differ": diffs}
    else:
        report["rebuild_sections"] = {"error": "missing"}
    log(f"sections differ {report['rebuild_sections'].get('differ')}")

    # final foreign still ours, then narrow-kill our own sleeper
    report["foreign_final"] = assert_foreign(foreign_pid, foreign_start)
    report["foreign_cleanup"] = kill_owned(foreign_pid, foreign_start)
    try:
        foreign.wait(timeout=1)
    except subprocess.TimeoutExpired:
        pass
    OUT.write_text(json.dumps(report, indent=2, default=str))
    log(f"wrote {OUT}")

if __name__ == "__main__":
    main()

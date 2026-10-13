# Broker capability recipe

Pinned image, pulled never, platform `linux/arm64`:

`docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`

Observed identity: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`.

Authorized broker process, root, and only these capability flags:

```
--user 0:0
--cap-drop ALL
--cap-add CHOWN
--cap-add SETUID
--cap-add SETGID
--security-opt no-new-privileges:true
```

The proof container also passed `--group-add 65533`. That supplementary group is how the equal-parent check was observed before the capability check. It is not a fourth capability. `CapEff` must contain bits 0, 6, and 7 (`0xc1`). Exact equality with `0xc1` is what this image showed. The library accepts any `CapEff` that includes those three bits, then requires `NoNewPrivs` of 1.

Proof container shape that matched `linux/broker-3/hostconfig.txt`:

```
--platform linux/arm64
--pull never
--init
--memory 6g
--memory-swap 6g
--cpus 1
--pids-limit 4096
--network <private bridge>
-e HOME=/tmp
```

Recorded host config: `Privileged=false`, `PidMode` empty, `NetworkMode=oo-a417-eef831aa-net`, `CapDrop=["ALL"]`, `CapAdd=["CAP_CHOWN","CAP_SETGID","CAP_SETUID"]`, `GroupAdd=["65533"]`, `SecurityOpt=["no-new-privileges:true"]`, `Memory=6442450944`, `NanoCpus=1000000000`.

Leave these off the broker: `--privileged`, host network, the Docker socket, host PID, and a host or root `HOME`. The proof network was a private bridge. It was not `internal: true`. This recipe does not claim network containment.

Child credentials used by the proof: uid `65532`, gid `65532`, supplementary `Groups` empty. After `setuid` from euid 0, `CapEff`, `CapPrm`, `CapInh`, and `CapAmb` were 0 and `NoNewPrivs` was 1. `CapBnd` stayed `00000000000000c1`, the same as the broker. Do not require `CapBnd` to be 0.

The broker `CapEff` does not include `CAP_KILL` or `CAP_DAC_OVERRIDE`. A root `kill` of the uid 65532 child returns `EPERM`, and `Child::wait` would then block. The harness asks a same-uid `/bin/sh` helper to `kill -9` the child when its stdin closes. Do not add `CAP_KILL` to this profile.

`Command::uid` on the pinned std clears supplementary groups and ignores `EPERM` from `setgroups`. The harness reads `/proc/<pid>/status` after spawn and fail-closes unless `Groups` is empty.

Directory modes are caller work. See the handoff under the evidence directory. `assign_directory` only `fchown`s an already-prepared directory opened with `O_DIRECTORY|O_NOFOLLOW`.

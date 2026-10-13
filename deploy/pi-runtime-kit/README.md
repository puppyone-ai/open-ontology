# Pi runtime kit

Reusable development prefix for Pi 1.0.4, Node 22.23.3, bash, ripgrep, `oo`, and `query-pg`. The procedure is `linux/install-launch.sh`, then `linux/probe-launch.sh`. Hashes and the consume manifest are produced by that run. They are not invented here.

This directory does not accept SP10-W04, SP10-W05, MP-03, or the short-term goal.

## Reproduce

The pinned Rust image must already be local. The scripts do not pull it.

```bash
bash deploy/pi-runtime-kit/linux/install-launch.sh
bash deploy/pi-runtime-kit/linux/probe-launch.sh
```

The install uses one private bridge and one 6 GiB arm64 builder. The probe uses `--network none` and the same builder slot after the install container is removed. `oo` and `query-pg` compile with `cargo build --locked --offline` from a clean checkout. The kit crate is not a member of the root workspace and is not added to the root manifest.

## Export

Volume `oo-a406-eef831aa-pi-kit` at `/opt/pi-runtime-kit`. a397 mounts that volume read-only at the same path on the same image. `deploy/pi-runtime-kit/KIT-HANDOFF.json` is a copy of the probe output. The volume is the binary bundle. Git does not store it.

`manifest.sha256` in the volume lists the deliverable files. `kit verify` checks those hashes. `kit cmap` writes the CJK ranges from the font file.

## Layout

See `CONTRACT.md` for paths, the probe, and the resource limits. See `RECIPE.md` for the origin of each pin. See `NOTICE.md` for the licenses.

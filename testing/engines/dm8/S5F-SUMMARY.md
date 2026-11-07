# E-S5f: official ARM64 DM8 package builds natively

Plan: S5-E-03. Evidence: `e-s5f-20261105`. Date: 2026-11-05.
Authority: E340 plus this task card's explicit same-page ARM64 acquisition.

The official anonymous page `https://eco.dameng.com/download/` exposes
Kunpeng 920 / UOS20 through its own download API:

```text
https://eco.dameng.com/eco-download-server/cpu/os/table/page/data
https://eco.dameng.com/eco-download-server/cpu/os/table/download/32/4/15
https://download.dameng.com/eco/adapter/DM8/202607/dm8_20260710_HWarm920_uos20_64.zip
```

The HTML, page JavaScript, API responses and download log are retained.
No registration, personal information, account, customer system or host
emulation settings were used or changed.

| Artifact | SHA256 |
|---|---|
| ZIP | `b67acb5cfd6bac926ddda72d22f312dab9b57b57dfd1f0aa6b2cb90da5c1da89` |
| ISO | `476f62b1d13acba66615cd3e69a1a74224cd6a2a08843a7477e01b225de00d5e` |
| DMInstall.bin | `e73f312e9bf083aef737a7c37f35c03d24be440d7622dc359cb11609b36291b9` |
| Built image ID | `sha256:8ddae8147e7a3a3f9bb20c0988a35857dfcaf1bbf2944a1babc5a7f513274f75` |

The ISO hash matches the checksum delivered inside the ZIP (not an
independent authenticity signature). Its checksum text is GB18030; the
initial UTF-8 decoding error is retained, not treated as verification.

Reproduction:

```sh
python3 testing/engines/dm8/build.py \
  --archive evidence/e-s5f-20261105/dm8-arm.zip \
  --run-id e-s5f-20261105 --owner-authorised-e340 --arm64
```

The checksum-gated builder completed with exit 0, platform `linux/arm64`,
label `oo.lane=e`. Ubuntu 22.04 base remains digest-pinned. This proves
native installation on that base, not official Ubuntu support for the UOS
package. `disql help` in the built arm64 image exits 0; unlike the old
x86 package, it does not die with `Illegal instruction`. This short-lived,
run-labelled container used `--rm`. The local image was not pushed.

`python3 -m unittest discover -s testing/engines/dm8 -v`: 6 pass, including
wrong ARM package rejection before extraction/execution.

DM8 T0/T1/T8, initialized server and SourceEngine: not run/not implemented.
The card's ARM64 acquisition/build exit is met. An x86-only/D1 downgrade
would now be incorrect. The old x86 failure evidence is preserved.

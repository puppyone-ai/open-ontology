# L3 读懂文件 · OO 侧证据（D80 阶段一，**未签收**）

状态：**未签收**。原因：S3 格子（RustFS、MinIO）与 `download`、`s3_rustfs` 没有被独立复核重跑——所需的固定镜像本机没有，拉取镜像需要 owner 批准（`result-verify-signoff.json` 的 `verify_runs`：在 4cee27056 上 0/2 个格子运行，`download` 与 `s3_rustfs` 无端点）。其余验收项的证据见下表。OO `feat/super-ontology` @ 4390b2cb4。独立复核运行于 `acf198b17`（分支 `so/V1-signoff`，2026-11-18，验证员与执行者分离），结果 `result-verify-signoff.json`：131 项检查，131 通过、0 失败（验收脚本 99/99、faults 9/9、cold1000 8/8、reprocess 4/4）。镜像摘要、二进制 sha256 记录在该文件里；harness 使用 debug 二进制（cold1000 的内存与时间是 debug 构建的数字）。Desktop 部分（30 §3 L3 验收 7 与前端交付）按 30 §7 在阶段二补齐。

## 30 §L3 验收（OO 侧口径，30 §7.2）

| # | 验收项 | 结果 | 证据 |
|---|---|---|---|
| 1 | 评测集每种格式解析成功率 ≥ 98%，失败文件有可读错误码 | 通过：51/51 = 100%（md 18/18、txt 3/3、csv 3/3、xlsx 4/4、docx 10/10、pptx 6/6、pdf 7/7）；13 个对抗文件全部落在规定状态与错误码（加密 docx/pdf、旧版 .doc、两个 zip 炸弹、普通 zip、路径穿越 docx、gb18030 txt、带线表格 pdf、合并区与隐藏表 xlsx、两种 csv 方言）；不发布的文件都带错误码且都在 `errors.v1.json` 里。每种格式只有 3–18 个文件，98% 在小集合上等于“全部通过” | `result-verify-signoff.json`、`l3-golden-states.json` |
| 2 | 故障注入：杀 sidecar、断 PG、对象存储 5xx、写到一半的文件 | 通过：`faults` 9/9（含 PG 重启、sidecar 被杀、sidecar 5xx、半写文件、引擎被杀后租约续跑）；对象存储 5xx/429、变更中读取、坏传输由 `s3_mock` 单元覆盖。注：较早一次复核（`verify-l3-tests.md`）里 PG 重启用例曾挂起，之后用 90 秒截止与端口断言修复，签收复跑 9/9 | `result-verify-signoff.json`、`faults.md` |
| 3 | 毒丸：最多重试 3 次后隔离，吞吐不受影响 | 通过：毒丸 pdf 在第 2 次崩溃后隔离为 `parse_failed`/`DOCS_PARSE_QUARANTINED`（35.4 s，杀了 2 次 worker）；同库另外 16 个文件 16/16 发布，0 个需要第二次解析作业；隔离之后 sidecar 仍可解析 | `result-verify-signoff.json` |
| 4 | 幂等：同内容 10 个文件名只解析一次 | 通过：md、pdf、docx、xlsx 各 1 个 `published` + 9 个 `duplicate`，每种内容 1 个解析作业与 1 个解析结果，10 个路径各有等量证据 | `result-verify-signoff.json` |
| 5 | fs 与 s3 两个后端各跑“新增 → 修改 → 重命名 → 删除” | fs 通过；s3 未独立复核（阻塞）：新增链 `stored→…→published`，修改走 `superseded→stored`，旧证据撤回（0 条有效）新证据 17 条有效，重命名为 `duplicate` 且有 `file.renamed`、没有事实被撤回，删除后证据撤回并撤回无证据的事实（2→7）。s3：**不是本次独立复核运行的**；RustFS 与 MinIO（`pgsty/minio` 分支）各 20/20 的结果来自执行者较早一轮（`s3-life-*.md`），L7 阶段又用本地 MinIO 跑通 `s3_rustfs` 7/7 与 `download` 3/3；独立验证员在 4cee27056 上尝试运行 s3 格子、`download`、`s3_rustfs`：全部 BLOCKED，未运行（镜像不在本机，按主机规则不拉取）。**因此验收 5 的 s3 半边没有独立证据，这是本闭环未签收的原因。**CRC64：RustFS/MinIO 对未带校验上传的对象不回 CRC64，校验路径用进程内 S3 仿真验证 | `result-verify-signoff.json`（fs）、`s3-life-rustfs.md`、`s3-life-minio.md`（执行者运行） |
| 6 | 1000 份文件冷启动解析 ≤ 30 分钟，内存在 10 分册上限内 | 通过：1000/1000 `published`，冷解析 13.7 分钟（总 824.2 s，72.8 文件/分钟）；峰值内存 docs-engine 54 MiB（上限 1.5 GiB）、doc-parser 1675 MiB（3 GiB）、docs-db 827 MiB（1 GiB）。样本为 1000 个黄金文件变体，共 3.7 MiB、pdf 为一页，内存数字针对该文件组合 | `result-verify-signoff.json`、`bench-cold1000.md`（较早一轮 23.8 分钟，已被取代） |
| 7 | 前端：每个状态在界面上有呈现，断线重连不丢状态 | 阶段二（Desktop 状态矩阵逐项截图、上传队列、批量重试、“解析结果”标签页、S3 登记表单、大目录虚拟滚动）。OO 侧对应：状态机与枚举契约 `conformance_state`、`contract`、`conformance_jobs` 通过 | `result-verify-signoff.json` |
| 8 | 自动感知；延迟 D49（2026-11-17 校准：md/txt p95 ≤ 7 s，pdf/docx ≤ 30 s，xlsx ≤ 20 s） | 通过（轮询与 hint 两种模式）：md/txt 6.17 s（n=20，p50 5.67）/ 6.07 s（n=10）；pdf 8.25 / 8.14 s；docx 6.20 / 4.16 s；xlsx 6.19 / 6.02 s。没有 API 调用即可检测直接写入 Drive 目录的文件。测量用的是 1 页 pdf/docx 的黄金文件，真实 20 页文件的 30 s 余量会被消耗。注：原规格 5 s 在默认值（settle 2 s、发布窗口 2 s）下做不到，D49 已校准，旧的 FAIL 记录见下方历史 | `result-verify-signoff.json`、`l3-latency-*.json` |
| 9 | 定向同步不触发全量清扫；新鲜度七种状态可见 | 通过：定向同步发布新文件，generation 695→695，磁盘上已删除的文件仍列出，下一次全量同步清扫（695→696）；`up_to_date`、`syncing`、`processing`、`publishing`、`waiting_review`、`degraded`（根目录不可读，恢复后自愈）、`held`（大批量删除 120 个，422 `DOCS_SOURCE_MASS_DELETE_HELD`，确认后放行）全部观察到 | `result-verify-signoff.json` |
| 10 | 精确失效：只改格式的 DOCX 不重新抽取 | 通过：新版本 `indexed` 且 `reused_from`（按块 id），证据 20 条迁到新版本、旧版本 0 条有效、没有事实被撤回；改一个词的对照版本照常重新抽取 | `result-verify-signoff.json` |

## 横切门禁（L3，OO 侧）

- 状态机契约：后端侧通过（`conformance_state`、`contract`、`conformance_jobs`）；前端状态矩阵对齐在阶段二。
- 形式化（80 §8 L3，D81 修订）：FM2 全量、FM3（fs）FAST 通过且与实现一致（`conformance_state`、`conformance_sync`），KP7 通过；见 `formal.md`。FM3 的 s3 部分被 D81 删除，s3 行为由验收 5 的 s3 格子覆盖；KP3、KP4 没有 Rust 代码可证，**未做**。
- SBOM 许可门禁（D64）：doc-parser 镜像通过，5 个弱 copyleft 组件已审阅，见 `sbom-doc-parser.gate.txt`（执行者运行，本次独立复核没有重跑）。

## 已知问题、复跑与 owner 事项

- 解除阻塞需要 owner 批准拉取固定镜像（`rustfs/rustfs`、`pgsty/minio`，摘要见 `result-verify-signoff.json`），之后重跑 `tools/oo-test/super_ontology_s3.py` 两个格子、`download` 与 `s3_rustfs`。s3 格子不在独立复核内（见验收 5）；MinIO 官方镜像已离开 Docker Hub，兼容格子用 `pgsty/minio`（按摘要固定）。
- 较早的独立复核（`verify-l3-tests.md`）曾因主机无法访问镜像仓库而无法签收，并记录 PG 重启用例挂起；本次签收复跑已取代该结论，该文件保留作历史。
- V2 审计提到约 14 个 `#[ignore]` live 目标没有持续门禁入口，L3 的 `faults`、`reprocess`、`conformance_*` 由签收复跑手工执行，不是 CI 自动门禁。
- Docker 的 Docling 解析路径要求本机有容器运行时；免 Docker 运行时见 L8。

## 复现

```
tools/oo-test/oo-test l3 run --suite super-ontology --run-id <id>                   # 验收 1、3–5、8–10
tools/oo-test/oo-test l3 run --suite super-ontology --only faults --run-id <id>     # 验收 2
tools/oo-test/oo-test l3 run --suite super-ontology --bench cold1000 --run-id <id>  # 验收 6
tools/oo-test/super_ontology_s3_life.py --store rustfs|minio                        # 验收 5 的 s3 部分
```

## 历史记录（已被签收结论取代）

下面是签收前的两份记录：2026-11-17 的独立复核（当时被阻塞，未签收）与更早执行者运行的证据。其中的 md/txt 5 s 目标、23.8 分钟冷启动等数字已被上面的签收结论取代。

# L3 sign-off verification (verify-L3), NOT SIGNED OFF on this HEAD

Branch `so/verify-L3` on `feat/super-ontology` e82490f57, 2026-11-17. Details: `verify-l3-tests.md` (same folder).
Result: **the OO-side sign-off is blocked, not passed.** This host could not reach any registry, so the
locked `docs-engine` image (and therefore the full stack runner, compose check and both S3 cells) could not
be run. The table below separates what was run on this HEAD from what is carried forward from the earlier run
(branch `so/B-S3-05-pipeline-routing`, commit 7834b28c2, section "History" below). Carried-forward numbers
are not verification at this HEAD.

| # | Acceptance (30 section L3) | verify-L3 status | Measured on this HEAD | Evidence |
|---|---|---|---|---|
| 1 | Golden set parse success >= 98 %, readable error codes | NOT VERIFIED here (blocked: engine image) | none this run; earlier run 51/51 (100 %) | `verify-l3-tests.md`; earlier: `l3-run.txt`, `l3-golden-states.json` |
| 2 | Fault injection: sidecar kill, PG disconnect, 5xx, half-written file | **FAIL (open)** | `sidecar_supervision` 6/6 pass; `faults` 8/9 pass; PG-restart case hung (no result after 20+ min, killed) | `verify-l3-tests.md`, `faults-run.txt` (earlier) |
| 3 | Poison file: <= 3 attempts, then quarantine, throughput unharmed | fault-level PASS; full-stack NOT VERIFIED | `faults`: poison pill quarantined after 2 crashes, rest flows (pass) | `verify-l3-tests.md`; earlier: `l3-run.txt` (quarantined at attempt 2, 33.4 s) |
| 4 | Idempotence: 10 copies parsed once | NOT VERIFIED here | none this run; earlier: 1 published + 9 duplicate | earlier: `l3-run.txt` |
| 5 | fs and s3 rounds: add, modify, rename, delete | NOT VERIFIED here; s3 cells blocked | none this run (`download` ignored tests need an S3 endpoint, not run) | earlier: `s3-life-rustfs.md`, `s3-life-minio.md` |
| 6 | 1000 files cold parse <= 30 min, memory limits | NOT VERIFIED here | none this run; earlier 23.8 min, peak 66 MiB / 1697 MiB / 411 MiB | earlier: `bench-cold1000.md`, `bench-cold1000.json` |
| 7 | Desktop frontend status matrix | phase two (D80) | not in this phase | section below |
| 8 | Auto detection, D49 latency (md/txt p95 <= 7 s, pdf/docx <= 30 s, xlsx <= 20 s) | NOT VERIFIED here | none this run; earlier md/txt p95 6.13 s (poll) and 6.11 s (hint), pdf 8.26 s, docx 6.21 s, xlsx 6.24 s | earlier: `l3-latency-poll.json`, `l3-latency-hint.json` |
| 9 | Targeted sync does not sweep; seven freshness states visible | NOT VERIFIED here at full stack; API/conformance level PASS | `freshness_live` 3/3, `conformance_freshness` 1/1 | earlier: `l3-run.txt` |
| 10 | Format-only DOCX change does not re-extract | NOT VERIFIED here | none this run | earlier: `l3-run.txt` |

Note on item 8: D49 was recalibrated on 2026-11-17 to md/txt p95 <= 7 s (spec 00). The earlier 5 s
FAIL in this README's history no longer applies to the spec, but the 6.1 s measurement is from the earlier run.

### Other test results on this HEAD (verify-L3)

- `cargo test -p oo-docs --locked`: exit 0 (unit 338 passed, integration 0 failed).
- `cargo test -p oo-connectors --features objectstore-s3 --locked --lib objectstore`: exit 0 (24 passed).
- `--ignored` suites: all pass except the following:
  - `reprocess`: 3/4 pass. The `a_parser_upgrade_marks_files_outdated_and_reprocesses_them_in_small_batches` assertion
    expects 2 queued reprocess jobs (`reprocess.rs:300`), the code gives 1 because `REPROCESS_INFLIGHT = 1`
    (`crates/oo-docs/src/jobs/stages.rs:60`). Both come from commit f2b604e99 and the spec gives no number
    (10 section 11.2 says only "low priority, small batches"). This is a mismatch between test and code;
    not changed here. Needs a development agent or a coordinator decision.
  - `faults`: `postgres_restarted_under_a_running_engine_loses_nothing` hung after `docker restart` of the
    labelled PG container (no result after 20+ min). Killed. Not a pass.
  - `download` (3 ignored tests) did not run: needs `--features objectstore-s3` and an S3 endpoint.
- `eval_live` and `faults` failed on the first pass only because required env vars were missing from the
  invocation (`OO_EVAL_OUT`, `OO_DOCS_TEST_PG_CONTAINER`). Reruns with the variables passed (`eval_live` passes).

### Gates in force at L3 (30 section 4)

| Gate | Result this run | Evidence |
|---|---|---|
| State-machine contract (front/back enum consistency) | backend side pass: `conformance_state` 1/1, `contract` 3/3, `conformance_jobs` 1/1. Desktop-side matrix comparison is phase two. | `verify-l3-tests.md` |
| Fault injection suite | 8/9 pass, PG-restart hung | `verify-l3-tests.md` |
| SBOM licence gate (D64: fail on forbidden licence or missing audit record) | NOT re-run (needs the doc-parser image). Earlier gate result: pass, five weak-copyleft components reviewed. | `sbom-doc-parser.gate.txt`, `sbom-doc-parser.json` (earlier) |

### Phase two (Desktop, not verified here, D80)

- Item 7 (status matrix screenshots), the upload queue, bulk retry and the parse-result tab.
- Drive registration form for S3 (endpoint, bucket, prefix, `secret_ref`), virtual scrolling for large Drives.
- Desktop reconnect without losing status (item 7).
- Desktop end-to-end run of items 1 to 10 and the Desktop part of the gates.

### Changes made by verify-L3

- `tools/oo-test/super_ontology_compose.py`: `LANE` set to `super-ontology-verify` (was `super-ontology-be2`), so this lane
  labels and cleans up only its own containers. Test-script change only.
- No product code changed. `deploy/docs/images.lock.json` unchanged.

## History (earlier run, branch so/B-S3-05-pipeline-routing at 7834b28c2, 2026-11-17)

# L3 evidence, "read the files" (30 section L3 acceptance 1-10, OO side)

Branch `so/B-S3-05-pipeline-routing` (on `feat/super-ontology` at 7834b28c2), 2026-11-17, local arm64
Docker Desktop. The stack is the real one: `oo serve --runtime compose` (docs-db, docs-engine and the
Docling doc-parser in containers, the daemon on the host). No model, no network for the parser (D17).
Per spec 00 D80 nothing here needs Desktop; UI items are verified through the API.

Result: 8 of the 9 testable items PASS; item 8 passes for pdf, docx and xlsx and **misses its md/txt target**.
Item 7 is a Desktop item (not run in this phase).

| # | Acceptance | Result | Numbers | Evidence |
|---|---|---|---|---|
| 1 | Golden set: per format parse success >= 98 %, every failure has a readable code | **PASS** | md 18/18, txt 3/3, csv 3/3, xlsx 4/4, docx 10/10, pptx 6/6, pdf 7/7 (51/51 = 100 %); the 13 hostile fixtures end in the state and code of 10 section 4.4 (encrypted docx and pdf, legacy .doc, two zip bombs, plain zip, traversal docx, gb18030 txt, ruled-table pdf, merged and hidden xlsx, two csv dialects); every non published file carries a code that is in `errors.v1.json` | `l3-run.txt`, `l3-golden-states.json`, `sidecar-tests-opencv.txt` |
| 2 | Fault injection: sidecar killed mid parse, PG disconnect, object store 5xx, half written file; the rest keeps flowing, nothing is stuck | **PASS** | see `faults.md` (9 engine tests, 6 sidecar supervision tests, s3 5xx with retries) | `faults.md`, `faults-run.txt` |
| 3 | Poison: at most 3 attempts, then quarantine, throughput unharmed | **PASS** | quarantined after 2 crashes (attempt 2) in 33.4 s, 2 worker kills; the 16 other files published 16/16, none needed a second parse job; the sidecar parses afterwards | `l3-run.txt` |
| 4 | Idempotence: 10 copies parsed once | **PASS** | md, pdf, docx, xlsx: 1 published + 9 `duplicate`, 1 parse job and 1 parse result per content, every one of the 10 paths has its own (equal) evidence | `l3-run.txt` |
| 5 | fs and s3 rounds: add, modify, rename, delete | **PASS** (fs) / **PASS** (s3 on RustFS and on MinIO) | fs: chains `stored>...>published`, modify goes `superseded>stored`, old evidence withdrawn (0 live) and new live (15), rename = `duplicate` with evidence on the new path and `file.renamed`, no fact retracted, delete withdraws evidence and retracts facts left without it (1 -> 5). s3: 20/20 checks on each store, including a same size modify and rename by copy+delete | `l3-run.txt`, `s3-life-rustfs.md`, `s3-life-minio.md` |
| 6 | 1000 files cold parse <= 30 min, memory inside the 10 section 12.9 limits | **PASS** | 1000/1000 published in **23.8 min** (1425 s, 42.1 files/min); peak memory engine 66 MiB (limit 1.5 GiB), doc-parser 1697 MiB (3 GiB), docs-db 411 MiB (1 GiB) | `bench-cold1000.md`, `bench-cold1000.json` |
| 7 | Frontend status matrix | not in this phase | Desktop item (D80) | - |
| 8 | Auto detection and latency (D49) | **md/txt FAIL**, pdf, docx, xlsx PASS | see the latency table below | `l3-latency-poll.json`, `l3-latency-hint.json`, `l3-latency-hops-*.txt` |
| 9 | A targeted sync does not sweep; the seven freshness states are visible | **PASS** | targeted sync published a new file, generation 670 -> 670, the file deleted on disk was still listed, the next full sync swept it (670 -> 671); `up_to_date`, `syncing`, `processing`, `publishing`, `waiting_review`, `degraded` (`DOCS_SOURCE_ROOT_MISSING`), `held` (`DOCS_SOURCE_MASS_DELETE_HELD`, 120 would be deleted, confirmed with the generation) all observed in the events | `l3-run.txt` |
| 10 | A format-only DOCX change does not run the extraction again | **PASS** | second version `indexed` with `reused_from` (by block ids), evidence 3 -> 3 live, none left on the old version, no fact retracted; a changed word is extracted again (control) | `l3-run.txt` |

Finder drop (item 8, "no API call"): files written straight into the Drive directory were detected by the poll (1 s in the
first measurement, default 30 s in the second together with the targeted sync hint the Desktop watcher sends, D46) and
reached `published` without any call to the API except the hint.

## Item 8, latency (D49: md/txt p95 <= 5 s, <= 20 page pdf/docx <= 30 s, xlsx <= 20 s)

Drop to `published`, measured from the client, 100 ms polling of the file list.

| group | mode | n | p50 | p95 | target | |
|---|---|---|---|---|---|---|
| md/txt | 1 s poll | 20 | 5.45 s | 6.13 s | 5.0 s | **FAIL** |
| md/txt | hint (30 s poll) | 10 | 4.05 s | 6.11 s | 5.0 s | **FAIL** |
| pdf | 1 s poll | 10 | 8.04 s | 8.26 s | 30 s | pass |
| pdf | hint | 5 | 8.10 s | 8.15 s | 30 s | pass |
| docx | 1 s poll | 10 | 6.03 s | 6.21 s | 30 s | pass |
| docx | hint | 5 | 4.01 s | 4.13 s | 30 s | pass |
| xlsx | 1 s poll | 10 | 6.06 s | 6.24 s | 20 s | pass |
| xlsx | hint | 5 | 4.08 s | 6.21 s | 20 s | pass |

Where the md/txt time goes (median seconds after the file was first seen, `l3-latency-hops-*.txt`):

| step | 1 s poll | hint |
|---|---|---|
| `queued` (hashed, after `settle_seconds` = 2) | 2.29 | 2.44 |
| `parsing` / `parsed` (pump tick 250 ms) | 2.64 | 2.96 |
| `extracting` / `indexed` | 3.14 / 3.15 | 3.46 / 3.47 |
| `published` (publish window of 2 s, 10 section 4.7) | 4.84 | 3.98 |

The floor of an md/txt file is therefore `settle_seconds` (2 s, spec default) + about 1 s of stage hand-offs + the
2 s publish window (0.5 s to 1.7 s observed, a file that is indexed just after the window's job has run waits for the
next window) + up to 1 s of polling when there is no hint. With the spec defaults the 5 s target cannot hold at p95 even
with the hint (p50 is 4.05 s, p95 6.11 s). D49 says "estimate, calibrated after L3". Options for the coordinator:
calibrate md/txt to p95 <= 7 s with the defaults, or change the defaults (publish window 1 s, pump tick 100 ms would take
about 1.5 s off). The engine constants were not changed here because both numbers are in the spec.

## Not covered or weaker than the table suggests

- The golden set has 3 to 18 files per format; 98 % is therefore "all of them" (one miss would be 2 to 33 %). The bench
  (1000 synthetic variants of the same files, all published) is the larger sample.
- The bench files are small (3.7 MiB for 1000 files, one page pdfs); the memory figures are for that mix.
- The 20-page pdf/docx latency was measured with the one-page golden files, so the 30 s target has headroom here that a
  real 20-page file will use.
- CRC64: RustFS and MinIO return no CRC64 for objects uploaded without a checksum (probe says `crc64: None`); the
  verification path is tested against an in-process S3 look-alike (`crates/oo-connectors/tests/s3_mock.rs`).
- The official MinIO image left Docker Hub; the compatibility cell runs the drop-in fork `pgsty/minio` (pinned by digest).

## Reproduce

```
tools/oo-test/oo-test l3 run --suite super-ontology --run-id <id>                # items 1, 3-5, 8-10
tools/oo-test/oo-test l3 run --suite super-ontology --only faults --run-id <id>  # item 2
tools/oo-test/oo-test l3 run --suite super-ontology --bench cold1000 --run-id <id>  # item 6
tools/oo-test/super_ontology_s3_life.py --store rustfs|minio                       # item 5, s3 half
```

# MySQL 当前可用性报告

2026-10-16T16:42Z 只读验收通过。固定代码是 `9b9ecb5c82c50f946393007982725dd4481fa888` 与指导提交 `b5a638ceffc55a6c6235d4cd58ba54d1d0c9fbfa`。四条当前 serve 转交 root 继续运行。正式父项、正式源计数、发行和十源 review 仍由 root 签收。

## 四条当前 serve

同一 C7 ELF `c7a216a654c650a496b43d20fc34cfccd1a8e9e8cce863d53ddea142434221a0`。容器 `oo-mysql-rust-runtime-linux-v8` 的 `ps` lstart 为 Fri Oct 16 16:22:31 2026，与 quiet 步骤纪元 2026-10-16T16:22:36Z 同一时钟。operator `serve_seconds` 为 86400，24 小时上限约 2026-10-17T16:22:31Z。`period_ms` 为 5000。默认全局 registry 未改，这四条进程未停。

| 链 | PID | generation | source | pack | Gateway |
|---|---:|---:|---|---|---|
| new-mysql57 | 25914 | 3 | `mysql_67a12c47d5d81951deb15a1b` | `4a69f8ddc5776dfad134cec694c5eb3225dfcea8114061ef6e55d12a76737611` | `http://127.0.0.1:44967` |
| new-mysql8 | 25925 | 3 | `mysql_4295e1915aeac2f9cbb8bf62` | `c488b2ddcbafa2befde082bcb732750076d89c56bac9c2709b84ec179018b8ca` | `http://127.0.0.1:43253` |
| original-mysql57 | 25907 | 11 | `mysql_911437f094f3278dab47b513` | `82c91935fa53a1d3d4402d14d91e93263dcc934b2ad11bcf22f9b36565bcc3c8` | `http://127.0.0.1:41635` |
| original-mysql8 | 25913 | 15 | `mysql_67fff4e8ebff9f91bee8c75e` | `209ecc4c5e21107543c45cbc5ea5d696a829f1707ddf7215f32899b32006798d` | `http://127.0.0.1:46165` |

binding 在 operator JSON 的 `onboarding.binding`：`b6f6fe57-8c94-4654-bcf8-a2d626414e56`、`e81b0c0e-02c7-464c-9b0f-2ea86f606907`、`150a87a1-24d7-406c-bbd1-fc598759995e`、`6e824358-6b96-4ddf-9f24-e02b1eaccbc5`。通过时每条 binding 恰好一个 C7 `source-onboarding-linux serve`。typed oracle 四条均为 `different_cells=0`、`rows=110019`、`s5_reused=true`、`readonly_replica=true`。Gateway ELF `d16f9729db8c98faac59d91004cf8a322ae5283cabf054e29f1114ee3d53dc6d`。CLI ELF `fb8073fd6e01822b6d42ded74ae3673cf202e778b54bdca0ca044c909bb56703`。挂载仅为 `/fixed` ro 与 `/run-current`。

quiet 观察日志 `/tmp/oo-mysql-avail-grok47-continue/watch-canonical-quiet.log` 在 2026-10-16T16:33:05Z 打印 DONE。当时 original 两条 span 627.2 秒，new 两条 span 628.9 与 628.6 秒。随后只读验收器测得 span 1138.752、1141.802、1140.900、1140.898 秒，末步年龄 3.2–5.8 秒。

## 客户端期限

官方续签收据 `/tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1/availability-v1/token-renewal-0600-v3/receipt.json`，协议 `oo.mysql.client-token-renewal/v3`，时间 2026-10-16T16:28:04.875762Z。`ready_pins_changed=false`，`serves_started=0`。账本 `absolute_deadline_ms` 与验收器实测剩余如下。剩余来自 2026-10-16T16:42Z 前后的通过轮，高于演示门槛 1200 秒。

| scope | role | absolute_deadline_ms | UTC | 通过轮 remain_s |
|---|---|---:|---|---:|
| original-mysql57 | `oo_r_mysql57_full` | 1792171682693 | 2026-10-16T17:28:02.693Z | 2781.1 |
| original-mysql8 | `oo_r_mysql_full` | 1792171683319 | 2026-10-16T17:28:03.319Z | 2781.4 |
| new-static-mysql57 | `oo_r_mysql57_avail_full` | 1792171683929 | 2026-10-16T17:28:03.929Z | 2783.2 |
| new-static-mysql8 | `oo_r_mysql8_avail_full` | 1792171684513 | 2026-10-16T17:28:04.513Z | 2783.3 |

发行间隔 3600000 毫秒。当前客户端到上表 UTC 后失效。serve 进程的 86400 秒上限独立于这 1 小时客户端。root 若要在期限后继续 CLI 演示，需官方续签并只替换 mutable `current-client.token`。

## 八轮事实

四组 verdict 均为 `verified`、`turns=2`、`counts_as_random=true`。query_id 在对应 WAL 中，事实单元格与本 turn 成功 query 一致。gold 文件哈希：mysql57 `f6d79827634990b3180e9e1de4d97461cbd63278a55d7196594b2af6812055a4`，mysql8 `cabeba07849cba0a081a8b95920130a2c0ce65a40f1b8d62f71282bc95f11857`。

| 组 | seed | query_id |
|---|---:|---|
| mysql57 group1-resume | 2135070846331562096 | `q_Xl5zHpWBhc0iGrPCRJokEtMONj`，`q_s4LgG1Ewh6yNt3LJwLwiHBkzrr` |
| mysql57 group2-resume | 6351115908580348747 | `q_Y53u7IiI37s78JBjDmWYoPN439`，`q_CqJdYcpjxlIkneXM4PaSXU6j2W` |
| mysql8 group2 | 2078911080907191073 | `q_Am6cqANxB5aibFZKUF4NgU6YG1`，`q_vD4ymLbppFO3LAoPxAQURjFR0D` |
| mysql8 group1-resume5 | 194566442114094847 | `q_aig3lVtxCjYC1T44vx5q1g0VdJ`，`q_AHaOzlJ5kdFP0jcm8UqUNIOgu2` |

本轮验收器对四条 Gateway 的只读 `COUNT(*)` 返回 query_id：`q_sLIqriJsYnxgY836H6Bvwyio6W`、`q_rdQnnRTDp6OWwnnsNyETvrCFS4`、`q_heXv5mHkRvoYNBd0dgZhePL9AV`、`q_QGT8Q4zKfgBBDAaGAv2UED2PXj`。这四条是验收查询，不是上面八个事实 id。

## 用量

12 份 dialogue `transcript.json` 按 model response id 去重：93 个 id，prompt 306484，completion 24140，total 330624。prompt 与 completion 之和等于 total。按文件计的 model 事件是 152，前缀重复，不能把各文件 token 相加。API 记录的 cached prompt 子集合计 158976，已包含在 prompt 306484 内，不另计、不折成节省。

已复用、本轮未再调用的 S5：mysql57 `c989b416-7bd1-94be-bf9f-16b0caa39a22` / `grok-4.7-build` / 6704；mysql8 `fa738499-a65e-97cd-978d-402dc82e233c` / `grok-4.7` / 5832。S5 合计 12536，`llm_calls=1` 且 cache_hit 为 false 的原始调用记录仍在。dialogue 与这两次 S5 合计 343160。任务目标 27411 未达到。历史主线 55 个 response、137055 token 另计，不并入这 343160。本交付轮模型调用 0，源 DML 0。

dialogue 响应模型名只有 `grok-4.7` 与 `grok-4.7-build`。工具失败仍在原文：validate exit 2 共 11，query exit 2 共 27，knowledge_cat exit 2 共 2。两份 GPT `gpt-6.1-sol` HTTP 429 原文仍在，各 1 次，`usage_limit_reached`，成功用量 null：

- `/Users/redux/.cache/oo-mysql-connection-whole/availability/mysql57/work/builds/b_5654171062bc8b8b3b501dd0559ea13a/native-model-output/result.json`
- `/Users/redux/.cache/oo-mysql-connection-whole/availability/mysql8/work/builds/b_195981a39793c4eb7bb7b256d7a46a5c/native-model-output/result.json`

## 验收器与基包

执行体 `/tmp/oo-mysql-connection-whole/sol-20261016/mysql-mainline-v1/availability-v1/verify-current-availability-grok47.py`，SHA256 `914b91637fbf0bd1660a606f3454befb8f59627a3ae03d2de5d8fde50c38279e`。通过轮 `/tmp/oo-mysql-avail-grok47-continue/verifier-run-after-index-fix.json`，SHA256 `9710dc2c9e60408a8caf59b8bfe827cbedd358cc7fe7debcfe1188478bace5b5`，exit 0，`ok=true`，`model_calls=0`，`source_dml=0`。失败原文保留在 `verifier-run-before-index-fix.json`，SHA256 `c68f28e75aa53708d4d65fe41fb9f32cdde3bd54793d1d579fa49bd2517c2cef`。外来 index 排除后仍为 `cb53ce84e87327bdee00e90a0b910c5aad314c301d966601edba019fbf9d22ad`。

Gateway strict clippy 仍是 baseline exit 101，11 条旧 lint，新增 0。比较文件 `gateway-clippy-baseline-comparison-v1.json`。runtime tests/clippy/release 0，Gateway 71 tests 与 release 0，沿原记录，本轮未重跑。

本轮重新计算的不可变基包，未重打包：

| 包 | 字节 | SHA256 |
|---|---:|---|
| `/tmp/oo-mysql-mainline-complete-delivery-v2-20261016.tar` | 696360960 | `5694c0f76824c790f5b30e58e5085262a514343ba545f10b178d482f63643eef` |
| `/tmp/oo-mysql-mainline-final-package-20261016.tar.gz` | 684910370 | `30ef70b64034f317ede82e1e3d2bfe1b4129a2bc7cd28db1ed9cbdff5bec32a5` |
| `.../mainline-source-b60bd6fb52c7-v4.tar` | 84572160 | `26359b01b6bfd6db4e11bf57d1d607e8fb1e853868337c7fcb986f978ca50376` |
| `.../availability-v1/source-delta-9b9ecb5c8.tar` | 235520 | `7c297b30ba3548d4b24ab7bfefca2e1371299ac495bd9f47cf08c7c1374f2ee1` |

`tar -tzf` 列出 final package 71898 行，其中目录项 1278，非目录项 70620。父级 `DELIVERY.json` 未改，SHA256 `a516600bf97e988e5580ce02319bd452c4a558effc7faf612ee2a2ef9d9ffcfb`。`HANDOFF.json` 未改，SHA256 `6a1b16da4092fedce2092fc24f29b714dcc761003c2992f0251675f2ab2aaaab`。单入口与 manifest、增量包见同目录 `availability-v1/DELIVERY.json`。

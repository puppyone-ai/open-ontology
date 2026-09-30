# 16 参考资料

> Status: Living document
> Last reviewed: 2026-09-30
> 说明：以下资料于 2026-09 检索整理，用于支撑本设计中的事实性陈述。标注“待核实”的条目在原型阶段需再次确认。参考资料不代表决策；决策与理由见各文档正文。

## 1. 语义层与“本体”

| 主题 | 要点 | 来源 |
|---|---|---|
| Palantir Foundry Ontology | Object / Link / Action / Function；AIP Agent 通过本体对象与工具工作；OSDK | https://palantir.com/docs/foundry/object-link-types/link-types-overview/ · https://palantir.com/docs/foundry/ontology-sdk/overview/ |
| Snowflake Semantic View / Cortex Analyst | YAML：tables、dimensions、time_dimensions、facts、metrics、relationships、verified_queries、synonyms、sample_values、custom_instructions | https://docs.snowflake.com/en/user-guide/views-semantic/semantic-view-yaml-spec |
| Snowflake Cortex Analyst 准确率（厂商内部数字） | 内部 150 题集 90%+ vs 单提示 GPT-4o 51% | https://www.snowflake.com/en/blog/engineering/cortex-analyst-text-to-sql-accuracy-bi/ |
| Databricks Genie Agents | 示例 SQL、指令、trusted assets；每 agent 表数与指令数上限 | https://docs.databricks.com/aws/en/genie-agents/set-up · https://docs.databricks.com/aws/en/genie-agents/tune-quality |
| Databricks Metric Views | YAML：source、joins、fields、measures、filter、materialization | https://docs.databricks.com/aws/en/uc-semantics/metric-views/yaml-reference |
| dbt Semantic Layer / MetricFlow | semantic_models、entities、dimensions、measures、metrics；MetricFlow Apache-2.0 | https://docs.getdbt.com/reference/semantic-model-properties · https://github.com/dbt-labs/metricflow |
| Open Semantic Interchange → Apache Ossie | 2025-09 发起，2026-01 发布首版规范，Apache-2.0，进入 Apache 孵化器 | https://www.snowflake.com/en/blog/open-semantic-interchanges-specs-finalized/ · https://github.com/apache/ossie |
| Cube | 面向 AI Agent 的语义层；Apache-2.0/MIT | https://github.com/cube-js/cube |
| Malloy | 开源语义建模/查询语言，MIT | https://github.com/malloydata/malloy |
| Wren AI MDL | models、relationships、calculated_fields、views；`generate-mdl` 从库生成骨架 | https://docs.getwren.ai/oss/concepts/what_is_mdl |
| Vanna、DB-GPT、Chat2DB、Dataherald | 开源 text-to-SQL 项目；Dataherald 自 2024-07 未维护 | https://github.com/vanna-ai/vanna · https://github.com/eosphoros-ai/DB-GPT · https://github.com/Dataherald/dataherald |
| MCP Toolbox for Databases / Postgres MCP Pro / DBHub / Supabase MCP | 面向 Agent 的数据库 MCP 服务 | https://github.com/googleapis/mcp-toolbox · https://github.com/crystaldba/postgres-mcp · https://github.com/bytebase/dbhub · https://github.com/supabase/mcp |
| AutoDDG | 画像 + LLM 的数据集描述自动生成（SIGMOD 2026） | https://arxiv.org/abs/2502.01050 · https://github.com/VIDA-NYU/AutoDDG |
| DataHub AI Docs | 基于 schema、血缘、画像、样本值生成文档，待人审核 | https://docs.datahub.com/docs/automations/ai-docs |
| OpenMetadata 2.0 | “面向 AI Agent 的开放上下文层” | https://blog.open-metadata.org/announcing-openmetadata-2-0-the-open-context-layer-for-ai-agents-83b8ce8b9dde |
| OpenAI 内部数据 Agent | 多层上下文（元数据、人工注释、管道代码、组织知识、运行时、记忆）；自动起草描述待人确认 | https://openai.com/index/inside-our-in-house-data-agent/ · https://open-metadata.org/case-study/openai |
| Vercel d0 | 去掉 80% 工具，语义层写成文件 + bash：3.5× 更快、token 减少约 37–40%、成功率 100% | https://vercel.com/blog/we-removed-80-percent-of-our-agents-tools |
| Anthropic 工程文章 | 为 Agent 编写工具；上下文工程；Agent Skills 渐进披露 | https://www.anthropic.com/engineering/writing-tools-for-agents · https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents · https://www.anthropic.com/engineering/equipping-agents-for-the-real-world-with-agent-skills |

## 2. Text-to-SQL 方法

| 方法 | 要点 | 来源 |
|---|---|---|
| DIN-SQL、MAC-SQL | 分解与多 Agent | https://arxiv.org/abs/2312.11242 |
| CHESS | schema 子集选择、LSH + 向量的值检索 | https://arxiv.org/abs/2405.16755 · https://github.com/ShayanTalaei/CHESS |
| CHASE-SQL | 多路候选生成 + 训练的选择器，BIRD 73.0% | https://arxiv.org/abs/2410.01943 |
| XiYan-SQL | M-Schema 表示、多生成器集成，BIRD 75.63% | https://arxiv.org/abs/2411.08599 |
| ReFoRCE | 大 schema 压缩、执行反馈自我修正（Spider 2.0） | https://github.com/Snowflake-Labs/ReFoRCE |
| 元数据对准确率的影响 | 生成的列描述提升 text-to-SQL，尤其大模型 | https://arxiv.org/abs/2408.04691 |

## 3. 管线与存储

| 主题 | 要点 | 来源 |
|---|---|---|
| Debezium Server Redis sink | 每表一个 stream；`memory.threshold.percentage` 背压；`batch.size`；无 tombstone（占位值）；offset/schema history 可存 Redis | https://debezium.io/documentation/reference/stable/operations/debezium-server.html |
| Debezium 版本 | 3.5.x（2026）；3.0 起运行时需 Java 17 | https://mvnrepository.com/artifact/io.debezium/debezium-server-redis · https://debezium.io/releases/3.0/release-notes |
| 增量快照（DDD-3）与信号通道 | signaling 表；MySQL 只读增量快照（GTID） | https://debezium.io/documentation/reference/stable/configuration/signalling.html · https://debezium.io/blog/2022/04/07/read-only-incremental-snapshots/ |
| 只读增量快照的竞态问题 | GTID 水位未变化时的提前结束 | https://github.com/debezium/dbz/issues/2636 |
| Redis Cluster 下的 sink 挂起问题 | Debezium Server 3.4.0 | https://github.com/debezium/dbz/issues/1793 |
| PG TOAST 未变化列 | 占位值与处理方式 | https://debezium.io/blog/2019/10/08/handling-unchanged-postgres-toast-values/ |
| PG 复制槽 WAL 保留 | `max_slot_wal_keep_size` 默认 -1；failover slot（PG17） | https://www.postgresql.org/docs/current/runtime-config-replication.html · https://www.postgresql.org/docs/current/logical-replication-failover.html |
| RDS MySQL binlog 保留 | `binlog retention hours` | https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/mysql-stored-proc-configuring.html |
| MySQL `binlog_row_image` | MINIMAL 导致更新缺列 | https://blogs.oracle.com/mysql/heatwave-mysql-solving-missing-updates-for-debezium-cdc |
| Redis 许可证变化 / Valkey | 2024 RSAL/SSPL；Redis 8 增加 AGPL；Valkey BSD-3 | https://redis.io/legal/licenses/ · https://www.linuxfoundation.org/press/linux-foundation-launches-open-source-valkey-community |
| AOF 持久化语义 | `everysec` 可能丢失约 1 秒 | https://redis.io/docs/latest/operate/oss_and_stack/management/persistence/ |
| MERGE vs ON CONFLICT | MERGE 在并发插入下可能报唯一冲突 | https://pganalyze.com/blog/5mins-postgres-15-merge-vs-insert-on-conflict · https://wiki.postgresql.org/wiki/UPSERT |
| 换表与依赖视图 | RENAME 后视图仍绑定旧表 | https://www.codejam.info/2024/05/postgresql-swap-tables-dependent-views.html |
| 热备冲突 | `max_standby_streaming_delay`、`hot_standby_feedback` | https://www.postgresql.org/docs/current/hot-standby.html |
| PG 18 | 2025-09-25 发布；异步 I/O | https://www.postgresql.org/docs/current/release-18.html |
| 类型映射陷阱 | MySQL 零日期、unsigned、enum/set；Oracle DATE/NUMBER | https://dbconvert.com/blog/how-to-migrate-mysql-to-postgresql/ · https://docs.aws.amazon.com/dms/latest/oracle-to-aurora-postgresql-migration-playbook/chap-oracle-aurora-pg.tables.common.html |
| ADBC / ConnectorX | Arrow 原生数据库访问 | https://arrow.apache.org/adbc/ · https://github.com/sfu-db/connector-x |
| DuckDB Excel | 仅 `.xlsx` | https://duckdb.org/docs/lts/guides/file_formats/excel_import.html |
| MinIO 社区版状态 | 2025-10 维护模式；2026-04 起仅源码分发 | https://www.infoq.com/news/2025/12/minio-s3-api-alternatives/ · https://github.com/minio/minio |
| ARQ 维护状态 | 仅维护模式 | https://github.com/python-arq/arq |
| Procrastinate | 基于 PG 的 Python 任务队列 | https://github.com/procrastinate-org/procrastinate |
| 同类工具许可证 | Airbyte ELv2、Estuary BSL、PeerDB AGPL、Redpanda Connect CDC 企业许可 | https://airbyte.com/blog/move-to-elv2 · https://github.com/estuary/flow · https://github.com/PeerDB-io/peerdb · https://github.com/redpanda-data/connect |
| 列式扩展 | pg_duckdb、pg_mooncake；ParadeDB pg_analytics 已归档 | https://github.com/duckdb/pg_duckdb · https://github.com/paradedb/pg_analytics |
| 对账 | data-diff 哈希分桶算法；上游已归档，reladiff 为维护中的分支 | https://github.com/datafold/data-diff/blob/master/docs/technical-explanation.md · https://github.com/erezsh/reladiff |

## 4. 安全、网关、沙盒与 MCP

| 主题 | 要点 | 来源 |
|---|---|---|
| MCP 规范 2026-07-28 | 无状态核心、MRTR、Tasks、可缓存 list | https://modelcontextprotocol.io/specification/2026-07-28 · https://blog.modelcontextprotocol.io/posts/2026-07-28/ |
| MCP 授权 | OAuth 2.1、RFC 9728、RFC 8707；禁止 token 透传 | https://modelcontextprotocol.io/specification/draft/basic/authorization |
| MCP 安全最佳实践 | confused deputy、会话劫持 | https://modelcontextprotocol.io/docs/2026-07-28/tutorials/security/security_best_practices |
| Supabase MCP 事件 | 工单数据中的注入 + `service_role` 绕过 RLS | https://generalanalysis.com/blog/supabase-mcp-blog · https://supabase.com/blog/defense-in-depth-mcp |
| GitHub MCP 事件 | 恶意 issue 诱导泄露私有仓库 | https://invariantlabs.ai/blog/mcp-github-vulnerability |
| 致命三要素 | 私有数据 + 不可信内容 + 对外通信 | https://simonwillison.net/2025/Jun/16/the-lethal-trifecta/ |
| pglast / libpg_query | PG 真实语法分析器的 Python 绑定 | https://github.com/lelit/pglast · https://pganalyze.com/blog/pg-query-2-0-postgres-query-parser |
| 可写 CTE | `WITH` 中执行 INSERT/UPDATE/DELETE | https://www.postgresql.org/docs/current/queries-with.html |
| RLS 与 LEAKPROOF、security_invoker 视图 | — | https://www.postgresql.org/docs/current/ddl-rowsecurity.html · https://pganalyze.com/blog/5mins-postgres-row-level-security-bypassrls-security-invoker-views-leakproof-functions |
| `default_transaction_read_only` 可被会话修改 | 只读需要引擎层保证 | https://jkatz.github.io/post/postgres/postgres-read-only/ |
| EXPLAIN 代价是估计值 | — | https://www.postgresql.org/docs/current/using-explain.html |
| OPA 部分求值 → SQL 过滤 | — | https://openpolicyagent.org/docs/filtering/partial-evaluation |
| gVisor | 用户态内核、性能指南 | https://github.com/google/gvisor · https://gvisor.dev/docs/architecture_guide/performance/ |
| kubernetes-sigs/agent-sandbox | Sandbox CRD、WarmPool | https://github.com/kubernetes-sigs/agent-sandbox · https://kubernetes.io/blog/2026/03/20/running-agents-on-kubernetes-with-agent-sandbox/ |
| Anthropic sandbox-runtime | bubblewrap/seatbelt + 网络代理 | https://github.com/anthropics/sandbox-runtime |
| E2B | Firecracker，自托管 | https://github.com/e2b-dev/runtime |
| 凭证注入代理 | Fly.io tokenizer；Envoy Gateway 凭证注入 | https://github.com/superfly/tokenizer · https://gateway.envoyproxy.io/docs/tasks/security/credential-injection/ |
| Pi | MIT；RPC 模式（stdio JSON）；2026 年转由 Earendil 维护 | https://github.com/earendil-works/pi · https://mariozechner.at/posts/2025-11-30-pi-coding-agent/ |
| Claude Code headless / Codex app-server | stdio JSON 驱动 | https://code.claude.com/docs/en/headless · https://learn.chatgpt.com/docs/non-interactive-mode |
| OWASP LLM Top 10 2025 / Agentic Top 10 | — | https://genai.owasp.org/llm-top-10/ · https://genai.owasp.org/resource/owasp-top-10-for-agentic-applications-for-2026/ |
| NIST AI RMF GenAI Profile | NIST AI 600-1 | https://nvlpubs.nist.gov/nistpubs/ai/NIST.AI.600-1.pdf |
| CaMeL、Spotlighting | 提示注入的设计型防御与输入变换 | https://arxiv.org/abs/2503.18813 · https://arxiv.org/abs/2403.14720 |
| Markdown 图片外传 | — | https://checkmarx.com/zero-post/exploiting-markdown-injection-in-ai-agents-microsoft-copilot-chat-and-google-gemini/ |
| RFC 8693 Token Exchange | — | https://datatracker.ietf.org/doc/html/rfc8693 |
| OpenBao | Vault 的 MPL-2.0 分支（Vault 自 2023-08 为 BSL） | https://openbao.org/ |
| OTel GenAI 语义约定 | Agent/工具部分仍为开发状态 | https://github.com/open-telemetry/semantic-conventions-genai |

## 5. 评测

| 主题 | 要点 | 来源 |
|---|---|---|
| Spider 1.0 / test-suite accuracy | — | https://yale-lily.github.io/spider · https://aclanthology.org/2020.emnlp-main.29/ |
| Spider 2.0 | 企业级大 schema；lite/snow/DBT 三种设置 | https://spider2-sql.github.io/ · https://arxiv.org/abs/2411.07763 |
| BIRD | EX、VES/R-VES、Soft-F1；evidence 的价值（+20 点） | https://bird-bench.github.io/ |
| 基准标注错误审计 | BIRD Mini-Dev 52.8%、Spider 2.0-Snow 62.8%–66.1% | https://www.vldb.org/pvldb/vol19/p931-jin.pdf · https://www.vldb.org/cidrdb/papers/2026/p5-jin.pdf |
| BIRD-Interact / LiveSQLBench | 交互式、防污染 | https://bird-interact.github.io/ · https://livesqlbench.ai/ |
| BEAVER | 真实企业数仓 + DBA 出题 | https://beaverbench.github.io/ |
| EHRSQL | 含不可回答问题 | https://github.com/glee4810/EHRSQL |
| AmbiQT | 歧义问题 | https://arxiv.org/abs/2310.13659 |
| DABstep、KramaBench、TableBench | 数据分析 Agent 基准 | https://huggingface.co/papers/2506.23719 · https://arxiv.org/abs/2506.06541 · https://tablebench.github.io/ |
| τ-bench / pass^k | 可靠性指标 | https://arxiv.org/abs/2406.12045 · https://github.com/sierra-research/tau2-bench |
| LLM 评审偏差与不稳定 | — | https://arxiv.org/abs/2306.05685 · https://arxiv.org/abs/2606.13685 |
| RBAC 下的 text-to-SQL | 违规率、过度拒绝率、SafeEX | https://github.com/2020dfff/RBAC-Text2SQL-Benchmark · https://arxiv.org/abs/2510.07642 |
| FreshQA | 答案随时间变化的评测设计 | https://github.com/freshllms/freshqa |
| AgentDojo / InjecAgent / ASB | Agent 注入安全基准 | https://agentdojo.spylab.ai/ · https://github.com/uiuc-kang-lab/InjecAgent · https://github.com/agiresearch/ASB |
| Inspect AI | 评测框架（MIT） | https://github.com/UKGovernmentBEIS/inspect_ai |
| promptfoo / garak / PyRIT | 红队工具 | https://github.com/promptfoo/promptfoo · https://github.com/NVIDIA/garak · https://github.com/microsoft/PyRIT |
| Toxiproxy | 故障注入 | https://github.com/Shopify/toxiproxy |

## 6. 待核实项

- pglast 的确切许可证及其对 Apache-2.0 分发的影响（[02 §5](02-technology-stack.md)）。
- Debezium 事件中 PG 事务提交 LSN 的可获得性（[04 §5.1](04-pipeline.md)）。
- Debezium Server 3.5 下 JDBC offset / schema history store 的可用性。
- 托管 PG（RDS/Aurora、Cloud SQL）对 PG17 failover slot 的支持。
- Procrastinate、Hatchet、DBOS、Sequin、Conduit、Supabase ETL、SeaweedFS、Garage、RustFS 的许可证（调研时部分未能核对 LICENSE 文件）。
- 部分二手来源的事件（promptfoo 被收购、SDV 许可证变化）。

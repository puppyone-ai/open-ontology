# 随机对话验收器

`dialogue.py` 仍是薄测试编排。入口仍是 `--config` 和 `--out`。产品查询、权限和类型仍在 Rust CLI / Gateway。本文件不生成题面、gold 或 SQL。

没有 `question_bank` 的配置是 legacy。原 `questions`、原 verdict 字段、原 `resume_transcript` 行为保持，不计随机。legacy 上再写 `seed`、`counts_as_random` 或 `frozen_manifest`，会在模型调用前拒绝。

## 题库

`question_bank` 是候选数组，或该 JSON 数组的文件路径。每个候选只有这四个字段：

- `semantic_id`：`^[a-z][a-z0-9_.-]{0,63}$`，题库内唯一
- `query_class`：非空字符串
- `prompt`：交给模型的问题
- `truth_key`：独立 gold 文件里的键

gold 沿用原结构：`truth_key -> {expected_named_facts: {列名: 文本}}`。源侧在首次模型调用前独立采集 gold。不要从待测 Gateway 的输出倒推 gold，也不要把 gold、源账号或 SQL 写进候选。多出来的字段会被拒绝。验收器不扫描措辞里是否夹带答案。

题库 4 到 8 条，至少四个不同 gold 载荷，且至少两类。只改措辞、只改类名、为同一题另换 semantic_id、或全部期望相同，都拒绝。`sample_size` 默认 2，且至少 2。抽出的序列本身也必须至少有两类不同 gold。

## seed、历史、身份

`identity` 含 `source_id`、`source_generation`（非负整数）、`pack_sha256`（64 位小写十六进制）、`role`。

未给 `seed` 时用 `secrets` 生成，`mode` 为 `random`，`counts_as_random` 为 true。给出 `seed` 时为 `replay`，`counts_as_random` 为 false，同一题库和同一 gold 得到同一序列，不计新的随机运行。

`history` 或 `history_file` 是已用过的 semantic_id 有序列表，二选一。新的 random 抽到相同顺序、或只把同一组题换序，都跳过。`prior_manifest` 指向上一次冻结的 `selection.json`；新的 random 不能再抽到其中任何 semantic_id。题不够时报 `question pool exhausted`。不能把旧的固定记录改标成 random。replay 不因 history 或 prior_manifest 改序列。history 或 prior_manifest 路径不存在则拒绝。

## 冻结与 resume

拒绝非法或重复 semantic_id、缺 gold、缺题库、无语义差、history 非法、resume 错配，都发生在读取 bearer 和任何模型调用之前。

通过后、模型调用前，`--out/selection.json`（0600）写入 `mode`、`counts_as_random`、`seed`、`semantic_ids`、`selected`、`questions`、`gold_sha256`、`identity`、`sample_size`。manifest 不含 gold 值。`gold_sha256` 是 gold 文件字节的 SHA-256。

`frozen_manifest` 指向该文件时不再抽题。gold 字节、identity 四个字段、再次给出的 seed、以及每个候选的四个字段必须一致。序列短于两题或 semantic_id 重复则拒绝。`resume_transcript` 加上 `question_bank` 时必须带同一份 manifest。legacy resume 仍只核 transcript 哈希和 named facts。

随机或 replay 的 verdict 在原字段外增加 `selection`、`counts_as_random`、`seed`、`semantic_ids`、`gold_sha256`。legacy verdict 不增加这些字段。

工具覆盖仍是 `query`、`freshness`、`describe`、`knowledge_cat`。模型可以自选合法 SQL，验收器不断言 SQL 字符串。若模型在这些工具尚未出现时给出最终答案，编排会要求补齐工具后再收答案；轮数用尽仍按原规则失败。现有循环在一轮里还没有 query 结果时，下一步会强制 `query`。`knowledge_cat` 指向包外路径时，`SCHEMA_INVALID` 交回模型，该次调用不计入覆盖。`knowledge_search` 的 `query` 最短长度为 1。CLI 对空查询返回的 `usage` / `search text is missing` 也交回模型，不计入覆盖，不另开重试。最终答案仍只能是 facts 与 evidence；只有 evidence 和 payload、没有 facts、顶层多出 qty 或 count、事实与成功查询单元格不一致、或 evidence 的 query_id 不在本轮成功查询里，仍失败。`verify_named_facts` 的这些拒绝不放宽。

系统消息写明最终对象只有 `facts` 和 `evidence`。`facts` 的值是单元格原文，或带 `value` 的对象。`evidence` 的值只有 `query_id`、`column` 和 `row`；`row` 是从 0 开始的整数。不要把 `query_id` 放进 `facts`，也不要使用 `zero_based_row` 这个键。题面只追加该题的 fact 名字，不追加 gold 值、SQL 或口令。

工具轮不带 `response_format`。本轮已经有成功 query，且要求的公共工具都已出现时，下一次请求才是最终格式：不附带 tools。当前 `FINAL_FORMAT` 是 `json_schema`，`strict` true。schema 只列该题 fact 名字和 evidence 的三个字段。一次真实探测打到 `grok-4.7`，代理请求 `e47c3ac5` 返回 HTTP 200，用时 6.501 秒，上游是 `https://cli-chat-proxy.grok.com/v1`。响应体没有留住，因为证据目录被写成了 0600。这不是 4xx，所以当前路径不用 `json_object`。同一函数在 `kind=json_object` 时返回 `{"type":"json_object"}`，与 `tools/pg-builder/src/generate.rs` 的 `chat_request` 相同，留作未支持时的格式。最终请求若返回 4xx，transcript 记下状态码和错误体后失败，不自动改格式再打一次。

其他源只读固定提交后的这三个文件，在私有目录准备自己的题库和 gold。不要为接入本验收器重跑旧模型或 S5。

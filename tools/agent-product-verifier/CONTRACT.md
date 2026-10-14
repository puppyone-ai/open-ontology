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

工具覆盖仍是 `query`、`freshness`、`describe`、`knowledge_cat`。模型可以自选合法 SQL，验收器不断言 SQL 字符串。若模型在这些工具尚未出现时给出最终答案，编排会要求补齐工具后再收答案；轮数用尽仍按原规则失败。现有循环在一轮里还没有 query 结果时，下一步会强制 `query`。`knowledge_cat` 指向包外路径时，`SCHEMA_INVALID` 交回模型，该次调用不计入覆盖。认证失败和其他工具错误仍中止本轮。

其他源只读固定提交后的这三个文件，在私有目录准备自己的题库和 gold。不要为接入本验收器重跑旧模型或 S5。

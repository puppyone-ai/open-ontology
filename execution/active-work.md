# 当前执行板（Codex维护）

更新：2026-10-08。只汇总最近一次真实句柄观测与启动；不把台账或源码锁当作运行证据。原始历史/冻结验收标准留在specs三个主台账。一次18分钟等待后统一观测，无中途重复轮询。

| 工作线 / UUID尾号 | 实际handle | 最近状态 / 固定来源 | 唯一写入边界 |
|---|---|---|---|
| E55 审计WAL/ingest a371 | 65332 | 统一观测live；未交最终包 | tools/gateway-audit-wal、tools/audit-ingest、自身证据 |
| E56 Self-build API a373 | 56260 | 原42599退出0但无报告；原会话收现有retest并接真实身份 | tools/selfbuild-control-api、自身证据 |
| E57 curated Git a374 | 36873 | 统一观测live；合同9d33bfc06，未交最终包 | tools/curated-git-apply、自身证据 |
| E54 OIDC独立复审 a375 | 89681 | 统一观测live，固定35bd6b815/合同089b | 自身证据；产品只读 |
| 同库迁移复审 a376 | terminal 0 | 三报告/146SHA一致；exact search_path契约缺陷交a378，不签修复完成 | 自身证据；产品只读 |
| 严格search_path修复 a378 | 44280 | 已启动；固定be4b59648，禁止重复启动 | tools/calendar-store、tools/enum-store、自身证据 |
| E51 全dump切点返工 a377 | 86777 | 统一观测live；旧20f065096被a372 WITHHOLD | tools/pg-backup-restore、自身证据 |
| 前端G1 a351 | 42805 | 原45339退出0，M5固定1fce257f41；下一真实B01接入 | 总卡G1；JS lock/SDK/shared/router/deploy唯一owner |
| 前端G2 a353 | terminal 0 | 固定0c3caa184，三报告/43SHA一致；58/59整体测试，字体失败归G1 | 总卡19管理features/自身tests；G1只读 |
| 前端G3 a352 | 67076 | 原18655退出1；M5三报告/72SHA一致；修自身archive与network扫描并复审固定G2 | 总卡review/tests/evidence；产品只读 |

主线开发上限20，独立复审不占主线开发槽；前端总共仅上述三个固定会话（含G3），禁止第四会话。Rust全局10/task1含host/helper/hashbuild，6GiB/1CPU；PG task1 1GiB/1CPU；Node全局2/task1 4GiB/1CPU；browser全局2/task1，实际满则排队。

## 固定来源 / 依赖

- B01：合同089b00d7f；作者真实HTTPS OIDC实现35bd6b815，完整包24hash0；a375未验收。B04可用固定作者服务做集成，不伪造已通过独立审核。
- B04：合同7bb2da3cc；a373尚未交固定实现。前端只能schema_frozen，不能live/假候选/假审核。
- Builder原E47：be9acbc23，经a369原完整范围APPROVE_LIMITED，117hash0；单一combinedPack保留两表批准body。fullSP07/auth/pub未签。
- durable审核ledger：6b3a92c2，E52已验收，但child authenticated=false、尚不Gitapply。服务需真实身份与对象/候选/编译catalog成员关系绑定。
- Git：a374待交付。a373不伪造curated已落Git；后来集成仅消费固定适配器。
- 前端M5：1fce257f41dedd1e460cb4180d5286e256caa10d，106hash0；两条G2旧空数组断言被作者跳过，整体suite不能当绿。G2更新后在固定组合树检查。
- 备份：a372原E51 WITHHOLD，持锁期间view/schema/table/sequence新增被入dump却cut_proven=true，a377修复，禁止仅改变支持范围或关所有readiness来验收。

## 分层计数 / 下一步

后端局部冻结76/90，历史验收101，首次独立组件91，唯一返工15/91；作者95。全量开发分母未冻结。完整87父项/16阶段仍未全签；M1-D17/24；集成0/4；发行0/6；X01–X05真实owner/客户范围/保护CI/runsc/UAT独立pending。企业源连接其他信息未收到，未经配置不能连接；所有实际产品试验本地PG，不使用企业只读账号做直接E2E。

下一未用UUID a379。任务退出先收已有真实资源/phase exit/完整三报告/source/SHA/cleanup，缺报告不能凭CLI0验收、不重复启动现有builder/PG。所有源码/测试/自动化/审阅由Grok4.7 xhigh执行；Codex写规范/卡片/台账、核验证据、精确Git提交。

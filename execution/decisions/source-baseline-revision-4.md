# 来源基线revision 4：Ontology交付与内置Pi的关系

日期：2026-10-09。来源固定提交：`12cfdc48260636032429449d4e66253d54c1e3cd`。上一来源revision3为`b504fd8f4369750f6a329075c77dcc4bf3720702`，上一sources-baseline.json SHA-256为`9875ee32eeacb3655a92b7803e5e9eb88dea89ca3930c30a52118d30024972b0`。

## 用户指令与本次范围

项目负责人明确：Open Ontology交付的是产出Ontology的系统，最终可通过MCP、CLI或其他形式外接harness；内置Pi提供快速体验及内部验证，不是最终产品形态。随后要求重新对齐docs。本记录将这两条直接指令作为本次文字及基线更新授权，不宣称已有独立reviewer签字或新产品验收。

## 修改的来源

docs/00、01、06、08、09、13、14和README：明确标准YAML、overlay、Git版本、角色Pack、Gateway及外接接口的核心交付；说明内置Pi的演示与验证职责；外部消费者不依赖内置Pi会话；外接验证使用非内置Pi真实调用方并记录实际协议/客户端/版本。其余11份基线文档未改。README去掉“尚无实现代码”的过时表述，仍保持设计状态，未宣称全部实现。

## 保留的要求

内置Pi、沙盒、网页/IM、状态释放、安全不变量、源零写、数据权限、模型/客户端实际支持范围全部保留。CLI/HTTP/SDK按SP09、MCP/OAuth按SP13原卡推进，不声称所有harness或transport已实现。未新增REQ/AT、未改87父项/16Phase、未改DBX分母或既有开发/集成/发行计数。

## 工程对齐

重新计算19份来源hash、bytes和lines，保留revision3及更早来源记录。Spec产品定义与Plan执行附录同步；Goal、Handoff、控制台、任务台账和生成TODO同步交付边界。旧的PG六问证明仍只属于未审核草案内部验证，不升级成外接交付证据。历史固定清单按原commit复核，不改旧结果。

文档检查只证明来源和追踪一致，不证明外接接口已经完成。源契约引用方领取新任务前读取本次变化，不自动把历史执行时使用的revision3改写成revision4。

验证：Spec/Plan变异自测与TODO一致性退出0；19份来源逐一匹配固定docs提交，8份改动文档的原代码示例保持不变，183条本地引用检查通过。原始结果在`execution/validation/product-boundary-20261009/`，无产品代码测试或外接功能通过声明。

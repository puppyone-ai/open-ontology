#!/usr/bin/env python3
"""Render current parent-item TODO; no inferred product acceptance."""
import argparse
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LEDGER = ROOT / 'execution/task-ledger.json'
TODO = ROOT / 'execution/TODO.md'


def render():
    data = json.loads(LEDGER.read_text())
    plan = (ROOT / data['baseline']['plan']).read_text()
    declared = re.findall(r'^#### ((?:SP\d{2}|DBX)-W\d{2}) —', plan, re.M)
    ids = [item['id'] for item in data['items']]
    if len(ids) != len(set(ids)) or set(ids) != set(declared):
        raise ValueError('Ledger must uniquely cover all Plan parent cards')
    if sum(item['track'] == 'SP' for item in data['items']) != 87:
        raise ValueError('Expected 87 SP parent cards')
    if sum(item['track'] == 'DBX' for item in data['items']) != 14:
        raise ValueError('Expected 14 DBX parent cards')
    if len(data['phases']) != 16 or len(data['prototypes']) != 8:
        raise ValueError('Expected 16 phases and 8 DBP prototypes')
    contracts = {}
    for match in re.finditer(r'^#### ((?:SP\d{2}|DBX)-W\d{2}) — ([^\n]+)\n(.*?)(?=^#{2,4} |\Z)', plan, re.M | re.S):
        task, title, body = match.groups()
        fields = {}
        for label in ('前置/执行', '执行', '交付/PR', '验收/停止'):
            field = re.search(r'^- ' + re.escape(label) + r'：(.*)$', body, re.M)
            if field:
                fields[label] = field[1]
        contracts[task] = (title, fields.get('前置/执行', fields.get('执行', '')), fields.get('交付/PR', ''), fields.get('验收/停止', ''))
    for item in data['items']:
        recorded = tuple(item[key] for key in ('title', 'execution', 'deliverables', 'exit_conditions'))
        if recorded != contracts[item['id']]:
            raise ValueError(f"Plan contract drift; reconcile ledger before dispatch: {item['id']}")
        if not item['execution'] or not item['deliverables'] or not item['exit_conditions']:
            raise ValueError(f"Missing Plan contract: {item['id']}")
        if item['status'] == 'accepted' and not all(
            (item['source_commit'], item['evidence'], item['independent_review'])
        ):
            raise ValueError(f"Cannot mark accepted without fixed evidence: {item['id']}")
    loops = data.get('delivery_loops', [])
    if [item['id'] for item in loops] != [f'DL{i:02}' for i in range(1, 6)]:
        raise ValueError('Expected five ordered product deliveries DL01–DL05')
    loop_doc = (ROOT / data['rules']['product_loop_registry']).read_text()
    checks = {
        task: mark for mark, task in re.findall(r'^- \[([ x])\] (DL\d{2})[：:]', loop_doc, re.M)
    }
    if set(checks) != {item['id'] for item in loops}:
        raise ValueError('Product delivery top checklist drift')
    for index, loop in enumerate(loops):
        accepted = loop['status'] == 'accepted'
        if (checks[loop['id']] == 'x') != accepted:
            raise ValueError(f"Product delivery status/checklist drift: {loop['id']}")
        if accepted:
            if any(prior['status'] != 'accepted' for prior in loops[:index]):
                raise ValueError('Cannot accept later delivery before previous deliveries')
            if not all(loop.get(key) for key in ('source_commit', 'artifact_digest', 'evidence', 'independent_review', 'acceptance_record')):
                raise ValueError(f"Missing fixed delivery receipt: {loop['id']}")
            if loop['id'] == 'DL01':
                targets = loop.get('source_acceptance', [])
                required_products = set(loop.get('required_source_product_ids', []))
                actual_products = {target.get('source_product_id') for target in targets if target.get('status') == 'accepted'}
                if len(actual_products) < 10 or not {'mysql', 'oracle'} <= actual_products or not required_products <= actual_products:
                    raise ValueError('DL01 requires at least ten independently accepted products including MySQL and Oracle')
                if any(not all(target.get(key) for key in ('version', 'deployment', 'driver', 'evidence', 'independent_review')) for target in targets if target.get('status') == 'accepted'):
                    raise ValueError('Missing fixed target/driver/evidence in DL01 source acceptance')
            due_items = [item for item in data['items'] if item.get('required_for_current_goal') and (item.get('final_delivery_loop') or 'DL99') <= loop['id']]
            if any(item['status'] != 'accepted' for item in due_items):
                raise ValueError(f"Required Plan work remains unaccepted at {loop['id']}")
            due_phases = [phase for phase in data['phases'] if phase.get('required_for_current_goal') and (phase.get('final_delivery_loop') or 'DL99') <= loop['id']]
            if any(phase['formal_status'] != 'accepted' or not phase.get('acceptance_record') for phase in due_phases):
                raise ValueError(f"Required Phase signoff remains pending at {loop['id']}")
    required_sp = [item for item in data['items'] if item['track'] == 'SP' and item.get('required_for_current_goal')]
    deferred_sp = [item for item in data['items'] if item['track'] == 'SP' and not item.get('required_for_current_goal')]
    if len(required_sp) != 80 or len(deferred_sp) != 7 or any(item['phase'] != 'SP15' for item in deferred_sp):
        raise ValueError('Required Plan scope drift: only seven SP15 expansion cards deferred')
    if any(not item.get('required_for_current_goal') for item in data['items'] if item['track'] == 'DBX'):
        raise ValueError('DBX work cannot be silently moved out of the current goal')
    if {phase['id'] for phase in data['phases'] if phase.get('required_for_current_goal')} != {f'SP{i:02}' for i in range(15)}:
        raise ValueError('All fifteen mandatory Phases must remain required')
    for phase in data['phases']:
        if phase.get('required_for_current_goal') and phase.get('final_delivery_loop') not in {loop['id'] for loop in loops}:
            raise ValueError(f"Missing final product-delivery mapping: {phase['id']}")
    for item in data['items']:
        if item.get('required_for_current_goal') and item.get('final_delivery_loop') not in {loop['id'] for loop in loops}:
            raise ValueError(f"Missing final product-delivery mapping: {item['id']}")
    current = data['current_delivery_loop']
    if current not in {loop['id'] for loop in loops}:
        raise ValueError('Unknown current product delivery')
    if any(loop['status'] != 'accepted' for loop in loops if loop['id'] < current):
        raise ValueError('Cannot advance current delivery before prior signoff')
    lines = [
        '# 完整产品 TODO 与派发入口', '',
        f"更新：{data['updated_on']}。[索引](INDEX.md) / [状态更新协议](UPDATE-PROTOCOL.md)。由 [task-ledger.json](task-ledger.json) 生成；修改台账后运行 `python3 execution/scripts/render_todo.py`，CI/协调核对使用 `--check`。",
        '',
        '本清单勾选表示**整个父项正式退出**，不是开发组件是否存在。未勾项可能已交付多个组件；状态“待签收对账”不能解释为尚未开发或开发完成率为零。开发子项与证据见[验收计数](../specs/open-ontology-acceptance-counting.md)，历史交付见[总台账](../specs/open-ontology-development-ledger.md)。87个SP、14个DBX与8个DBP分别统计，不合并分母。', '',
        '完整要求、输入/交付/退出条件从[主Plan](../specs/open-ontology-v0.1-plan.md)逐卡引入机器台账；实际派发前还须冻结owner、接口SHA、路径、测试命令、资源和完整能力卡。未填owner/handle表示未派发，不从OS进程数推断。', '',
        '产品交付是Ontology生产/审核/发布系统、角色Pack和外接接口。内置Pi用于体验和内部验证；外接使用不依赖Pi会话，具体支持协议/客户端/版本以实际验收为准。', '',
        '交付顺序见[PRODUCT-LOOPS](PRODUCT-LOOPS.md)：当前DL01，整版按序签收，开发按固定依赖/独占文件/资源余量并行；与SP共用实现和证据。五版结束必须同时完成80必交SP/14DBX/15阶段，SP15七项明确后续扩展单列。', '',
        '## 当前已验收里程碑', '',
    ]
    for m in data['milestones']:
        lines.append(f"- [x] {m['id']}：{m['title']}；[固定复审](../{m['review']})。**{m['scope_limit']}**。")
    if data.get('active_batch'):
        lines += ['', '## 当前冻结批次', '', f"- [{data['active_batch']}]({data['active_batch'].removeprefix('execution/')})；实际运行句柄见CONTROL/JSON，批次状态只按退出/证据更新。"]
        for task in data.get('dispatch_tasks', []):
            lines.append(f"- {task['task_id']} / {task['name']}：{task['kind']}；{task['status']}；Plan {task['parent']}；固定依赖 {task['base_commit'][:12]}。")
    lines += ['', '## 最近下一步与当前派发', '',
        '- [ ] 对账SP既有固定证据：保留已验组件，标出各父项的剩余退出条件；不得将本次清单重置为“全未实现”。',
        '- [ ] DBX-W01/W02/W03/W04/W05/W14：先冻结共享schema/ABI/适用目标ADR及开发子验收分母，再释放消费者。',
        '- [ ] 优先真实MySQL/PG目标，具备依赖后并行SQL Server/Oracle批量→本地PG→实际Builder/Pack/Pi问答；协议兼容目标各自证明。',
        '- [ ] 正式S9审核/S10分类与YAML→overlay→Git版本→发布→Pack贯通；草案工具和六问不能替代。',
        '- [ ] SP09/SP13外接验收：非内置Pi的真实调用方取得同一发布Knowledge/Pack，经声明支持的CLI/HTTP/SDK/MCP核对数据、权限和审计；不要求内置Pi会话、不冒称所有harness已验证。',
        '- [ ] 当前DL01跨SP领取完整十源预览子能力；后续独立卡按固定依赖、独占文件及资源余量并行，不挤占当前关键路径；各版按序签收，既有模块先对账；前端仍仅master-task三个固定会话。',
        '- [ ] 每次转态同步CONTROL/运行快照、任务台账、开发计数、主Plan和CHANGELOG；Grok已获准新派或续跑，GPT例外范围及资源限制按最新用户指令执行。', '',
        '## P0 数据库扩展父项（14项）', '',
        'DBX状态仅表示计划/正式签收；支持矩阵按目标×版本×变体×驱动×能力独立定级，当前没有verified目标。', '']
    def row(item):
        mark = 'x' if item['status'] == 'accepted' else ' '
        status = {'planned': '计划，待冻结/派发', 'needs_acceptance_reconciliation': '待固定签收对账', 'accepted': '正式签收'}.get(item['status'], item['status'])
        delivery = item.get('final_delivery_loop') or 'SP15后续扩展'
        return f"- [{mark}] **{item['id']} — {item['title']}**：{status}；owner：{item['owner'] or '待派'}；完整签收：{delivery}。"
    for item in data['items']:
        if item['track'] == 'DBX':
            lines.append(row(item))
    lines += ['', '## 完整SP父项（87项，16个Phase）', '']
    for phase in data['phases']:
        lines += [f"### {phase['id']} — {phase['title']}", '']
        children = [item for item in data['items'] if item['phase'] == phase['id']]
        if not children:
            lines.append('风险原型Phase：按主Plan的P1–P16逐原型证据和Phase退出条件对账，无额外SP父项；不新增分母。')
        for item in children:
            lines.append(row(item))
        lines.append('')
    lines += ['## DBP风险原型（8项，单列）', '']
    for p in data['prototypes']:
        lines.append(f"- [ ] **{p['id']}**：{p['question']}；既有结果待固定证据对账，不计开发父项。")
    lines += ['', '## 外部门槛（不进入开发分母）', '']
    for gate in data['external_gates']:
        lines.append(f"- [ ] **{gate['id']}**：{gate['description']}；待外部核验。仅阻塞适用验收，不阻塞其他独立开发。")
    lines += ['', '## 更新与签收规则', '',
        '任务台账是当前派发/父项签收结构化入口；开发历史仍以原总台账、冻结计数及固定报告为准。转态填写真实owner/session/handle、资源和路径；作者交付、独立review、集成、发行分别记录，父项全部退出后才能status=accepted并勾选。本生成器拒绝无固定源码/证据/独立复审的accepted项，但不能替代人工审阅全部退出条件。', '',
        data.get('current_execution_note', '本清单不自动推断验收；平台Goal和不明进程状态以CONTROL实际核实为准。'), '']
    return '\n'.join(lines)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    expected = render()
    if args.check:
        if not TODO.exists() or TODO.read_text() != expected:
            raise SystemExit('FAIL: TODO drift; render from task-ledger.json')
        print('PASS: 87 SP + 14 DBX cards, 16 phases, 8 DBP; five deliveries cover 80 required SP/14 DBX/15 phases; TODO matches ledger')
    else:
        TODO.write_text(expected)
        print('Rendered execution/TODO.md')


if __name__ == '__main__':
    main()

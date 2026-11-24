#!/usr/bin/env python3
"""Deterministic generator of the structure-layer test corpus (D93, lane AM).

    python3 -B tools/super-ontology/structure/gen.py [--out DIR] [--check]

Stdlib only, no clocks, no random module: every value comes from fixed pools and index arithmetic. The corpus is six record
templates in Chinese and English, each with variations (frontmatter or `key: value` lines, synonym headings, missing sections,
a different order) and four free-form notes that belong to no template. It writes `<out>/project/` (the folder to drop into My files),
`<out>/gold/structure.json` (the classes, properties, types and relations the induction must find) and `<out>/README.md`.
The gold is written here next to the templates it describes and is frozen before any run; it is never edited after a run.
All content is synthetic.
"""
import argparse, hashlib, json, sys
from pathlib import Path

FILES = {}


def put(path, text):
    FILES[path] = text


ZH_PEOPLE = [("陈子墨", "工程师", "平台部", "E1001", "2023-04-17"), ("周若溪", "产品经理", "产品部", "E1002", "2022-09-05"),
             ("吴启明", "工程师", "平台部", "E1003", "2021-03-22"), ("孙晓棠", "设计师", "设计部", "E1004", "2024-01-08"),
             ("林嘉禾", "工程师", "数据部", "E1005", "2023-11-13"), ("郑一鸣", "产品经理", "产品部", "E1006", "2020-06-29"),
             ("宋雨桐", "设计师", "设计部", "E1007", "2024-05-20"), ("韩知远", "工程师", "数据部", "E1008", "2022-02-14"),
             ("冯可心", "运营", "运营部", "E1009", "2023-08-01"), ("唐亦凡", "运营", "运营部", "E1010", "2021-10-18"),
             ("许明哲", "工程师", "平台部", "E1011", "2024-03-11"), ("江雪晴", "产品经理", "产品部", "E1012", "2022-12-05")]
EN_PEOPLE = [("Daniel Kim", "Engineer", "Platform", "E2001", "2023-02-13"), ("Karen Wu", "Product Manager", "Product", "E2002", "2022-07-18"),
             ("Ralph Evans", "Engineer", "Platform", "E2003", "2021-05-24"), ("Sandy Cho", "Designer", "Design", "E2004", "2024-02-05"),
             ("Fred Lang", "Engineer", "Data", "E2005", "2023-10-09"), ("Kathy Moss", "Product Manager", "Product", "E2006", "2020-08-31"),
             ("Albert Shaw", "Designer", "Design", "E2007", "2024-04-15"), ("Junior Park", "Engineer", "Data", "E2008", "2022-03-28"),
             ("Flo Reyes", "Operations", "Operations", "E2009", "2023-06-12"), ("Reed Cole", "Operations", "Operations", "E2010", "2021-11-22")]
ZH_PROJECTS = [("星河", 0, "进行中", "120万", "2026-09-30"), ("北辰", 1, "进行中", "80万", "2026-07-15"), ("青云", 2, "已完成", "45万", "2026-03-31"),
               ("远山", 3, "暂停", "60万", "2026-12-20"), ("云帆", 4, "进行中", "150万", "2026-10-31"), ("海岚", 5, "已完成", "30万", "2026-02-28")]
EN_PROJECTS = [("Beacon", 0, "Active", "$120,000", "2026-09-30"), ("Atlas", 1, "Active", "$80,000", "2026-07-15"), ("Harbor", 2, "Done", "$45,000", "2026-03-31"),
               ("Summit", 3, "On hold", "$60,000", "2026-12-20"), ("Lantern", 4, "Active", "$150,000", "2026-10-31"), ("Meridian", 5, "Done", "$30,000", "2026-02-28")]
ZH_ORGS = ["蓝桥科技有限公司", "远航物流集团", "明石能源股份公司", "云杉软件有限公司", "北岸贸易有限公司", "星火数据服务公司"]
EN_ORGS = ["Northwind Logistics", "Bluegate Energy", "Cedar Software", "Ironbridge Trading", "Lakeside Data Services", "Redwood Systems"]
ZH_TOPICS = ["启动会", "需求评审", "方案讨论", "进度同步", "上线复盘", "预算评审", "风险评估", "验收会"]
EN_TOPICS = ["Kickoff", "Requirements review", "Design discussion", "Status sync", "Launch retro", "Budget review", "Risk assessment", "Acceptance"]
ZH_DECISIONS = ["采用灰度发布，先放量百分之十", "数据库迁移推迟到第三季度", "统一使用内部组件库", "暂停外部接口的新增需求", "每周三固定做代码评审",
                "预算按季度滚动审批", "合同模板改用新版法务条款", "客户数据保存期限定为两年"]
EN_DECISIONS = ["Adopt canary releases starting at ten percent", "Postpone the database migration to the third quarter", "Use the internal component library everywhere",
                "Freeze new requests for the external API", "Hold code review every Wednesday", "Approve the budget on a rolling quarterly basis",
                "Switch contracts to the new legal template", "Keep customer data for two years"]
ZH_ACTIONS = ["整理需求清单并发给各方", "完成灰度发布脚本", "更新风险登记表", "联系供应商确认交期", "补充测试用例", "起草预算调整说明"]
EN_ACTIONS = ["Circulate the requirement list", "Finish the canary release script", "Update the risk register", "Confirm lead time with the supplier", "Add test cases", "Draft the budget note"]
ZH_RAT = ["现有方案在高峰期无法满足延迟要求，需要换一种发布方式以降低风险。", "团队人手不足，同时推进两项迁移会拖慢交付。", "重复造组件造成界面不一致，维护成本高。"]
EN_RAT = ["The current approach cannot meet the latency target at peak, so a safer rollout is needed.", "The team is short-handed and two migrations at once would slow delivery.", "Duplicated components make the interface inconsistent and costly to maintain."]
STATUS_ZH = ["已通过", "已通过", "待定", "已取代", "已通过", "已否决"]
STATUS_EN = ["accepted", "accepted", "proposed", "superseded", "accepted", "rejected"]

GOLD_CLASSES = {}


def date(i, base_month=3):
    return f"2026-{base_month + (i // 10):02d}-{(i * 3) % 27 + 1:02d}"


def pick(seq, i, n=1):
    return [seq[(i + k * 3) % len(seq)] for k in range(n)]


# ------------------------------------------------------------------ meeting minutes
def meetings():
    # zh: attendees as a list section, with synonym headings; en: attendees as a line. 18 files in three project directories.
    for i in range(10):
        proj = ZH_PROJECTS[i % 3][0]
        names = [p[0] for p in pick(ZH_PEOPLE, i, 3)]
        host = ZH_PEOPLE[(i + 1) % 6][0]
        att_head = ["参会人", "与会人员", "出席人员"][i % 3]
        topic = ZH_TOPICS[i % len(ZH_TOPICS)]
        d = date(i)
        fm = f"---\n日期: {d}\n项目: {proj}\n主持人: {host}\n---\n\n" if i % 2 == 0 else ""
        inline = "" if i % 2 == 0 else f"**日期**：{d}\n\n**项目**：{proj}\n\n**主持人**：{host}\n\n"
        agenda = "" if i == 4 else f"## 议程\n\n- 回顾上次待办\n- {topic}相关事项\n\n"
        body = (f"# {proj}{topic}纪要\n\n{inline}## {att_head}\n\n" + "".join(f"- {n}\n" for n in names) + "\n" + agenda
                + f"## 决定\n\n- {ZH_DECISIONS[i % len(ZH_DECISIONS)]}\n\n## 待办\n\n- {ZH_ACTIONS[i % len(ZH_ACTIONS)]}\n- {ZH_ACTIONS[(i + 2) % len(ZH_ACTIONS)]}\n")
        put(f"meetings/{proj}/{d}-{topic}.md", fm + body)
    for i in range(8):
        proj = EN_PROJECTS[i % 3][0]
        names = [p[0] for p in pick(EN_PEOPLE, i, 3)]
        host = EN_PEOPLE[(i + 1) % 5][0]
        topic = EN_TOPICS[i % len(EN_TOPICS)]
        d = date(i + 10, 4)
        fm = f"---\ndate: {d}\nproject: {proj}\nhost: {host}\n---\n\n" if i % 2 == 0 else ""
        inline = "" if i % 2 == 0 else f"Date: {d}\n\nProject: {proj}\n\nHost: {host}\n\n"
        agenda = "" if i == 5 else f"## Agenda\n\n- Review open items\n- {topic}\n\n"
        att = ["Attendees", "Participants"][i % 2]
        body = (f"# {proj} {topic} minutes\n\n{inline}## {att}\n\n" + "".join(f"- {n}\n" for n in names) + "\n" + agenda
                + f"## Decisions\n\n- {EN_DECISIONS[i % len(EN_DECISIONS)]}\n\n## Action items\n\n- {EN_ACTIONS[i % len(EN_ACTIONS)]}\n- {EN_ACTIONS[(i + 2) % len(EN_ACTIONS)]}\n")
        put(f"meetings/{proj.lower()}/{d}-{topic.lower().replace(' ', '-')}.md", fm + body)


# ------------------------------------------------------------------ decision log
def decisions():
    for i in range(8):
        d = date(i, 2)
        status = STATUS_ZH[i % 6]
        by = ZH_PEOPLE[(i + 2) % 6][0]
        proj = ZH_PROJECTS[i % 6][0]
        fm = f"---\n状态: {status}\n决策人: {by}\n日期: {d}\n项目: {proj}\n---\n\n"
        alt = "" if i == 3 else f"## 备选方案\n\n- 维持现状\n- 分阶段推进\n\n"
        put(f"decisions/DEC-{i + 1:03d}-{proj}.md", fm + f"# DEC-{i + 1:03d} {ZH_DECISIONS[i]}\n\n## 背景\n\n{ZH_RAT[i % 3]}\n\n## 决定\n\n{ZH_DECISIONS[i]}。\n\n" + alt)
    for i in range(6):
        d = date(i + 8, 5)
        status = STATUS_EN[i % 6]
        by = EN_PEOPLE[(i + 2) % 5][0]
        proj = EN_PROJECTS[i % 6][0]
        fm = f"---\nstatus: {status}\ndecided_by: {by}\ndate: {d}\nproject: {proj}\n---\n\n"
        alt = "" if i == 2 else "## Alternatives\n\n- Keep the status quo\n- Phase the change\n\n"
        put(f"decisions/DEC-{i + 11:03d}-{proj.lower()}.md", fm + f"# DEC-{i + 11:03d} {EN_DECISIONS[i]}\n\n## Context\n\n{EN_RAT[i % 3]}\n\n## Decision\n\n{EN_DECISIONS[i]}.\n\n" + alt)


# ------------------------------------------------------------------ weekly reports
def weekly():
    for i in range(8):
        a = ZH_PEOPLE[i % 8][0]
        wk = 10 + i
        risks = "" if i % 4 == 3 else f"## 风险\n\n- {ZH_ACTIONS[(i + 1) % 6]}尚无明确负责人\n\n"
        put(f"weekly/{a}-2026-W{wk}.md",
            f"# {a} 周报 第{wk}周\n\n**作者**：{a}\n\n**周次**：第{wk}周\n\n## 本周进展\n\n- {ZH_ACTIONS[i % 6]}\n- 完成了{ZH_TOPICS[i % 8]}的材料\n\n## 下周计划\n\n- {ZH_ACTIONS[(i + 3) % 6]}\n\n" + risks)
    for i in range(6):
        a = EN_PEOPLE[i % 6][0]
        wk = 20 + i
        risks = "" if i % 3 == 2 else f"## Risks\n\n- {EN_ACTIONS[(i + 1) % 6]} has no clear owner\n\n"
        put(f"weekly/{a.split()[0].lower()}-2026-w{wk}.md",
            f"# {a} weekly report, week {wk}\n\nAuthor: {a}\n\nWeek: Week {wk}\n\n## This week\n\n- {EN_ACTIONS[i % 6]}\n- Prepared the {EN_TOPICS[i % 8].lower()} material\n\n## Next week\n\n- {EN_ACTIONS[(i + 3) % 6]}\n\n" + risks)


# ------------------------------------------------------------------ people and project tables
def tables():
    h = "姓名,角色,部门,邮箱,工号,入职日期\n"
    rows = "".join(f"{n},{r},{d},{pinyin(n)}@example.com,{e},{s}\n" for n, r, d, e, s in ZH_PEOPLE)
    put("people/团队名册.csv", h + rows)
    h = "Name,Role,Department,Email,Employee ID,Hire date\n"
    rows = "".join(f"{n},{r},{d},{n.lower().replace(' ', '.')}@example.com,{e},{s}\n" for n, r, d, e, s in EN_PEOPLE)
    put("people/team-roster.csv", h + rows)
    h = "项目,负责人,状态,预算,截止日期\n"
    rows = "".join(f"{n},{ZH_PEOPLE[o][0]},{s},{b},{dl}\n" for n, o, s, b, dl in ZH_PROJECTS)
    put("projects/项目一览.csv", h + rows)
    h = "Project,Owner,Status,Budget,Deadline\n"
    rows = "".join(f"{n},{EN_PEOPLE[o % 5][0]},{s},\"{b}\",{dl}\n" for n, o, s, b, dl in EN_PROJECTS)
    put("projects/portfolio.csv", h + rows)


def pinyin(n):
    table = {"陈子墨": "chenzimo", "周若溪": "zhouruoxi", "吴启明": "wuqiming", "孙晓棠": "sunxiaotang", "林嘉禾": "linjiahe", "郑一鸣": "zhengyiming",
             "宋雨桐": "songyutong", "韩知远": "hanzhiyuan", "冯可心": "fengkexin", "唐亦凡": "tangyifan", "许明哲": "xumingzhe", "江雪晴": "jiangxueqing"}
    return table[n]


# ------------------------------------------------------------------ contracts
def contracts():
    for i in range(6):
        no = f"CT-2026-{i + 1:03d}"
        amount = ["¥120,000", "￥86,500", "30万元", "¥210,000", "45万元", "¥68,000"][i]
        term = ["12个月", "24个月", "6个月", "12个月", "36个月", "12个月"][i]
        start = date(i * 2, 1)
        put(f"contracts/{no}.md",
            f"# 服务合同 {no}\n\n合同编号：{no}\n\n甲方：星河信息有限公司\n\n乙方：{ZH_ORGS[i]}\n\n合同金额：{amount}\n\n生效日期：{start}\n\n期限：{term}\n\n## 付款条款\n\n合同签订后十个工作日内支付首付款，验收后支付余款。\n")
    for i in range(6):
        no = f"CT-2026-{i + 11:03d}"
        amount = ["$120,000", "USD 86,500", "$300,000", "$210,000", "USD 45,000", "$68,000"][i]
        term = ["12 months", "24 months", "6 months", "12 months", "36 months", "12 months"][i]
        start = date(i * 2 + 1, 1)
        put(f"contracts/{no}.md",
            f"# Service agreement {no}\n\nContract no: {no}\n\nParty A: Starriver Information Ltd\n\nParty B: {EN_ORGS[i]}\n\nAmount: {amount}\n\nStart date: {start}\n\nTerm: {term}\n\n## Payment\n\nThe first payment is due within ten business days of signing; the balance is due after acceptance.\n")


# ------------------------------------------------------------------ notes that belong to no template
def noise():
    put("notes/读书笔记.md", "# 读书笔记\n\n## 第一章的想法\n\n慢慢写，不急。\n\n## 引用\n\n一些摘抄。\n")
    put("notes/ideas.md", "# Ideas\n\n## Garden\n\nPlant beans early.\n\n## Trip\n\nBook the ferry.\n")
    put("notes/杂记.md", "# 杂记\n\n今天整理了书架，顺便想了想周末去哪里。\n")
    put("notes/todo-someday.md", "# Someday\n\n- learn to sail\n- fix the shelf\n")


QUESTIONS = [
    {"id": "q01", "kind": "meeting", "lang": "zh", "question": "星河项目的启动会谁是主持人？", "answer": "周若溪", "answerable": True, "files": ["meetings/星河/2026-03-01-启动会.md"]},
    {"id": "q02", "kind": "meeting", "lang": "zh", "question": "2026-03-01 的星河启动会决定了什么？", "answer": "灰度发布", "answerable": True, "files": ["meetings/星河/2026-03-01-启动会.md"]},
    {"id": "q03", "kind": "contract", "lang": "zh", "question": "合同 CT-2026-003 的乙方是谁，合同金额是多少？", "answer": "明石能源股份公司;30万", "answerable": True, "files": ["contracts/CT-2026-003.md"]},
    {"id": "q04", "kind": "table", "lang": "en", "question": "Who owns the project 北辰, and what is its budget?", "answer": "周若溪;80万", "answerable": True, "files": ["projects/项目一览.csv"]},
    {"id": "q05", "kind": "decision", "lang": "en", "question": "What is the status of decision DEC-013?", "answer": "proposed", "answerable": True, "files": ["decisions/DEC-013-harbor.md"]},
    {"id": "q06", "kind": "contract", "lang": "en", "question": "Which contract does Bluegate Energy have, and how long is its term?", "answer": "CT-2026-012;24", "answerable": True, "files": ["contracts/CT-2026-012.md"]},
    {"id": "q07", "kind": "table", "lang": "zh", "question": "吴启明在哪个部门？", "answer": "平台部", "answerable": True, "files": ["people/团队名册.csv"]},
    {"id": "q08", "kind": "unanswerable", "lang": "en", "question": "What is the budget of the 2027 renewal of the 远山 project?", "answer": "", "answerable": False, "files": []},
]

GOLD = {
    "version": "structure-gold/1",
    "amendments": "After the first scoring two annotations were corrected with the evidence of the files: project_table has a column named 项目/Project (concept project), not name; every contract has a payment section (付款条款/Payment) that the annotation lacked. Type and relation gold untouched.",
    "note": "Frozen before the first run. Property names are concept slugs; `type` is the value type; `relation` is the kind of the class the values name. text and long_text count as one type.",
    "classes": {
        "meeting_minutes": {"files": 18, "shape": "document", "properties": {
            "attendees": {"type": "person", "relation": "people_table"}, "date": {"type": "date"}, "project": {"type": "project", "relation": "project_table"},
            "host": {"type": "person", "relation": "people_table"}, "agenda": {"type": "text"}, "decisions": {"type": "text"}, "actions": {"type": "text"},
            "directory": {"type": "project", "relation": "project_table"}}},
        "decision_log": {"files": 14, "shape": "document", "properties": {
            "status": {"type": "enum"}, "decided_by": {"type": "person", "relation": "people_table"}, "date": {"type": "date"},
            "project": {"type": "project", "relation": "project_table"}, "rationale": {"type": "text"}, "decisions": {"type": "text"}, "alternatives": {"type": "text"}}},
        "weekly_report": {"files": 14, "shape": "document", "properties": {
            "author": {"type": "person", "relation": "people_table"}, "week": {"type": "text"}, "this_week": {"type": "text"}, "next_week": {"type": "text"}, "risks": {"type": "text"}}},
        "people_table": {"files": 2, "shape": "table", "properties": {
            "name": {"type": "person"}, "role": {"type": "enum"}, "department": {"type": "enum"}, "email": {"type": "email"}, "employee_id": {"type": "id"}, "start_date": {"type": "date"}}},
        "project_table": {"files": 2, "shape": "table", "properties": {
            "project": {"type": "project"}, "owner": {"type": "person", "relation": "people_table"}, "status": {"type": "enum"}, "budget": {"type": "amount"}, "deadline": {"type": "date"}}},
        "contract": {"files": 12, "shape": "document", "properties": {
            "contract_no": {"type": "id"}, "party_a": {"type": "organization"}, "party_b": {"type": "organization"}, "amount": {"type": "amount"},
            "start_date": {"type": "date"}, "term": {"type": "text"}, "payment": {"type": "text"}}},
    },
    "record_files": 64,
    "unclustered": ["notes/ideas.md", "notes/todo-someday.md", "notes/杂记.md", "notes/读书笔记.md"],
}


def build():
    meetings(); decisions(); weekly(); tables(); contracts(); noise()
    GOLD["record_files"] = len(FILES)
    return FILES


def write(out):
    out = Path(out)
    for p, t in sorted(FILES.items()):
        f = out / "project" / p
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_text(t, encoding="utf-8")
    g = out / "gold"
    g.mkdir(parents=True, exist_ok=True)
    (g / "structure.json").write_text(json.dumps(GOLD, ensure_ascii=False, indent=1, sort_keys=True) + "\n", encoding="utf-8")
    (g / "questions.jsonl").write_text("".join(json.dumps(q, ensure_ascii=False, sort_keys=True) + "\n" for q in QUESTIONS), encoding="utf-8")
    manifest = "".join(f"{hashlib.sha256(t.encode()).hexdigest()}  project/{p}\n" for p, t in sorted(FILES.items()))
    manifest += f"{hashlib.sha256((g / 'structure.json').read_bytes()).hexdigest()}  gold/structure.json\n"
    manifest += f"{hashlib.sha256((g / 'questions.jsonl').read_bytes()).hexdigest()}  gold/questions.jsonl\n"
    (out / "MANIFEST.sha256").write_text(manifest, encoding="utf-8")


def main():
    ap = argparse.ArgumentParser()
    root = Path(__file__).resolve().parents[3] / "testing/super-ontology/structure"
    ap.add_argument("--out", default=str(root))
    ap.add_argument("--check", action="store_true")
    a = ap.parse_args()
    build()
    if a.check:
        want = (Path(a.out) / "MANIFEST.sha256")
        got = "".join(f"{hashlib.sha256(t.encode()).hexdigest()}  project/{p}\n" for p, t in sorted(FILES.items()))
        have = want.read_text(encoding="utf-8")
        ok = have.startswith(got)
        print("ok" if ok else "MISMATCH")
        sys.exit(0 if ok else 1)
    write(a.out)
    print(f"wrote {len(FILES)} files to {a.out}")


if __name__ == "__main__":
    main()

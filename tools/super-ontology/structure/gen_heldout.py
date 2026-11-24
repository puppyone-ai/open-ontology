#!/usr/bin/env python3
"""Held-out structure corpus (D93, lane AM): other domains, other field names and orders, zh/en mixed inside files, new pools.

    python3 -B tools/super-ontology/structure/gen_heldout.py [--out DIR] [--check]

Written and frozen before the induction was run on it; gold is never edited after a run. Classes: incident reports, customer call notes,
expense tables, release notes, a staff table; plus noise notes. Gold properties are named by the labels the files use (any listed label).
"""
import argparse, hashlib, json, sys
from pathlib import Path

F = {}
STAFF = [("Mara Lindqvist", "SRE", "mara@example.org"), ("陆承宇", "后端工程师", "luchengyu@example.org"), ("Tobias Wren", "Support Lead", "tobias@example.org"),
         ("闵思远", "产品经理", "minsiyuan@example.org"), ("Ines Okafor", "QA", "ines@example.org"), ("康婉宁", "运维", "kangwanning@example.org"),
         ("Pavel Duric", "Engineer", "pavel@example.org"), ("洪嘉树", "设计师", "hongjiashu@example.org")]
CUST = ["Harlow Freight", "云岭制造", "Quillon Labs", "瀚海传媒", "Ostrava Mills", "禾木教育"]
SERV = ["billing-api", "search", "auth", "notifier"]


def d(i, m):
    return f"2026-{m + i // 12:02d}-{(i * 5) % 27 + 1:02d}"


def incidents():
    for i in range(16):
        zh = i % 2 == 0
        rep = STAFF[(i * 3) % 8][0]
        sev = ["sev1", "sev2", "sev2", "sev3", "sev3", "sev3"][i % 6]
        svc = SERV[i % 4]
        if zh:
            F[f"incidents/INC-{i + 40}.md"] = (f"---\nreported: {d(i, 2)}\nseverity: {sev}\nservice: {svc}\nreporter: {rep}\n---\n\n# INC-{i + 40} {svc} 故障\n\n"
                                               f"## 影响\n\n{svc} 在晚高峰出现超时，约 {5 + i}% 的请求失败。\n\n## 根因\n\n连接池配置在上次发布后被重置。\n\n## 处置\n\n回滚配置并增加告警。\n")
        else:
            F[f"incidents/INC-{i + 40}.md"] = (f"# INC-{i + 40} {svc} outage\n\n## Impact\n\nTimeouts at peak hours, about {5 + i}% of requests failed.\n\n"
                                               f"Reporter: {rep}\n\nSeverity: {sev}\n\nService: {svc}\n\nReported: {d(i, 2)}\n\n## Root cause\n\nPool settings were reset by the last release.\n\n## Fix\n\nRolled back and added an alert.\n")


def calls():
    outc = ["won", "lost", "follow-up", "won", "follow-up"]
    for i in range(14):
        c = CUST[i % 6]
        contact = STAFF[(i + 2) % 8][0]
        if i % 3 == 0:
            F[f"calls/{d(i, 3)}-{i}.md"] = f"# 通话记录\n\n客户：{c}\n\n对接人：{contact}\n\n时长：{30 + i} 分钟\n\n结果：{['成交', '流失', '待跟进', '成交', '待跟进'][i % 5]}\n\n## 下一步\n\n- 周五前发送报价\n"
        else:
            F[f"calls/{d(i, 3)}-{i}.md"] = f"# Call notes\n\nCustomer: {c}\n\nContact: {contact}\n\nDuration: {30 + i} min\n\nOutcome: {outc[i % 5]}\n\n## Next step\n\n- Send the quote by Friday\n"


def expenses():
    cats = ["travel", "meals", "software", "travel", "office"]
    for f in range(4):
        zh = f % 2 == 0
        h = "日期,员工,类别,金额,已审批,票据号\n" if zh else "Date,Employee,Category,Amount,Approved,Receipt\n"
        rows = ""
        for r in range(8):
            i = f * 8 + r
            rows += f"{d(i, 4)},{STAFF[i % 8][0]},{cats[i % 5]},{'¥' if zh else '$'}{120 + i * 37},{['yes', 'yes', 'no'][i % 3]},RC{1000 + i}\n"
        F[f"finance/expenses-{f + 1}.csv"] = h + rows


def releases():
    for i in range(14):
        v = f"v1.{i}.0"
        own = STAFF[(i * 5) % 8][0]
        if i % 2:
            F[f"releases/{v}.md"] = f"---\nversion: {v}\nreleased: {d(i, 5)}\nowner: {own}\n---\n\n# Release {v}\n\n## Features\n\n- Faster search index {i}\n- Export to CSV\n\n## Fixes\n\n- Login redirect loop\n\n## Known issues\n\n- Dark mode flicker on resize\n"
        else:
            F[f"releases/{v}.md"] = f"---\nversion: {v}\nreleased: {d(i, 5)}\nowner: {own}\n---\n\n# 发布 {v}\n\n## 新功能\n\n- 搜索索引提速 {i}\n- 支持导出 CSV\n\n## 修复\n\n- 登录跳转循环\n\n## 已知问题\n\n- 调整窗口时深色模式闪烁\n"


def staff():
    F["people/staff.csv"] = "Name,Position,Email\n" + "".join(f"{n},{p},{e}\n" for n, p, e in STAFF)
    F["people/员工.csv"] = "姓名,职位,邮箱\n" + "".join(f"{n},{p},{e}\n" for n, p, e in STAFF[:6])


def noise():
    F["misc/shopping.md"] = "# Shopping\n\n## Pantry\n\nRice, oil.\n"
    F["misc/随想.md"] = "# 随想\n\n今天路过一家旧书店。\n"
    F["misc/trip.md"] = "# Trip\n\n## Hotels\n\nTwo candidates.\n\n## Maps\n\nOffline copies.\n"
    F["misc/lyrics-idea.md"] = "# Idea\n\nA song about tides.\n"
    F["misc/notes.txt"] = "remember to water the plants\n"


GOLD = {
    "version": "structure-heldout-gold/1",
    "note": "Frozen before the first run on this corpus. Classes are found by their member files (prefix); properties by any listed label; text and long_text are one type; a relation names the prefix of the class the values point at.",
    "classes": {
        "incidents": {"prefix": "incidents/", "properties": {
            "reported": {"labels": ["reported", "Reported"], "type": "date"},
            "severity": {"labels": ["severity", "Severity"], "type": "enum"},
            "service": {"labels": ["service", "Service"], "type": "enum"},
            "reporter": {"labels": ["reporter", "Reporter"], "type": "person", "relation": "people/"},
            "impact": {"labels": ["影响", "Impact"], "type": "text"}, "root_cause": {"labels": ["根因", "Root cause"], "type": "text"}}},
        "calls": {"prefix": "calls/", "properties": {
            "customer": {"labels": ["客户", "Customer"], "type": "organization"},
            "contact": {"labels": ["对接人", "Contact"], "type": "person", "relation": "people/"},
            "outcome": {"labels": ["结果", "Outcome"], "type": "enum"},
            "duration": {"labels": ["时长", "Duration"], "type": "text"}}},
        "expenses": {"prefix": "finance/", "properties": {
            "date": {"labels": ["日期", "Date"], "type": "date"}, "employee": {"labels": ["员工", "Employee"], "type": "person", "relation": "people/"},
            "category": {"labels": ["类别", "Category"], "type": "enum"}, "amount": {"labels": ["金额", "Amount"], "type": "amount"},
            "approved": {"labels": ["已审批", "Approved"], "type": "enum"}, "receipt": {"labels": ["票据号", "Receipt"], "type": "id"}}},
        "releases": {"prefix": "releases/", "properties": {
            "version": {"labels": ["version"], "type": "id"}, "released": {"labels": ["released"], "type": "date"},
            "owner": {"labels": ["owner"], "type": "person", "relation": "people/"},
            "features": {"labels": ["新功能", "Features"], "type": "text"}, "fixes": {"labels": ["修复", "Fixes"], "type": "text"}}},
        "staff": {"prefix": "people/", "properties": {
            "name": {"labels": ["姓名", "Name"], "type": "person"}, "position": {"labels": ["职位", "Position"], "type": "enum"},
            "email": {"labels": ["邮箱", "Email"], "type": "email"}}},
    },
    "noise_prefix": "misc/",
}


def build():
    incidents(); calls(); expenses(); releases(); staff(); noise()
    GOLD["record_files"] = len(F)
    return F


def manifest(g):
    m = "".join(f"{hashlib.sha256(t.encode()).hexdigest()}  project/{p}\n" for p, t in sorted(F.items()))
    return m + f"{hashlib.sha256(g.read_bytes()).hexdigest()}  gold/structure.json\n"


def main():
    ap = argparse.ArgumentParser()
    root = Path(__file__).resolve().parents[3] / "testing/super-ontology/structure-heldout"
    ap.add_argument("--out", default=str(root))
    ap.add_argument("--check", action="store_true")
    a = ap.parse_args()
    build()
    out = Path(a.out)
    g = out / "gold" / "structure.json"
    if a.check:
        sys.exit(0 if (out / "MANIFEST.sha256").read_text() == manifest(g) else 1)
    for p, t in sorted(F.items()):
        f = out / "project" / p
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_text(t, encoding="utf-8")
    g.parent.mkdir(parents=True, exist_ok=True)
    g.write_text(json.dumps(GOLD, ensure_ascii=False, indent=1, sort_keys=True) + "\n", encoding="utf-8")
    (out / "MANIFEST.sha256").write_text(manifest(g), encoding="utf-8")
    print(f"wrote {len(F)} files")


if __name__ == "__main__":
    main()

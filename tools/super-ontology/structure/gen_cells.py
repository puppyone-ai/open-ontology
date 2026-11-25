#!/usr/bin/env python3
"""Cell-value gold for the table classes (D93, lane AT): what every row of every table of the structure corpora should fill.

    python3 -B tools/super-ontology/structure/gen_cells.py [--check]

Three corpora, one gold file each (`gold/cells.json`), all written BEFORE the engine is run on them and never edited after a run:

* `testing/super-ontology/structure`           the people and project tables of the structure corpus (gen.py): the gold is computed from
                                               gen.py's own source lists (ZH_PEOPLE, EN_PEOPLE, ZH_PROJECTS, EN_PROJECTS), not from the CSV.
* `testing/super-ontology/structure-heldout`   the expense and staff tables of the held-out corpus (gen_heldout.py), from its STAFF list
                                               and the formulas of `expenses()`.
* `testing/super-ontology/structure-cells`     a corpus made here for values written in words: budgets as 六百八十五万元, 3.5亿, six point
                                               eight five million, $3.5m; percentages as 百分之八十八, 三成五, eighty-eight percent; dates as
                                               二〇二七年三月十六日, 3月16号, 下周二, March sixteenth, next Tuesday, tomorrow (the files carry
                                               their own date in the name, 2027-01-12, a Tuesday). Every cell is rendered from a typed truth
                                               (a number, a currency, a date, a share) by one of several writers, so the gold is the truth, not a
                                               reading of the text.

A gold row: `{file, row (the sheet row, header is row 1), name (the instance), cells {property: {kind, value}}, relations [names]}`. Properties are
named by the labels the file uses (`labels`); `kind` is one of `amount`, `date`, `ratio`, `number`, `text` (text, enum, id, email and version are
one kind) and `value` the canonical form (`1200000 CNY`, `2027-03-16`, `0.88`).
"""
import argparse, hashlib, json, sys
from decimal import Decimal
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.dont_write_bytecode = True
import gen  # noqa: E402
import gen_heldout  # noqa: E402

ROOT = HERE.parents[2]
NOTE = ("Frozen before the first run on these corpora. Generated from the corpus generators' own source data (not read from the rendered files). "
        "Properties are matched by any of their labels; text, enum, id, email and version values are one kind (`text`).")


def money(n, cur=None):
    return f"{n}" + (f" {cur}" if cur else "")


def cell(kind, value):
    return {"kind": kind, "value": value}


# ---------------------------------------------------------------------------------------------- structure corpus
def structure_gold():
    tables = []
    people_props = {"role": ["角色", "Role"], "department": ["部门", "Department"], "email": ["邮箱", "Email"], "employee_id": ["工号", "Employee ID"],
                    "start_date": ["入职日期", "Hire date"]}
    for path, people, mail in (("people/团队名册.csv", gen.ZH_PEOPLE, lambda n: gen.pinyin(n) + "@example.com"),
                               ("people/team-roster.csv", gen.EN_PEOPLE, lambda n: n.lower().replace(" ", ".") + "@example.com")):
        rows = []
        for i, (n, role, dept, eid, start) in enumerate(people):
            rows.append({"row": i + 2, "name": n, "cells": {"role": cell("text", role), "department": cell("text", dept), "email": cell("text", mail(n)),
                                                              "employee_id": cell("text", eid), "start_date": cell("date", start)}, "relations": []})
        tables.append({"file": path, "identity": ["姓名", "Name"], "properties": people_props, "rows": rows})
    proj_props = {"owner": ["负责人", "Owner"], "status": ["状态", "Status"], "budget": ["预算", "Budget"], "deadline": ["截止日期", "Deadline"]}
    for path, projects, people, scale, cur, mod in (("projects/项目一览.csv", gen.ZH_PROJECTS, gen.ZH_PEOPLE, 10_000, None, None),
                                                     ("projects/portfolio.csv", gen.EN_PROJECTS, gen.EN_PEOPLE, 1, "USD", 5)):
        rows = []
        for i, (n, o, status, budget, dl) in enumerate(projects):
            owner = people[o if mod is None else o % mod][0]
            amount = int(budget.rstrip("万").replace("$", "").replace(",", "")) * scale
            rows.append({"row": i + 2, "name": n, "cells": {"status": cell("text", status), "budget": cell("amount", money(amount, cur)),
                                                              "deadline": cell("date", dl)}, "relations": [owner]})
        tables.append({"file": path, "identity": ["项目", "Project"], "properties": proj_props, "rows": rows})
    return tables


# ---------------------------------------------------------------------------------------------- held-out corpus
def heldout_gold():
    tables = []
    cats = ["travel", "meals", "software", "travel", "office"]
    props = {"date": ["日期", "Date"], "employee": ["员工", "Employee"], "category": ["类别", "Category"], "amount": ["金额", "Amount"],
             "approved": ["已审批", "Approved"]}
    for f in range(4):
        zh = f % 2 == 0
        rows = []
        for r in range(8):
            i = f * 8 + r
            rows.append({"row": r + 2, "name": f"RC{1000 + i}", "cells": {
                "date": cell("date", gen_heldout.d(i, 4)), "category": cell("text", cats[i % 5]),
                "amount": cell("amount", money(120 + i * 37, "CNY" if zh else "USD")),
                "approved": cell("text", ["yes", "yes", "no"][i % 3])}, "relations": [gen_heldout.STAFF[i % 8][0]]})
        tables.append({"file": f"finance/expenses-{f + 1}.csv", "identity": ["票据号", "Receipt"], "properties": props, "rows": rows})
    sp = {"position": ["职位", "Position"], "email": ["邮箱", "Email"]}
    for path, staff in (("people/staff.csv", gen_heldout.STAFF), ("people/员工.csv", gen_heldout.STAFF[:6])):
        rows = [{"row": i + 2, "name": n, "cells": {"position": cell("text", p), "email": cell("text", e)}, "relations": []} for i, (n, p, e) in enumerate(staff)]
        tables.append({"file": path, "identity": ["姓名", "Name"], "properties": sp, "rows": rows})
    return tables


# ---------------------------------------------------------------------------------------------- words corpus
NAMES_ZH = ["陈子墨", "周若溪", "吴启明", "孙晓棠", "林嘉禾", "郑一鸣", "宋雨桐", "韩知远"]
NAMES_EN = ["Daniel Kim", "Karen Wu", "Ralph Evans", "Sandy Cho", "Fred Lang", "Kathy Moss", "Albert Shaw", "Junior Park"]
PROJ_ZH = ["星河", "北辰", "青云", "远山", "云帆", "海岚", "天枢", "玉衡"]
PROJ_EN = ["Beacon", "Atlas", "Harbor", "Summit", "Lantern", "Meridian", "Compass", "Anchor"]
ZH_DIGITS = "零一二三四五六七八九"


def zh_num(n):
    """Chinese numerals of a whole number below 10^8 that is a multiple of 10,000 or below 10,000 (the writer of the generator)."""
    def below_wan(x):
        out, units = "", [(1000, "千"), (100, "百"), (10, "十")]
        zero = False
        for u, c in units:
            d, x = divmod(x, u)
            if d:
                if zero:
                    out += "零"
                    zero = False
                out += ("" if (u == 10 and d == 1 and not out) else ZH_DIGITS[d]) + c
            elif out:
                zero = True
        if x:
            out += ("零" if zero else "") + ZH_DIGITS[x]
        return out
    wan, rest = divmod(n, 10_000)
    out = ""
    if wan:
        out = below_wan(wan) + "万"
    if rest:
        out += ("零" if wan and rest < 1000 else "") + below_wan(rest)
    return out


def en_words(n):
    small = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen",
             "sixteen", "seventeen", "eighteen", "nineteen"]
    tens = {2: "twenty", 3: "thirty", 4: "forty", 5: "fifty", 6: "sixty", 7: "seventy", 8: "eighty", 9: "ninety"}

    def under_1000(x):
        parts = []
        if x >= 100:
            parts.append(small[x // 100] + " hundred")
            x %= 100
        if x >= 20:
            parts.append(tens[x // 10] + ("-" + small[x % 10] if x % 10 else ""))
        elif x:
            parts.append(small[x])
        return " ".join(parts)
    out = []
    for scale, name in ((1_000_000, "million"), (1_000, "thousand")):
        if n >= scale:
            out.append(under_1000(n // scale) + " " + name)
            n %= scale
    if n:
        out.append(under_1000(n))
    return " ".join(out)


# (amount in whole units, currency) with a writer per language; every writer states the same truth
def write_amount_zh(n, cur, style):
    sym = {"CNY": "¥", "USD": "$", "EUR": "€"}[cur]
    word = {"CNY": "元", "USD": "美元", "EUR": "欧元"}[cur]
    if style == 0:
        return f"{zh_num(n)}{word}"
    if style == 1:
        return f"{n // 10_000}万{word}" if n % 10_000 == 0 else f"{sym}{n:,}"
    if style == 2:
        return f"{sym}{n:,}"
    if style == 3:
        return f"{n / 100_000_000:g}亿{word}" if n % 10_000_000 == 0 and n >= 100_000_000 else f"{n}{word}"
    return f"{n}{word}"


def write_amount_en(n, cur, style):
    sym = {"CNY": "¥", "USD": "$", "EUR": "€"}[cur]
    if style == 0:
        return f"{en_words(n)} {'yuan' if cur == 'CNY' else 'dollars' if cur == 'USD' else 'euros'}"
    if style == 1:
        return f"{sym}{n / 1_000_000:g}m" if n % 100_000 == 0 and n >= 1_000_000 else f"{sym}{n:,}"
    if style == 2:
        return f"{cur} {n / 1_000_000:g} million" if n % 100_000 == 0 and n >= 1_000_000 else f"{cur} {n}"
    return f"{sym}{n:,}"


def write_pct_zh(p, style):
    p = Decimal(str(p))
    if style == 0:
        whole = int(p)
        s = zh_num(whole) or "零"
        if p != whole:
            s += "点" + "".join(ZH_DIGITS[int(c)] for c in str(p).split(".")[1])
        return f"百分之{s}"
    if style == 1:
        return f"{p}%"
    if style == 2 and p % 5 == 0 and p % 10 != 0:
        return f"{ZH_DIGITS[int(p) // 10]}成{ZH_DIGITS[int(p) % 10]}"
    return f"{p}%"


def write_pct_en(p, style):
    p = Decimal(str(p))
    if style == 0 and p == int(p):
        return f"{en_words(int(p))} percent"
    if style == 1:
        return f"{p} percent"
    return f"{p}%"


DOC = "2027-01-12"  # a Tuesday


def date_zh(iso, style):
    y, m, d = (int(x) for x in iso.split("-"))
    if style == 0:
        return "".join(ZH_DIGITS[int(c)] for c in str(y)) + f"年{zh_num(m)}月{zh_num(d)}日"
    if style == 1:
        return f"{m}月{d}号"
    if style == 2:
        return f"{zh_num(m)}月{zh_num(d)}日"
    return iso


ORD = {5: "fifth", 10: "tenth", 16: "sixteenth", 3: "third", 18: "eighteenth", 21: "twenty-first", 25: "twenty-fifth", 12: "twelfth", 20: "twentieth"}
MONTH = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"]


def date_en(iso, style):
    y, m, d = (int(x) for x in iso.split("-"))
    if style == 0 and d in ORD:
        return f"{MONTH[m - 1]} {ORD[d]}"
    if style == 1 and d in ORD:
        return f"{d}th of {MONTH[m - 1]}".replace("1th", "1st").replace("2th", "2nd").replace("3th", "3rd")
    if style == 2:
        return f"{MONTH[m - 1]} {d}, {y}"
    return iso


def budget_rows(lang, f):
    """Eight rows per file; every row states one typed truth with one of the writers."""
    rows = []
    amounts = [(6_850_000, "CNY"), (1_420_000, "CNY"), (2_480_000, "CNY"), (350_000_000, "CNY"), (980_000, "CNY"), (3_500_000, "USD"), (2_500_000, "USD"), (200_000, "EUR")]
    pcts = [88, 71, 85, "12.5", 35, 60, 45, 95]
    dates = ["2027-03-16", "2027-03-05", "2027-02-10", "2027-03-10", "2027-03-18", "2027-04-01", "2027-02-26", "2027-03-20"]
    relative = {"下周二": "2027-01-19", "明天": "2027-01-13", "next Tuesday": "2027-01-19", "tomorrow": "2027-01-13", "后天": "2027-01-14", "next Saturday": "2027-01-23"}
    status_zh = ["进行中", "已完成", "进行中", "暂停", "进行中", "已完成", "进行中", "暂停"]
    status_en = ["Active", "Done", "Active", "On hold", "Active", "Done", "Active", "On hold"]
    for r in range(8):
        i = (f * 3 + r) % 8
        n, cur = amounts[i]
        # zh and en amounts cannot all be written in every style; the writer falls back to digits with the same truth.
        if lang == "zh":
            amt = write_amount_zh(n, cur, (r + f) % 4)
            pct = write_pct_zh(pcts[i], (r + f) % 3)
            due_iso, due = dates[i], None
            if r % 4 == 3:
                due = ["下周二", "明天", "后天"][f % 3]
                due_iso = relative[due]
            else:
                due = date_zh(due_iso, (r + f) % 4)
                if (r + f) % 4 in (1, 2):
                    # a year-less date is read against the document date (2027-01-12): the truth is the same ISO date only within 150 days.
                    pass
            name, owner, status = PROJ_ZH[i] + ("" if f == 0 else f"{f + 1}"), NAMES_ZH[i], status_zh[i]
        else:
            amt = write_amount_en(n, cur, (r + f) % 4)
            pct = write_pct_en(pcts[i], (r + f) % 3)
            due_iso, due = dates[i], None
            if r % 4 == 3:
                due = ["next Tuesday", "tomorrow", "next Saturday"][f % 3]
                due_iso = relative[due]
            else:
                due = date_en(due_iso, (r + f) % 3)
            name, owner, status = PROJ_EN[i] + ("" if f == 0 else f" {f + 1}"), NAMES_EN[i], status_en[i]
        rows.append({"name": name, "owner": owner, "amount": amt, "amount_truth": money(n, cur), "pct": pct, "pct_truth": str(Decimal(str(pcts[i])) / 100),
                     "due": due, "due_truth": due_iso, "status": status})
    return rows


def cells_corpus():
    files, tables = {}, []
    # Both languages' labels for every property: the zh and en tables may be read as one class, named after either.
    props = {"owner": ["负责人", "Owner"], "budget": ["预算", "Budget"], "completion": ["完成率", "Completion"], "due": ["截止日期", "Due"], "status": ["状态", "Status"]}
    for lang, head in (("zh", "项目,负责人,预算,完成率,截止日期,状态"), ("en", "Project,Owner,Budget,Completion,Due,Status")):
        for f in range(3):
            path = f"finance/budget-{lang}-{f + 1}-{DOC}.csv"
            body, rows = head + "\n", []
            for r, row in enumerate(budget_rows(lang, f)):
                cells = [row["name"], row["owner"], row["amount"], row["pct"], row["due"], row["status"]]
                body += ",".join(f'"{c}"' if "," in c else c for c in cells) + "\n"
                rows.append({"row": r + 2, "name": row["name"], "cells": {
                    "budget": cell("amount", row["amount_truth"]), "completion": cell("ratio", row["pct_truth"]),
                    "due": cell("date", row["due_truth"]), "status": cell("text", row["status"])}, "relations": [row["owner"]]})
            files[path] = body
            tables.append({"file": path, "identity": ["项目", "Project"], "properties": props, "rows": rows})
    return files, tables


def gold_doc(tables):
    return {"version": "structure-cells-gold/1", "note": NOTE, "tables": tables}


def write_all(check):
    ok = True
    cells_files, cells_tables = cells_corpus()
    corpora = {"structure": structure_gold(), "structure-heldout": heldout_gold(), "structure-cells": cells_tables}
    for name, tables in corpora.items():
        base = ROOT / "testing/super-ontology" / name
        g = base / "gold" / "cells.json"
        text = json.dumps(gold_doc(tables), ensure_ascii=False, indent=1, sort_keys=True) + "\n"
        files = cells_files if name == "structure-cells" else {}
        manifest = "".join(f"{hashlib.sha256(t.encode()).hexdigest()}  project/{p}\n" for p, t in sorted(files.items()))
        manifest += f"{hashlib.sha256(text.encode()).hexdigest()}  gold/cells.json\n"
        mf = base / ("MANIFEST.sha256" if name == "structure-cells" else "MANIFEST-cells.sha256")
        if check:
            same = g.exists() and g.read_text() == text and mf.exists() and mf.read_text() == manifest
            ok &= same
            print(("ok " if same else "MISMATCH ") + name)
            continue
        g.parent.mkdir(parents=True, exist_ok=True)
        g.write_text(text, encoding="utf-8")
        for p, t in sorted(files.items()):
            f = base / "project" / p
            f.parent.mkdir(parents=True, exist_ok=True)
            f.write_text(t, encoding="utf-8")
        mf.write_text(manifest, encoding="utf-8")
        print(f"wrote {name}: {sum(len(t['rows']) for t in tables)} rows in {len(tables)} tables")
    return ok


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    a = ap.parse_args()
    gen.build()
    gen_heldout.build()
    sys.exit(0 if write_all(a.check) else 1)


if __name__ == "__main__":
    main()

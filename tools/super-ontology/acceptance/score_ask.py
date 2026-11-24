"""Scores the Ask run (3): correct, cited, honest on the unanswerable.

    python3 -B score_ask.py --ask-results ask-results.json --questions gold/questions.jsonl [--out score-ask.json]

The answers come from the app's own `ask-run` channel (ask_questions.mjs). Correct is decided without a model, so it can be re-run and
audited: every number or date the gold answer states must appear in the answer (thousands separators are ignored, a decimal is accepted within half a unit of its last digit, and the word "percent" is not counted), and at least half of its other significant words (Latin words of 3+ letters outside a small stop list, runs of 2+ Chinese
characters) must too. Cited is true when one of the answer's sources is a file the gold names. An unanswerable question is honest when
the app declined (status unanswerable) and did not cite anything.
"""
import argparse, json, re, sys
from pathlib import Path

STOP = set("percent the and for are was were that this with from into its has have had not but all any can will shall per after before over under than then they them their there here which who what when where how does did been being also only each both more most such yes".split())
NUM = re.compile(r"\d{4}-\d{2}-\d{2}|\d[\d,]*(?:\.\d+)?")


def norm(s):
    return s.replace("，", ",").replace("：", ":").lower()


def numbers(s):
    out = set()
    for m in NUM.findall(norm(s)):
        out.add(m.replace(",", "").rstrip("."))
    return out


def number_present(need, answer_text):
    """A gold number is present when the answer has it, or (for a decimal) a number within half a unit of the gold's last digit: 45.3 accepts 45.26."""
    if need in answer_text.replace(",", ""):
        return True
    if re.fullmatch(r"\d+\.\d+", need):
        tol = 0.5 * 10 ** -len(need.split(".")[1])
        return any(abs(float(x) - float(need)) <= tol for x in re.findall(r"\d+(?:\.\d+)?", answer_text.replace(",", "")))
    return False


def words(s):
    s = NUM.sub(" ", norm(s))
    w = {x for x in re.findall(r"[a-z]{3,}", s) if x not in STOP}
    w |= set(re.findall(r"[一-鿿]{2,}", s))
    return w


MONTHS = {m: i for i, m in enumerate("january february march april may june july august september october november december".split(), 1)}


def iso_dates(text):
    """Dates the answer writes in words ("2 February 2027", "February 2, 2027", "2027年2月2日") as ISO strings, so 2027-02-02 is found."""
    t = norm(text)
    out = []
    for d, m, y in re.findall(r"(\d{1,2})(?:st|nd|rd|th)? (" + "|".join(MONTHS) + r"),? (\d{4})", t):
        out.append(f"{y}-{MONTHS[m]:02d}-{int(d):02d}")
    for m, d, y in re.findall(r"(" + "|".join(MONTHS) + r") (\d{1,2})(?:st|nd|rd|th)?,? (\d{4})", t):
        out.append(f"{y}-{MONTHS[m]:02d}-{int(d):02d}")
    for y, m, d in re.findall(r"(\d{4})年\s*(\d{1,2})月\s*(\d{1,2})", t):
        out.append(f"{y}-{int(m):02d}-{int(d):02d}")
    return " ".join(out)


def correct(gold, answer):
    """Every ';'-separated part of the gold answer must be present on its own."""
    parts = [p for p in gold.split(";") if p.strip()]
    return all(correct_part(p, answer) for p in parts) if parts else False


def correct_part(gold, answer):
    a = norm(answer) + " " + iso_dates(answer)
    if any(not number_present(n, a) for n in numbers(gold)):
        return False
    need = words(gold)
    if not need:
        return bool(numbers(gold))
    have = sum(1 for w in need if w in a)
    return have / len(need) >= 0.5


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ask-results", required=True)
    ap.add_argument("--questions", required=True)
    ap.add_argument("--out")
    a = ap.parse_args()
    qs = {q["id"]: q for q in (json.loads(l) for l in Path(a.questions).read_text().splitlines() if l)}
    res = {r["id"]: r for r in json.loads(Path(a.ask_results).read_text())}
    rows, by_kind = [], {}
    for qid, q in qs.items():
        r = res.get(qid)
        if r is None:
            rows.append({"id": qid, "kind": q["kind"], "missing": True})
            continue
        status = r.get("status")
        cited_files = {s["path"].split("project/", 1)[-1] for s in r.get("sources", []) if s.get("path")}
        row = {"id": qid, "kind": q["kind"], "status": status, "ms": r.get("ms"), "sources": sorted(cited_files)}
        if q["answerable"]:
            row["declined"] = status == "unanswerable"
            row["correct"] = status == "answered" and correct(q["answer"], r.get("answer", ""))
            row["cited"] = bool(cited_files & set(q["files"]))
            row["correct_and_cited"] = row["correct"] and row["cited"]
        else:
            row["honest"] = status == "unanswerable" and not cited_files
            row["made_up"] = status == "answered"
        rows.append(row)
    ans = [r for r in rows if "correct" in r]
    un = [r for r in rows if "honest" in r]
    pct = lambda n, d: round(100 * n / d, 1) if d else None
    kinds = {}
    for r in ans:
        k = kinds.setdefault(r["kind"], [0, 0])
        k[0] += r["correct"]; k[1] += 1
    summary = {
        "answerable": len(ans), "correct": sum(r["correct"] for r in ans), "correct_pct": pct(sum(r["correct"] for r in ans), len(ans)),
        "cited": sum(r["cited"] for r in ans), "cited_pct": pct(sum(r["cited"] for r in ans), len(ans)),
        "correct_and_cited": sum(r["correct_and_cited"] for r in ans), "wrongly_declined": sum(r["declined"] for r in ans),
        "unanswerable": len(un), "honest": sum(r["honest"] for r in un), "made_up": sum(r["made_up"] for r in un),
        "by_kind": {k: f"{c}/{n}" for k, (c, n) in kinds.items()},
        "not_run": [r["id"] for r in rows if r.get("missing")],
    }
    doc = {"summary": summary, "rows": rows}
    if a.out:
        Path(a.out).write_text(json.dumps(doc, ensure_ascii=False, indent=1))
    print(json.dumps(summary, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()

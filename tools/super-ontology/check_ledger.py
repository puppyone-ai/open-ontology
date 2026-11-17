#!/usr/bin/env python3
"""Ledger consistency check (Super Ontology 50 sec.9-10 rule 2, card B-S1-06).

Ledger: markdown table in execution/super-ontology/ledger.md with columns
card | loop | status | branch | commits | PR | evidence | notes.
Rules:
  * status in todo/in_progress/review/merged/signed/dropped; dropped needs a reason in notes
  * card ids are unique and well formed
  * every merged/signed card has at least one `Plan: <card>` commit on the integration ref
  * every `Plan: SO-...` seen on the integration ref has a ledger row
Exit 0 ok, 1 violations, 2 usage/git error.
"""
import argparse
import re
import sys

from check_trailers import PLAN_RE, git, trailer_values, commits

STATUSES = {"todo", "in_progress", "review", "merged", "signed", "dropped"}
EMPTY_NOTES = {"", "-", "—", "–"}


def parse_ledger(text):
    rows = []
    for n, line in enumerate(text.splitlines(), 1):
        s = line.strip()
        if not s.startswith("|"):
            continue
        cells = [c.strip() for c in s.strip("|").split("|")]
        if all(re.fullmatch(r":?-{2,}:?", c) for c in cells if c) and any(cells):
            continue
        if not cells[0].startswith("SO-"):
            continue  # header or non-card row
        rows.append((n, cells))
    return rows


def check(ledger_text, plans_on_ref):
    """plans_on_ref: set of Plan values found on the integration ref."""
    bad = []
    seen = set()
    cards = {}
    for n, c in parse_ledger(ledger_text):
        c += [""] * (8 - len(c))
        card, status, notes = c[0], c[2], c[7]
        if not PLAN_RE.match(card):
            bad.append((n, f"malformed card id {card!r}"))
        if card in seen:
            bad.append((n, f"duplicate ledger row for {card}"))
        seen.add(card)
        if status not in STATUSES:
            bad.append((n, f"{card}: invalid status {status!r}"))
        if status == "dropped" and notes in EMPTY_NOTES:
            bad.append((n, f"{card}: dropped requires a reason in notes"))
        cards[card] = (n, status)
    for card, (n, status) in cards.items():
        if status in ("merged", "signed") and card not in plans_on_ref:
            bad.append((n, f"{card}: status {status} but no commit with this Plan: on the integration ref"))
    for p in sorted(plans_on_ref):
        if p not in cards:
            bad.append((0, f"{p}: Plan: on the integration ref has no ledger row"))
    return bad


def plans_on(repo, ref, base=None):
    rng = f"{base}..{ref}" if base else ref
    found = set()
    for _sha, _an, _ae, _cn, _ce, body in commits(repo, rng):
        for p in trailer_values(body, "Plan"):
            if p.startswith("SO-"):
                found.add(p)
    return found


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--repo", default=".")
    ap.add_argument("--ledger", default="execution/super-ontology/ledger.md")
    ap.add_argument("--ref", default="HEAD", help="integration branch ref")
    ap.add_argument("--base", help="only consider commits in <base>..<ref> (default: whole history)")
    a = ap.parse_args(argv)
    try:
        import os
        path = a.ledger if os.path.isabs(a.ledger) else os.path.join(a.repo, a.ledger)
        text = open(path, encoding="utf-8").read()
        bad = check(text, plans_on(a.repo, a.ref, a.base))
    except (RuntimeError, OSError) as e:
        print(e, file=sys.stderr)
        return 2
    for n, m in bad:
        print(f"ledger:{n}: {m}" if n else f"ledger: {m}")
    print(f"ledger violations: {len(bad)}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())

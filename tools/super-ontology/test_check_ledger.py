import check_ledger as cl
from test_check_trailers import commit, repo

HEAD = "| 卡号 | 闭环 | 状态 | 分支 | 提交（首 → 末） | PR | 证据 | 备注 |\n|---|---|---|---|---|---|---|---|\n"


def row(card, status="merged", notes="—"):
    return f"| {card} | L1 | {status} | so/x | a → b | #1 | e.md | {notes} |\n"


def msgs(text, plans):
    return [m for _, m in cl.check(text, set(plans))]


def test_ledger_ok():
    t = HEAD + row("SO-B-S1-01") + row("SO-SPEC", "signed") + row("SO-B-S1-02", "todo")
    assert msgs(t, {"SO-B-S1-01", "SO-SPEC"}) == []


def test_ledger_merged_without_commit():
    t = HEAD + row("SO-B-S1-01")
    assert any("no commit" in m for m in msgs(t, set()))
    t = HEAD + row("SO-B-S1-01", "review")
    assert msgs(t, set()) == []


def test_ledger_plan_without_row():
    assert any("no ledger row" in m for m in msgs(HEAD, {"SO-B-S1-09"}))


def test_ledger_bad_status_and_dup_and_dropped():
    t = HEAD + row("SO-B-S1-01", "done") + row("SO-B-S1-02", "todo") + row("SO-B-S1-02", "todo") + row("SO-B-S1-03", "dropped")
    m = msgs(t, set())
    assert any("invalid status" in x for x in m)
    assert any("duplicate" in x for x in m)
    assert any("dropped requires" in x for x in m)
    assert msgs(HEAD + row("SO-B-S1-03", "dropped", "superseded by 04"), set()) == []


def test_ledger_malformed_card_id():
    assert any("malformed" in m for m in msgs(HEAD + row("SO-Q-S1-1", "todo"), set()))


def test_ledger_cli_with_git_repo(tmp_path):
    r = repo(tmp_path)
    commit(r, "a\n\nPlan: SO-B-S1-01")
    led = r / "execution/super-ontology/ledger.md"
    led.parent.mkdir(parents=True)
    led.write_text(HEAD + row("SO-B-S1-01"))
    assert cl.main(["--repo", str(r), "--ref", "HEAD"]) == 0
    led.write_text(HEAD + row("SO-B-S1-02"))
    assert cl.main(["--repo", str(r), "--ref", "HEAD"]) == 1
    # --base limits history: pre-base Plans are not demanded in the ledger
    assert cl.main(["--repo", str(r), "--ref", "HEAD", "--base", "HEAD"]) == 1  # merged card 02 w/o commit
    assert cl.main(["--repo", str(r), "--ledger", "missing.md"]) == 2


def test_frontend_rows_checked_only_against_puppyone_plans():
    text = HEAD + row("SO-F-S1-01")
    assert cl.check(text, set()) == []
    assert cl.check(text, set(), {"SO-F-S1-01"}) == []
    bad = cl.check(text, set(), set())
    assert len(bad) == 1 and "Puppyone" in bad[0][1]


def test_base_limits_only_the_new_plan_direction():
    text = HEAD + row("SO-B-S1-01")
    # a merged row backed by history outside the base range is fine
    assert cl.check(text, {"SO-B-S1-01"}, None, set()) == []
    # a new Plan in base..ref still needs a row
    assert any("SO-B-S9-09" in m[1] for m in cl.check(text, {"SO-B-S1-01", "SO-B-S9-09"}, None, {"SO-B-S9-09"}))
    # a merged row with no commit anywhere in the whole history is still a violation
    assert cl.check(text, set(), None, set()) != []
